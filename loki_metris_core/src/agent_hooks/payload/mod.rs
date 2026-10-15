//! 把各 Agent 原生 stdin 归约为 listener 最小信封。

use encoding_rs::{Encoding, UTF_8};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{AiTool, HookError, MAX_NATIVE_HOOK_INPUT_BYTES};

/// 本机 Hook 接口的唯一正文契约。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MinimalHookPayload {
    /// 配置登记的事件名。
    pub hook_event_name: String,
    /// 可选会话标识。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// 可选轮次标识。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    /// 执行该交互的子 Agent；不会拿它替代父会话或轮次 ID。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// 单次工具/澄清交互标识，用于保护其它并行的用户等待。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interaction_id: Option<String>,
    /// 工具公开名称；不包含工具参数或回答。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// 可选状态标量。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// relay 对一份原生 stdin 的领域决策。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreparedNativeHook {
    /// 投递给本机 listener。
    Deliver(MinimalHookPayload),
    /// 因跨宿主调用丢弃。
    SuppressForeignHost,
}

/// 解析原生 stdin 一次再决定投递或抑制。
pub fn prepare_native_hook(
    tool: AiTool,
    native_json: &[u8],
    configured_event: &str,
) -> Result<PreparedNativeHook, HookError> {
    let source = parse_native_hook_object(native_json)?;
    if tool != AiTool::Cursor && is_cursor_hosted(&source) {
        return Ok(PreparedNativeHook::SuppressForeignHost);
    }
    envelope_from_source(tool, &source, configured_event).map(PreparedNativeHook::Deliver)
}

/// 解析原生 JSON 对象。
fn parse_native_hook_object(native_json: &[u8]) -> Result<Map<String, Value>, HookError> {
    if native_json.is_empty() || native_json.len() > MAX_NATIVE_HOOK_INPUT_BYTES {
        return Err(HookError::new("error.payload.rawInputInvalid"));
    }
    let decoded = decode_native_json(native_json)?;
    let source = serde_json::from_str::<Value>(&decoded).map_err(|error| {
        HookError::new("error.payload.rawJsonInvalid").param("detail", error.to_string())
    })?;
    match source {
        Value::Object(source) => Ok(source),
        _ => Err(HookError::new("error.payload.rawJsonRootNotObject")),
    }
}

/// 从原生对象提取最小信封。
fn envelope_from_source(
    tool: AiTool,
    source: &Map<String, Value>,
    configured_event: &str,
) -> Result<MinimalHookPayload, HookError> {
    let body_event = string_field(source, &["hook_event_name", "hookEventName", "type"]);
    if body_event
        .as_deref()
        .is_some_and(|event| !event_names_match(event, configured_event))
    {
        return Err(HookError::new("error.payload.eventMismatch"));
    }
    Ok(MinimalHookPayload {
        hook_event_name: configured_event.to_owned(),
        session_id: string_field(
            source,
            &[
                "session_id",
                "sessionId",
                "conversation_id",
                "conversationId",
            ],
        ),
        turn_id: string_field(
            source,
            &[
                "turn_id",
                "turnId",
                "generation_id",
                "generationId",
                "prompt_id",
                "promptId",
            ],
        ),
        agent_id: string_field(source, &["agent_id", "agentId"]),
        interaction_id: string_field(
            source,
            &[
                "interaction_id",
                "tool_use_id",
                "toolUseId",
                "tool_call_id",
                "toolCallId",
                "elicitation_id",
            ],
        ),
        tool_name: string_field(source, &["tool_name", "toolName"]).or_else(|| {
            // 无调用ID的MCP表单至少按服务器隔离；该名称不冒充工具调用ID。
            matches!(configured_event, "Elicitation" | "ElicitationResult")
                .then(|| {
                    string_field(source, &["mcp_server_name"])
                        .map(|name| format!("mcp-server:{name}"))
                })
                .flatten()
        }),
        status: event_status(tool, source, configured_event),
    })
}

/// 只传递可证实的有限语义；错误正文、工具参数、问题和回答均留在原生输入边界。
fn event_status(tool: AiTool, source: &Map<String, Value>, event: &str) -> Option<String> {
    if event.eq_ignore_ascii_case("Notification") {
        return string_field(source, &["notification_type", "notificationType"]);
    }
    if event == "FinalStop" && matches!(tool, AiTool::WorkBuddy | AiTool::CodeBuddy) {
        return string_field(source, &["final_stop_reason"]);
    }
    if tool == AiTool::Cursor && event == "sessionEnd" {
        return Some(
            match string_field(source, &["final_status", "reason", "status"]).as_deref() {
                Some("completed") => "completed",
                Some("error") => "error",
                Some("aborted") => "aborted",
                _ => "closed",
            }
            .into(),
        );
    }
    if matches!(event, "PostToolUseFailure" | "postToolUseFailure")
        && source.get("is_interrupt") == Some(&Value::Bool(true))
    {
        return Some("cancelled".into());
    }
    if event == "PreToolUse" {
        let name = string_field(source, &["tool_name", "toolName"]);
        let asks = match tool {
            AiTool::ClaudeCode => {
                matches!(name.as_deref(), Some("AskUserQuestion" | "ExitPlanMode"))
            }
            AiTool::WorkBuddy | AiTool::CodeBuddy => matches!(
                name.as_deref(),
                Some("AskUserQuestion" | "ExitPlanMode" | "AskUserForStructuredInput")
            ),
            AiTool::Grok => name.as_deref() == Some("ask_user_question"),
            _ => false,
        };
        if asks {
            return Some("awaiting_user".into());
        }
    }
    if tool == AiTool::GeminiCli
        && event == "AfterTool"
        && source
            .get("tool_response")
            .and_then(|value| value.get("error"))
            .is_some_and(|value| match value {
                Value::String(text) => !text.is_empty(),
                Value::Object(_) => true,
                Value::Bool(value) => *value,
                _ => false,
            })
    {
        return Some("tool_error".into());
    }
    scalar_field(source, "status")
}

/// Cursor 宿主字段；非 Cursor adapter 遇到非空 cursor_version 时抑制。
fn is_cursor_hosted(source: &Map<String, Value>) -> bool {
    string_field(source, &["cursor_version"]).is_some()
}

/// 忽略大小写与分隔符比较事件名。
fn event_names_match(left: &str, right: &str) -> bool {
    /// 移除分隔符并转为小写，得到用于协议比较的事件名。
    fn normalize(value: &str) -> String {
        value
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .flat_map(char::to_lowercase)
            .collect()
    }
    normalize(left) == normalize(right)
}

/// 严格解码 UTF-8，或由 BOM 明确声明的 UTF-16LE/BE。
fn decode_native_json(native_json: &[u8]) -> Result<String, HookError> {
    let (encoding, bom_len) = Encoding::for_bom(native_json).unwrap_or((UTF_8, 0));
    encoding
        .decode_without_bom_handling_and_without_replacement(&native_json[bom_len..])
        .map(std::borrow::Cow::into_owned)
        .ok_or_else(|| {
            if encoding == UTF_8 {
                HookError::new("error.payload.invalidUtf8")
            } else {
                HookError::new("error.payload.invalidUtf16")
            }
        })
}

/// 按候选字段名取第一个非空字符串。
fn string_field(source: &Map<String, Value>, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        source
            .get(*name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

/// 读取标量字段并转成字符串。
fn scalar_field(source: &Map<String, Value>, name: &str) -> Option<String> {
    match source.get(name) {
        Some(Value::String(value)) => Some(value.clone()),
        Some(Value::Bool(value)) => Some(value.to_string()),
        Some(Value::Number(value)) => Some(value.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
