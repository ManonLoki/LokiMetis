//! 负责 Token 规范化、增量检查点和受限时间戳编码。

use super::{
    CUMULATIVE_CHECKPOINT_PREFIX, MAX_SUPPORTED_TIMESTAMP_MS, MIN_SUPPORTED_TIMESTAMP_MS,
    RawTokenUsage, Timestamp, TokenUsage, Value,
};

/// 把上游 Token 字段交给 core 校验并建立单次事实。
pub(super) fn normalize_usage(raw: RawTokenUsage) -> Result<TokenUsage, ()> {
    TokenUsage::new_with_availability(
        raw.input_tokens.unwrap_or_default(),
        raw.cached_input_tokens,
        raw.cache_write_input_tokens,
        raw.output_tokens.unwrap_or_default(),
        raw.reasoning_output_tokens,
        raw.total_tokens,
    )
    .map_err(|_| ())
}

/// 为调用指纹保留既有已知数值表示，并显式区分上游未提供。
pub(super) fn optional_token_fingerprint(value: Option<u64>) -> String {
    value.map_or_else(|| "unavailable".to_owned(), |value| value.to_string())
}

/// 增量扫描恢复用的累计与上一 `last` 快照，不含正文、路径或身份。
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(test)]
pub(crate) struct IncrementalUsageCheckpoint {
    /// 最近一次可信会话累计。
    pub(crate) cumulative: TokenUsage,
    /// 最近一次观察到的单次 `last`；缺失时无法识别 same-last 重放。
    pub(crate) last: Option<TokenUsage>,
}

/// 把累计 Token 与最近 `last` 编码成不含用户内容的 adapter 私有检查点。
pub(super) fn encode_incremental_checkpoint(
    sequence: u64,
    usage: &TokenUsage,
    last: Option<&TokenUsage>,
) -> String {
    let mut encoded = format!(
        "{CUMULATIVE_CHECKPOINT_PREFIX}:{sequence:020}:{}:{}",
        encode_token_fields(usage),
        u8::from(last.is_some()),
    );
    if let Some(last) = last {
        encoded.push(':');
        encoded.push_str(&encode_token_fields(last));
    }
    encoded
}

/// 编码一组 Token 字段，顺序与恢复逻辑保持一致。
fn encode_token_fields(usage: &TokenUsage) -> String {
    format!(
        "{}:{}:{}:{}:{}:{}:{}",
        usage.input_tokens,
        encode_optional_token(usage.cached_input_tokens),
        encode_optional_token(usage.cache_write_input_tokens),
        usage.output_tokens,
        encode_optional_token(usage.reasoning_output_tokens),
        usage.total_tokens,
        u8::from(usage.total_is_derived),
    )
}

/// 从 adapter 私有检查点恢复累计与最近 `last`；错误或其他版本一律保守拒绝。
#[cfg(test)]
pub(crate) fn restore_incremental_checkpoint(
    value: Option<&str>,
) -> Option<IncrementalUsageCheckpoint> {
    let value = value?;
    let mut parts = value.split(':');
    if parts.next()? != CUMULATIVE_CHECKPOINT_PREFIX {
        return None;
    }
    let sequence = parts.next()?;
    if sequence.len() != 20 || !sequence.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    sequence.parse::<u64>().ok()?;
    let cumulative = decode_token_fields(&mut parts)?;
    let last = match parts.next()? {
        "0" => None,
        "1" => Some(decode_token_fields(&mut parts)?),
        _ => return None,
    };
    if parts.next().is_some() {
        return None;
    }
    Some(IncrementalUsageCheckpoint { cumulative, last })
}

/// 从检查点剩余字段解析一组 Token 数值。
#[cfg(test)]
fn decode_token_fields(parts: &mut std::str::Split<'_, char>) -> Option<TokenUsage> {
    let input = parts.next()?.parse::<u64>().ok()?;
    let cached = decode_optional_token(parts.next()?)?;
    let cache_write = decode_optional_token(parts.next()?)?;
    let output = parts.next()?.parse::<u64>().ok()?;
    let reasoning = decode_optional_token(parts.next()?)?;
    let total = parts.next()?.parse::<u64>().ok()?;
    let total_is_derived = match parts.next()? {
        "0" => false,
        "1" => true,
        _ => return None,
    };
    let usage = TokenUsage::new_with_availability(
        input,
        cached,
        cache_write,
        output,
        reasoning,
        (!total_is_derived).then_some(total),
    )
    .ok()?;
    (usage.total_tokens == total).then_some(usage)
}

/// 编码上游是否提供某个可选 Token 分量。
fn encode_optional_token(value: Option<u64>) -> String {
    value.map_or_else(|| "u".to_owned(), |value| value.to_string())
}

/// 解码可选 Token 分量；`u` 表示上游未提供。
#[cfg(test)]
fn decode_optional_token(value: &str) -> Option<Option<u64>> {
    if value == "u" {
        Some(None)
    } else {
        value.parse::<u64>().ok().map(Some)
    }
}

/// 解析数字毫秒/秒或 RFC3339 时间；失败时由调用方跳过事件并计入警告。
// 上游时间戳可能是数字（秒或毫秒，取决于具体事件来源）或 RFC3339 字符串。
// 区分“数字是秒还是毫秒”用的是一个经验量级判断：`±10_000_000_000`
// 按秒计算对应约公元 2286 年附近，按毫秒计算则对应 1970 年附近——
// 真实数据不可能出现“秒值大到超过 100 亿”的情况，所以小于这个阈值的
// 数字按“秒”处理再乘 1000 转毫秒，达到或超过阈值的数字本身已经是毫秒。
pub(super) fn parse_timestamp(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    if let Some(integer) = value.as_i64() {
        let milliseconds = if (-10_000_000_000..10_000_000_000).contains(&integer) {
            integer.checked_mul(1_000)?
        } else {
            integer
        };
        return supported_timestamp_millis(milliseconds);
    }
    if let Some(unsigned) = value.as_u64() {
        let integer = i64::try_from(unsigned).ok()?;
        let milliseconds = if integer < 10_000_000_000 {
            integer.checked_mul(1_000)?
        } else {
            integer
        };
        return supported_timestamp_millis(milliseconds);
    }
    parse_rfc3339_millis(value.as_str()?).and_then(supported_timestamp_millis)
}

/// 拒绝无法用当前四位年份日期契约表示的数字时间，避免后续静默丢桶。
fn supported_timestamp_millis(value: i64) -> Option<i64> {
    (MIN_SUPPORTED_TIMESTAMP_MS..=MAX_SUPPORTED_TIMESTAMP_MS)
        .contains(&value)
        .then_some(value)
}

/// 解析 rollout 常见 RFC3339 时间，并保留既有四位年份与大写分隔符契约。
pub(super) fn parse_rfc3339_millis(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
        || !has_supported_timezone_suffix(bytes)
    {
        return None;
    }
    value
        .parse::<Timestamp>()
        .ok()
        .map(|timestamp| timestamp.as_millisecond())
}

/// 只允许既有 `Z` 或 `±HH:MM` 结尾，避免 jiff 的宽松语法扩大索引接受集。
fn has_supported_timezone_suffix(value: &[u8]) -> bool {
    value.last() == Some(&b'Z')
        || (value.len() >= 6
            && matches!(value.get(value.len() - 6), Some(b'+' | b'-'))
            && value.get(value.len() - 3) == Some(&b':'))
}
