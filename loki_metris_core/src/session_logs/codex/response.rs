//! 把 Codex 逐响应记录映射到 core 单次事实与严格身份写入策略。

use super::*;

/// adapter 私有标记只供摄入层选择 core 严格写入 API，不包含原始响应 ID。
const RESPONSE_CONSISTENCY_KEY: &str = "codex-response-v1";

/// 识别本 adapter 发出的现代响应调用；core 不解释此协议标记。
pub fn is_response_call(call: &UsageCall) -> bool {
    call.adapter_consistency_key.as_deref() == Some(RESPONSE_CONSISTENCY_KEY)
}

/// 仅接收有身份、时间及完整单次字段且属于当前线程的现代记录。
/// 首次有效记录推进 core 权威来源，复制前缀和坏记录均不能推进。
pub(super) fn parse_response_record(
    context: &mut JsonlParseContext,
    payload: RolloutPayload,
    timestamp: Option<&Value>,
    provenance: &SourceProvenance,
) -> LineOutcome {
    let opens_owned_segment = context.awaits_owned_start();
    let Some(thread_id) = payload
        .thread_id
        .as_deref()
        .filter(|id| valid_protocol_id(id))
    else {
        return LineOutcome::MissingUsage;
    };
    if stable_id("thread", thread_id) != context.thread_key {
        // 只有合法且明确属于其他线程的复制前缀可忽略；缺失身份仍须记质量警告。
        return if opens_owned_segment { LineOutcome::Ignored } else { LineOutcome::InvalidUsage };
    }
    let Some(response_id) = payload
        .response_id
        .as_deref()
        .filter(|id| valid_protocol_id(id))
    else {
        return LineOutcome::MissingUsage;
    };
    let Some(raw) = payload.usage else {
        return LineOutcome::MissingUsage;
    };
    if raw.input_tokens.is_none() || raw.output_tokens.is_none() || raw.total_tokens.is_none() {
        return LineOutcome::MissingUsage;
    }
    let usage = match normalize_usage(raw) {
        Ok(usage) => usage,
        Err(()) => return LineOutcome::InvalidUsage,
    };
    let Some(occurred_at_epoch_ms) =
        parse_timestamp(timestamp).or_else(|| parse_timestamp(payload.timestamp.as_ref()))
    else {
        return LineOutcome::InvalidTimestamp;
    };
    // 身份、用量和时间全部有效后才开启自身区段；保留先前旧用量归属歧义。
    if opens_owned_segment {
        context.ownership = RolloutOwnershipState::Owned;
        context.call_sequence = 0;
    }
    let selected = context.usage_authority.observe_response(usage);
    context.call_sequence = context.call_sequence.saturating_add(1);
    let logical_call_id = stable_id(
        "call",
        &format!("{}\u{0}response\u{0}{response_id}", context.thread_key),
    );
    LineOutcome::Call(
        Box::new(UsageCall {
            logical_call_id,
            occurred_at_epoch_ms,
            model: context.model.clone(),
            reasoning_effort: context.reasoning_effort.clone(),
            project_key: context.project_key.clone(),
            thread_key: context.thread_key.clone(),
            project_label: context.project_label.clone(),
            thread_label: context.thread_label.clone(),
            usage: selected.usage,
            adapter_consistency_key: Some(RESPONSE_CONSISTENCY_KEY.into()),
            confidence: selected.confidence,
            provenance: vec![provenance.clone()],
        }),
        false,
        None,
    )
}

/// 限制协议 ID 的大小与空值，不把身份内容暴露为展示字段。
fn valid_protocol_id(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512
}
