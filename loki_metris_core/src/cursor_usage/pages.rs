//! 严格解释已知网页响应，拒绝部分分页和不合法数值。

use serde_json::Value;
use super::{CursorUsageError as Error, CursorUsagePages, MAX_EVENTS, PAGE_SIZE};

/// 已消毒且仅用于本次汇总的逐事件事实。
pub(super) struct Event {
    pub(super) timestamp: i64,
    pub(super) model: Option<String>,
    pub(super) tokens: [Option<u64>; 4],
    pub(super) without_usage: bool,
}

impl CursorUsagePages {
    /// 消费精确的下一页；失败不提交本页，也不能发布先前页面。
    pub fn push(&mut self, page: usize, body: &str) -> Result<(), Error> {
        if self.next_page() != Some(page) || page > MAX_EVENTS / PAGE_SIZE { return Err(Error::Incomplete) }
        if body.len() > 1024 * 1024 { return Err(Error::Malformed) }
        let value: Value = serde_json::from_str(body).map_err(|_| Error::Malformed)?;
        let object = value.as_object().ok_or(Error::Malformed)?;
        let rows = match object.get("usageEventsDisplay") {
            None => &[][..],
            Some(value) => value.as_array().ok_or(Error::Malformed)?.as_slice(),
        };
        // 账户网页接口在零条时返回 {}，并省略默认值和空数组；仅接受已知空形状。
        let total = match object.get("totalUsageEventsCount") {
            None if object.is_empty() || (object.len() == 1 && rows.is_empty()
                && object.contains_key("usageEventsDisplay")) => 0,
            total => total.and_then(Value::as_u64).filter(|total| *total <= MAX_EVENTS as u64)
                .ok_or(Error::Incomplete)? as usize,
        };
        if self.expected.is_some_and(|old| old != total) { return Err(Error::Incomplete) }
        let remaining = total.checked_sub(self.events.len()).ok_or(Error::Incomplete)?;
        if rows.len() != remaining.min(PAGE_SIZE) { return Err(Error::Incomplete) }
        let mut batch = std::collections::BTreeMap::new();
        for row in rows {
            let id = row.get("usageEventId").and_then(Value::as_str)
                .filter(|id| !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control))
                .ok_or(Error::Malformed)?;
            // 只在瞬时分页内保留不可显示的稳定散列，不保存账户或会话正文。
            let id = crate::stable_id("cursor-event", id);
            if self.events.contains_key(&id) || batch.contains_key(&id) { return Err(Error::Incomplete) }
            let timestamp = row.get("timestamp").and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
                .filter(|ms| *ms >= self.range.start_ms && *ms < self.range.end_ms).ok_or(Error::Malformed)?;
            let usage = match row.get("tokenUsage") {
                None | Some(Value::Null) => None,
                Some(value) => Some(value.as_object().ok_or(Error::Malformed)?),
            };
            let mut tokens = [None; 4];
            for (slot, key) in tokens.iter_mut().zip(["inputTokens", "outputTokens", "cacheReadTokens", "cacheWriteTokens"]) {
                *slot = match usage.and_then(|object| object.get(key)) {
                    None | Some(Value::Null) => None,
                    Some(number) => Some(number.as_u64().ok_or(Error::Malformed)?),
                };
            }
            let model = row.get("model").and_then(Value::as_str).and_then(crate::safe_model_label);
            let without_usage = tokens.iter().all(Option::is_none);
            batch.insert(id, Event { timestamp, model, tokens, without_usage });
        }
        self.events.extend(batch);
        self.expected = Some(total);
        self.next_page += 1;
        Ok(())
    }
}
