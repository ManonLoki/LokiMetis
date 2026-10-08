//! Kimi Code TOML command Hook 协议。

use std::fmt::Write;

use serde::Serialize;
use serde_json::{Map, Value, json};

use super::{
    AiTool, HookBehavior, HookConfigPreview, HookError, HookEvent, HookEventKind, HookProtocol,
    HookWriteOutcome, ManagedCommands, managed_hook_marker,
};

/// Kimi Code 协议单例。
pub(super) static KIMI_CODE: KimiCodeProtocol = KimiCodeProtocol;

/// Kimi Code 无状态协议实现。
pub(super) struct KimiCodeProtocol;

/// Kimi 配置中由 LokiMetis 管理的 Hook 集合。
#[derive(Serialize)]
struct KimiConfig {
    /// 全部托管 Hook 条目。
    hooks: Vec<KimiHook>,
}

/// 单条 Kimi TOML Hook 记录。
#[derive(Serialize)]
struct KimiHook {
    /// 事件名。
    event: String,
    /// 可选 matcher。
    #[serde(skip_serializing_if = "Option::is_none")]
    matcher: Option<String>,
    /// Hook 命令。
    command: String,
}

/// Kimi Code 会稳定推进四态展示的公开事件。
const EVENTS: &[HookEvent] = &[
    HookEvent::new("SessionStart", HookEventKind::SessionStart),
    HookEvent::new("UserPromptSubmit", HookEventKind::WorkStart),
    HookEvent::new(
        "PreToolUse",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "PostToolUse",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "PostToolUseFailure",
        HookEventKind::State(HookBehavior::Error),
    ),
    HookEvent::new(
        "PermissionRequest",
        HookEventKind::State(HookBehavior::Asking),
    ),
    HookEvent::new("Stop", HookEventKind::Stop),
    HookEvent::new("StopFailure", HookEventKind::State(HookBehavior::Error)),
    HookEvent::new("Interrupt", HookEventKind::Stop),
    HookEvent::new(
        "SubagentStart",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "SubagentStop",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "PreCompact",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "PostCompact",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new("SessionEnd", HookEventKind::SessionEnd),
];

impl HookProtocol for KimiCodeProtocol {
    /// 返回 Kimi Code 工具类型。
    fn tool(&self) -> AiTool {
        AiTool::KimiCode
    }

    /// 返回展示名称。
    fn name(&self) -> &'static str {
        "Kimi Code"
    }

    /// 返回中继 slug。
    fn slug(&self) -> &'static str {
        "kimi-code"
    }

    /// 返回共享用户配置文件名。
    fn config_filename(&self) -> &'static str {
        "config.toml"
    }

    /// 返回预览路径。
    fn preview_filename(&self) -> &'static str {
        ".kimi-code/config.toml"
    }

    /// 返回完整事件表。
    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    /// Kimi 各平台均经 Git Bash 执行 POSIX 命令。
    fn handler(&self, event: &HookEvent, commands: &ManagedCommands) -> Value {
        let mut handler = json!({ "command": commands.posix });
        if let Some(matcher) = event.matcher {
            handler["matcher"] = Value::String(matcher.to_owned());
        }
        handler
    }

    /// 把公共事件表转换为带受管边界的 Kimi TOML 区块。
    fn render_config(&self, hooks: Map<String, Value>) -> Result<String, HookError> {
        let marker = managed_hook_marker(self.tool());
        let mut serialized_hooks = Vec::with_capacity(self.events().len());
        for event in self.events() {
            let handler = hooks
                .get(event.name)
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    HookError::new("error.hooks.kimiMissingHandler").param("event", event.name)
                })?;
            let command = handler
                .get("command")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    HookError::new("error.hooks.kimiMissingCommand").param("event", event.name)
                })?;
            serialized_hooks.push(KimiHook {
                event: event.name.to_owned(),
                matcher: handler
                    .get("matcher")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                command: command.to_owned(),
            });
        }
        let serialized = toml::to_string(&KimiConfig {
            hooks: serialized_hooks,
        })
        .map_err(|error| {
            HookError::new("error.hooks.kimiSerializeFailed").param("detail", error.to_string())
        })?;
        let mut content = format!("# {marker} begin\n{serialized}");
        if !content.ends_with('\n') {
            content.push('\n');
        }
        writeln!(content, "# {marker} end").expect("writing to String cannot fail");
        Ok(content)
    }

    /// Kimi 与用户配置共享 TOML，必须使用边界感知的自定义合并器。
    fn uses_custom_merge(&self) -> bool {
        true
    }

    /// 只替换 LokiMetis 受管区块，用户其余 TOML 内容逐字保留。
    fn merge_standalone(
        &self,
        existing_content: Option<&str>,
        generated: &HookConfigPreview,
    ) -> Result<String, HookError> {
        let mut merged = remove_managed_block(existing_content.unwrap_or(""), self.tool())?;
        if !merged.is_empty() {
            if !merged.ends_with('\n') {
                merged.push('\n');
            }
            if !merged.ends_with("\n\n") {
                merged.push('\n');
            }
        }
        merged.push_str(&generated.content);
        Ok(merged)
    }

    /// Kimi Code 在新会话启动时读取配置。
    fn changed_write_outcome(&self) -> HookWriteOutcome {
        HookWriteOutcome::RestartRequired
    }
}

/// 从配置中移除唯一且边界完整的 LokiMetis 受管区块。
fn remove_managed_block(content: &str, tool: AiTool) -> Result<String, HookError> {
    let marker = managed_hook_marker(tool);
    let begin = format!("# {marker} begin");
    let end = format!("# {marker} end");
    let begins = marker_line_ranges(content, &begin);
    let ends = marker_line_ranges(content, &end);
    if begins.is_empty() && ends.is_empty() {
        return Ok(content.to_owned());
    }
    if begins.len() != 1 || ends.len() != 1 || begins[0].0 >= ends[0].0 {
        return Err(HookError::new("error.hooks.kimiManagedBlockCorrupted"));
    }
    let mut stripped = String::with_capacity(content.len());
    stripped.push_str(&content[..begins[0].0]);
    stripped.push_str(&content[ends[0].1..]);
    Ok(stripped)
}

/// 返回与 marker 完全相等的各行字节范围，范围包含换行符。
fn marker_line_ranges(content: &str, marker: &str) -> Vec<(usize, usize)> {
    let mut offset = 0;
    let mut ranges = Vec::new();
    for line in content.split_inclusive('\n') {
        let end = offset + line.len();
        if line.trim_end_matches(['\r', '\n']) == marker {
            ranges.push((offset, end));
        }
        offset = end;
    }
    ranges
}
