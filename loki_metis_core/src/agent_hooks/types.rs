//! AgentHooks 的工具目录、写入结果与配置定位类型。

use serde::{Deserialize, Serialize};

/// 本产品监控区支持写入 Hooks 的四项 Agent。
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum AiTool {
    /// Codex CLI。
    Codex,
    /// Claude Code CLI。
    ClaudeCode,
    /// Grok Build CLI。
    Grok,
    /// WorkBuddy。
    WorkBuddy,
}

impl AiTool {
    /// 固定顺序的完整目录。
    pub const ALL: [Self; 4] = [Self::Codex, Self::ClaudeCode, Self::Grok, Self::WorkBuddy];
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
            Self::CodexReviewRequired | Self::WorkBuddyReviewRequired
        )
    }

    /// 是否需要重启或等价流程。
    pub const fn restart_required(self) -> bool {
        matches!(
            self,
            Self::RestartRequired | Self::CodexReviewRequired | Self::WorkBuddyReviewRequired
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
    /// Grok 自定义目录。
    #[serde(default)]
    pub grok: String,
    /// WorkBuddy 自定义目录。
    #[serde(default)]
    pub work_buddy: String,
}

impl HookConfigDirectories {
    /// 取出对应工具的自定义目录。
    pub fn get(&self, tool: AiTool) -> &str {
        match tool {
            AiTool::Codex => &self.codex,
            AiTool::ClaudeCode => &self.claude_code,
            AiTool::Grok => &self.grok,
            AiTool::WorkBuddy => &self.work_buddy,
        }
    }

    /// 写入对应工具的自定义目录。
    pub fn set(&mut self, tool: AiTool, directory: String) {
        match tool {
            AiTool::Codex => self.codex = directory,
            AiTool::ClaudeCode => self.claude_code = directory,
            AiTool::Grok => self.grok = directory,
            AiTool::WorkBuddy => self.work_buddy = directory,
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
    pub const DISPLAY_BEHAVIORS: [Self; 4] =
        [Self::Idle, Self::Running, Self::Asking, Self::Error];
}

/// 状态机迁移动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookTransition {
    /// 展示指定行为。
    Display(HookBehavior),
    /// 释放展示位。
    Release,
}

/// 按固定顺序规范化已选工具并去重。
pub fn normalize_enabled_ai_tools(selected: &[AiTool]) -> Vec<AiTool> {
    AiTool::ALL
        .into_iter()
        .filter(|tool| selected.contains(tool))
        .collect()
}

/// 本机 Hook 中继监听端口。
pub const DEFAULT_HOOK_RELAY_PORT: u16 = 10_240;

/// 原生 Hook stdin 最大字节数。
pub const MAX_NATIVE_HOOK_INPUT_BYTES: usize = 4 * 1024 * 1024;
