//! AgentHooks 的工具目录、写入结果与配置定位类型。

use serde::{Deserialize, Serialize};

/// LokiMetis 支持接入的全部 AI 工具。
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum AiTool {
    /// Codex CLI。
    Codex,
    /// Claude Code CLI。
    ClaudeCode,
    /// Cursor 编辑器。
    Cursor,
    /// OpenCode。
    OpenCode,
    /// WorkBuddy。
    WorkBuddy,
    /// Hermes。
    Hermes,
    /// OpenClaw。
    OpenClaw,
    /// CodeBuddy。
    CodeBuddy,
    /// Qwen Code。
    QwenCode,
    /// Kimi Code。
    KimiCode,
    /// Qoder。
    Qoder,
    /// Gemini CLI。
    GeminiCli,
    /// GitHub Copilot CLI。
    GitHubCopilot,
    /// Grok Build CLI。
    Grok,
}

impl AiTool {
    /// 固定顺序的完整目录。
    pub const ALL: [Self; 14] = [
        Self::Codex,
        Self::ClaudeCode,
        Self::Cursor,
        Self::OpenCode,
        Self::WorkBuddy,
        Self::Hermes,
        Self::OpenClaw,
        Self::CodeBuddy,
        Self::QwenCode,
        Self::KimiCode,
        Self::Qoder,
        Self::GeminiCli,
        Self::GitHubCopilot,
        Self::Grok,
    ];
}

/// 前端展示 AI 工具选择项所需的稳定目录条目。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiToolDescriptor {
    /// 对应的工具枚举值。
    pub tool: AiTool,
    /// 供前端直接展示的工具名称。
    pub name: String,
}

/// Hooks 配置发生变化后，用户在对应工具中还需完成的唯一激活流程。
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HookWriteOutcome {
    /// 配置未变化。
    Unchanged,
    /// 已写入且立即生效。
    Active,
    /// 需要重启或新建会话。
    RestartRequired,
    /// Codex 需要审核。
    CodexReviewRequired,
    /// WorkBuddy 需要审核。
    WorkBuddyReviewRequired,
    /// CodeBuddy 需要审核。
    CodeBuddyReviewRequired,
    /// Hermes 需要手动启用插件。
    HermesEnableRequired,
    /// OpenClaw 需要手动启用插件。
    OpenClawEnableRequired,
}

impl HookWriteOutcome {
    /// 配置是否实际变化。
    pub const fn config_changed(self) -> bool {
        !matches!(self, Self::Unchanged)
    }

    /// 是否需要用户审核。
    pub const fn requires_review(self) -> bool {
        matches!(
            self,
            Self::CodexReviewRequired
                | Self::WorkBuddyReviewRequired
                | Self::CodeBuddyReviewRequired
                | Self::HermesEnableRequired
                | Self::OpenClawEnableRequired
        )
    }

    /// 是否需要重启或等价流程。
    pub const fn restart_required(self) -> bool {
        matches!(
            self,
            Self::RestartRequired
                | Self::CodexReviewRequired
                | Self::WorkBuddyReviewRequired
                | Self::CodeBuddyReviewRequired
                | Self::HermesEnableRequired
                | Self::OpenClawEnableRequired
        )
    }
}

/// 一次写入 Hooks 配置后的结果。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookConfigWriteResult {
    /// 目标工具。
    pub tool: AiTool,
    /// 写入文件名。
    pub filename: String,
    /// 精确结果码。
    pub outcome: HookWriteOutcome,
    /// 由 outcome 派生。
    pub config_changed: bool,
    /// 由 outcome 派生。
    pub requires_review: bool,
    /// 由 outcome 派生。
    pub restart_required: bool,
}

impl HookConfigWriteResult {
    /// 未变化时折叠为 Unchanged。
    pub fn from_changed_outcome(
        tool: AiTool,
        filename: String,
        config_changed: bool,
        changed_outcome: HookWriteOutcome,
    ) -> Self {
        debug_assert_ne!(changed_outcome, HookWriteOutcome::Unchanged);
        let outcome = if config_changed {
            changed_outcome
        } else {
            HookWriteOutcome::Unchanged
        };
        Self {
            tool,
            filename,
            outcome,
            config_changed: outcome.config_changed(),
            requires_review: outcome.requires_review(),
            restart_required: outcome.restart_required(),
        }
    }
}

/// 生成的 Hooks 配置文件预览。
#[derive(Clone, Debug)]
pub struct HookConfigPreview {
    /// 目标配置文件名。
    pub filename: String,
    /// 完整文件内容。
    pub content: String,
}

/// 各工具自定义 Hooks 配置目录；空字符串表示使用默认目录。
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookConfigDirectories {
    /// Codex 自定义目录。
    #[serde(default)]
    pub codex: String,
    /// Claude Code 自定义目录。
    #[serde(default)]
    pub claude_code: String,
    /// Cursor 自定义目录。
    #[serde(default)]
    pub cursor: String,
    /// OpenCode 自定义目录。
    #[serde(default)]
    pub open_code: String,
    /// WorkBuddy 自定义目录。
    #[serde(default)]
    pub work_buddy: String,
    /// Hermes 自定义目录。
    #[serde(default)]
    pub hermes: String,
    /// OpenClaw 自定义目录。
    #[serde(default)]
    pub open_claw: String,
    /// CodeBuddy 自定义目录。
    #[serde(default)]
    pub code_buddy: String,
    /// Qwen Code 自定义目录。
    #[serde(default)]
    pub qwen_code: String,
    /// Kimi Code 自定义目录。
    #[serde(default)]
    pub kimi_code: String,
    /// Qoder 自定义目录。
    #[serde(default)]
    pub qoder: String,
    /// Gemini CLI 自定义目录。
    #[serde(default)]
    pub gemini_cli: String,
    /// GitHub Copilot CLI 自定义目录。
    #[serde(default)]
    pub github_copilot: String,
    /// Grok 自定义目录。
    #[serde(default)]
    pub grok: String,
}

impl HookConfigDirectories {
    /// 取出对应工具的自定义目录。
    pub fn get(&self, tool: AiTool) -> &str {
        match tool {
            AiTool::Codex => &self.codex,
            AiTool::ClaudeCode => &self.claude_code,
            AiTool::Cursor => &self.cursor,
            AiTool::OpenCode => &self.open_code,
            AiTool::WorkBuddy => &self.work_buddy,
            AiTool::Hermes => &self.hermes,
            AiTool::OpenClaw => &self.open_claw,
            AiTool::CodeBuddy => &self.code_buddy,
            AiTool::QwenCode => &self.qwen_code,
            AiTool::KimiCode => &self.kimi_code,
            AiTool::Qoder => &self.qoder,
            AiTool::GeminiCli => &self.gemini_cli,
            AiTool::GitHubCopilot => &self.github_copilot,
            AiTool::Grok => &self.grok,
        }
    }

    /// 写入对应工具的自定义目录。
    pub fn set(&mut self, tool: AiTool, directory: String) {
        match tool {
            AiTool::Codex => self.codex = directory,
            AiTool::ClaudeCode => self.claude_code = directory,
            AiTool::Cursor => self.cursor = directory,
            AiTool::OpenCode => self.open_code = directory,
            AiTool::WorkBuddy => self.work_buddy = directory,
            AiTool::Hermes => self.hermes = directory,
            AiTool::OpenClaw => self.open_claw = directory,
            AiTool::CodeBuddy => self.code_buddy = directory,
            AiTool::QwenCode => self.qwen_code = directory,
            AiTool::KimiCode => self.kimi_code = directory,
            AiTool::Qoder => self.qoder = directory,
            AiTool::GeminiCli => self.gemini_cli = directory,
            AiTool::GitHubCopilot => self.github_copilot = directory,
            AiTool::Grok => self.grok = directory,
        }
    }
}

/// 某工具 Hooks 配置文件的最终定位。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookConfigLocation {
    /// 所属工具。
    pub tool: AiTool,
    /// 配置目录。
    pub directory: String,
    /// 配置文件完整路径。
    pub config_path: String,
    /// 是否为用户自定义目录。
    pub is_custom: bool,
}

/// AI 实例在展示面上的四态行为。
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum HookBehavior {
    /// 空闲。
    Idle,
    /// 运行中。
    Running,
    /// 询问中。
    Asking,
    /// 出错。
    Error,
}

impl HookBehavior {
    /// 固定展示顺序。
    pub const DISPLAY_BEHAVIORS: [Self; 4] = [Self::Idle, Self::Running, Self::Asking, Self::Error];
}

/// 状态机迁移动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookTransition {
    /// 展示指定行为。
    Display(HookBehavior),
    /// 释放展示位。
    Release,
}

/// 按统一公开目录的监控映射顺序规范化已选工具，隐藏项与重复项均被丢弃。
pub fn normalize_enabled_ai_tools(selected: &[AiTool]) -> Vec<AiTool> {
    crate::public_monitor_ai_tools()
        .filter(|tool| selected.contains(tool))
        .collect()
}

/// Hook relay 活跃实例 rendezvous 文件名。
pub const HOOK_RELAY_RENDEZVOUS_FILENAME: &str = "loki-metis-hook-relay.json";

/// Hook relay rendezvous JSON 协议版本。
pub const HOOK_RELAY_RENDEZVOUS_SCHEMA_VERSION: u8 = 1;

/// 请求和响应共同携带的 relay 实例校验头。
pub const HOOK_RELAY_INSTANCE_HEADER: &str = "X-LokiMetis-Hook-Instance";

/// 请求携带的规范 Hook 事件名头。
pub const HOOK_EVENT_TYPE_HEADER: &str = "X-LokiMetis-Hook-Type";

/// 请求操作系统在回环上分配空闲端口。
pub const HOOK_RELAY_EPHEMERAL_PORT: u16 = 0;

/// 本机 Hook 中继的回环地址。
pub fn hook_relay_loopback_address(port: u16) -> String {
    format!("127.0.0.1:{port}")
}

/// 原生 Hook stdin 最大字节数。
pub const MAX_NATIVE_HOOK_INPUT_BYTES: usize = 4 * 1024 * 1024;

#[cfg(test)]
mod tests {
    use super::{
        AiTool, HOOK_RELAY_EPHEMERAL_PORT, HookConfigDirectories, hook_relay_loopback_address,
        normalize_enabled_ai_tools,
    };

    #[test]
    fn loopback_address_uses_the_given_port() {
        assert_eq!(hook_relay_loopback_address(23_456), "127.0.0.1:23456");
        assert_eq!(
            hook_relay_loopback_address(HOOK_RELAY_EPHEMERAL_PORT),
            "127.0.0.1:0"
        );
    }

    #[test]
    fn all_ai_tool_names_match_the_camel_case_contract() {
        let expected = [
            "codex",
            "claudeCode",
            "cursor",
            "openCode",
            "workBuddy",
            "hermes",
            "openClaw",
            "codeBuddy",
            "qwenCode",
            "kimiCode",
            "qoder",
            "geminiCli",
            "gitHubCopilot",
            "grok",
        ];
        for (tool, expected_name) in AiTool::ALL.into_iter().zip(expected) {
            let encoded = serde_json::to_string(&tool).unwrap();
            assert_eq!(encoded, format!("\"{expected_name}\""));
            assert_eq!(serde_json::from_str::<AiTool>(&encoded).unwrap(), tool);
        }
    }

    #[test]
    fn hook_directories_cover_every_tool_and_serialize_camel_case() {
        let mut directories = HookConfigDirectories::default();
        for (index, tool) in AiTool::ALL.into_iter().enumerate() {
            let value = format!("/hooks/{index}");
            directories.set(tool, value.clone());
            assert_eq!(directories.get(tool), value);
        }
        let serialized = serde_json::to_value(directories).unwrap();
        for key in [
            "codex",
            "claudeCode",
            "cursor",
            "openCode",
            "workBuddy",
            "hermes",
            "openClaw",
            "codeBuddy",
            "qwenCode",
            "kimiCode",
            "qoder",
            "geminiCli",
            "githubCopilot",
            "grok",
        ] {
            assert!(serialized.get(key).is_some(), "missing {key}");
        }
    }

    #[test]
    fn enabled_tools_are_normalized_in_the_public_monitor_order() {
        assert_eq!(
            normalize_enabled_ai_tools(&[
                AiTool::Grok,
                AiTool::Cursor,
                AiTool::Codex,
                AiTool::Cursor,
                AiTool::OpenClaw,
            ]),
            vec![AiTool::Codex, AiTool::Cursor, AiTool::Grok]
        );
    }
}
