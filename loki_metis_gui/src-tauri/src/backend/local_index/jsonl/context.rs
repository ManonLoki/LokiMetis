//! 管理 rollout 事件分类、所有权边界和安全展示上下文。

use super::{
    JsonlParseContext, OWNED_START_TOLERANCE_MS, RolloutOwnershipState, RolloutPayload, TokenUsage,
    safe_path_basename, safe_thread_title, stable_id,
};

/// 识别不需要 payload 也能确认的正常非用量顶层记录。
pub(super) fn is_known_non_usage_envelope(envelope_kind: Option<&str>) -> bool {
    matches!(
        envelope_kind,
        Some("response_item" | "world_state" | "inter_agent_communication_metadata" | "compacted")
    )
}

/// 识别真实 rollout 中已确认的正常非用量事件，同时让未知子类型继续形成漂移警告。
pub(super) fn is_known_non_usage_event(
    envelope_kind: Option<&str>,
    payload_kind: Option<&str>,
) -> bool {
    if is_known_non_usage_envelope(envelope_kind) {
        return true;
    }
    envelope_kind == Some("event_msg")
        && matches!(
            payload_kind,
            Some(
                "agent_reasoning"
                    | "agent_message"
                    | "patch_apply_end"
                    | "task_started"
                    | "task_complete"
                    | "user_message"
                    | "sub_agent_activity"
                    | "thread_settings_applied"
                    | "mcp_tool_call_end"
                    | "web_search_end"
                    | "thread_goal_updated"
                    | "context_compacted"
                    | "turn_aborted"
                    | "image_generation_end"
                    | "thread_rolled_back"
            )
        )
}

/// 用首条 session 元数据固定 rollout owner；后续复制元数据不得覆盖身份。
// `stable_id`（定义在 discovery.rs）对原始 session id / 项目路径做单向哈希，
// 生成一个内容无关的稳定标识：同一个 session/项目每次都会得到同一个哈希值
// （所以可以用来去重、分组），但哈希值本身不能反推出原始工作目录路径或
// 真实 session id——这是本索引“不保存真实文件路径”这条隐私边界的具体实现。
// 例外是 `project_label`/`thread_label`：保存的是受控消毒后的路径末段/短标题。
pub(super) fn initialize_owner_context(
    context: &mut JsonlParseContext,
    payload: &RolloutPayload,
    owner_created_at_epoch_ms: Option<i64>,
) {
    if !matches!(
        context.ownership,
        RolloutOwnershipState::AwaitingOwnerMetadata
    ) {
        return;
    }
    if let Some(session_id) = payload.session_id.as_deref().or(payload.id.as_deref()) {
        context.thread_key = stable_id("thread", session_id);
    }
    update_dynamic_context(context, payload);
    context.ownership = if payload.forked_from_id.is_some() || payload.agent_path.is_some() {
        RolloutOwnershipState::AwaitingOwnedStart {
            owner_created_at_epoch_ms,
        }
    } else {
        RolloutOwnershipState::Owned
    };
}

/// 只在 fork 的 `started_at` 与 owner 创建时刻相符时开启自身用量区段。
pub(super) fn open_owned_segment_if_matching(
    context: &mut JsonlParseContext,
    task_started_at_epoch_ms: Option<i64>,
) {
    let matches_owner = match (&context.ownership, task_started_at_epoch_ms) {
        (
            RolloutOwnershipState::AwaitingOwnedStart {
                owner_created_at_epoch_ms: Some(owner_created_at_epoch_ms),
            },
            Some(task_started_at_epoch_ms),
        ) => {
            owner_created_at_epoch_ms.abs_diff(task_started_at_epoch_ms) <= OWNED_START_TOLERANCE_MS
        }
        _ => false,
    };
    if matches_owner {
        context.ownership = RolloutOwnershipState::Owned;
        context.call_sequence = 0;
    }
}

/// 更新项目、模型与展示标签；线程身份只允许由 owner 初始化函数写入。
pub(super) fn update_dynamic_context(context: &mut JsonlParseContext, payload: &RolloutPayload) {
    if let Some(project) = payload
        .project_id
        .as_deref()
        .or(payload.project_key.as_deref())
        .or(payload.cwd.as_deref())
    {
        context.project_key = Some(stable_id("project", project));
        context.project_label = safe_path_basename(project);
    }
    if let Some(nickname) = payload
        .agent_nickname
        .as_deref()
        .and_then(safe_thread_title)
    {
        context.thread_label = Some(nickname);
    }
    if let Some(model) = payload.model.as_deref() {
        context.model = safe_label(model);
    }
    if let Some(reasoning_effort) = payload.reasoning_effort.as_deref() {
        context.reasoning_effort = safe_label(reasoning_effort);
    }
}

/// 只把选择器确认过的新调用推进为累计/last 基线；估算与重放保持旧基线。
pub(super) fn update_incremental_baseline(
    context: &mut JsonlParseContext,
    last: Option<TokenUsage>,
    cumulative: Option<TokenUsage>,
) {
    if let Some(last) = last {
        context.previous_last = Some(last);
    }
    if let Some(cumulative) = cumulative {
        context.previous_cumulative = Some(cumulative);
    }
}

/// 接受遗留 `thread_name_updated` 中的短标题；不读取 goal.objective。
pub(super) fn update_thread_title(context: &mut JsonlParseContext, payload: &RolloutPayload) {
    if let Some(title) = payload
        .thread_name
        .as_deref()
        .or(payload.name.as_deref())
        .and_then(safe_thread_title)
    {
        context.thread_label = Some(title);
    }
}

/// 仅允许短小技术标签进入索引，拒绝路径分隔符、正文空白与异常长字符串。
fn safe_label(value: &str) -> Option<String> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    {
        return None;
    }
    Some(value.to_owned())
}
