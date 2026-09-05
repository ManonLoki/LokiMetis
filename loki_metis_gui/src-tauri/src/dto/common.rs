//! 跨领域共用的稳定语义代码：可见消息、展示标签与索引位置。

use serde::{Deserialize, Serialize};

/// 标识用户选择的界面语言偏好；系统偏好由前端按当前系统语言解析。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LanguagePreferenceDto {
    /// 跟随当前系统语言；中文系统使用简体中文，其他系统使用英文。
    #[default]
    #[serde(rename = "system")]
    System,
    /// 固定使用简体中文。
    #[serde(rename = "zh-CN")]
    ZhCn,
    /// 固定使用美式英文。
    #[serde(rename = "en-US")]
    EnUs,
}

/// 标识前端必须按当前 locale 渲染的固定可见消息。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UiMessageCodeDto {
    /// 本机索引读取失败，当前本机概览需要重试。
    OverviewLocalIndexUnavailable,
    /// WorkBuddy 当次读取失败，「全部」仅保留其他 Agent 且标记为部分覆盖。
    OverviewWorkbuddyUnavailable,
    /// 尚未启动扫描。
    ScanIdle,
    /// 正在扫描授权范围。
    ScanRunning,
    /// 正在请求取消扫描。
    ScanCancelling,
    /// 扫描已取消。
    ScanCancelled,
    /// 扫描已完成。
    ScanCompleted,
    /// 扫描失败。
    ScanFailed,
    /// 用户取消了原生目录选择。
    SourceAddCancelled,
    /// 数据根已经登记。
    SourceRegistered,
    /// 所选目录已经存在于当前客户端的数据源中。
    SourceAlreadyRegistered,
    /// 手动添加已启动所选子树深搜。
    SourceManualDeepSearchStarted,
    /// 手动子树深搜未发现合格根。
    SourceManualDeepSearchEmpty,
    /// 数据根已经启用。
    SourceEnabled,
    /// 数据根已经停用。
    SourceDisabled,
    /// 数据根别名已经更新。
    SourceRenamed,
    /// 数据根已经从本产品索引移除。
    SourceRemoved,
    /// Codex 主数据目录已经切换。
    PrimaryChanged,
    /// 所选目录已经是 Codex 主数据目录。
    PrimaryAlreadySelected,
    /// Codex 主数据目录选择已经清除。
    PrimaryCleared,
    /// 当前没有 Codex 主数据目录选择。
    PrimaryNotSet,
    /// 本产品索引已经清空。
    IndexCleared,
}

/// 标识前端不得直接展示 backend 中文占位的固定标签语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DisplayLabelCodeDto {
    /// `label` 是用户或安全技术字面值，保持原样。
    Literal,
    /// 模型不可用。
    UnknownModel,
    /// 推理强度不可用。
    UnknownReasoningEffort,
    /// 明确没有推理强度。
    ReasoningNone,
    /// 最低推理强度。
    ReasoningMinimal,
    /// 低推理强度。
    ReasoningLow,
    /// 中推理强度。
    ReasoningMedium,
    /// 高推理强度。
    ReasoningHigh,
    /// 很高推理强度。
    ReasoningXHigh,
    /// 项目键不可用。
    UncategorizedProject,
    /// `label` 是项目匿名短标识。
    Project,
    /// 线程键不可用。
    UnknownThread,
    /// `label` 是线程匿名短标识。
    Thread,
    /// 数据根别名不可用。
    UnnamedRoot,
    /// Top-N 之外合并的其余项。
    Remainder,
}

impl From<loki_metis_core::DisplayLabelCode> for DisplayLabelCodeDto {
    /// 把 core 的展示语义代码映射为 IPC 边界的稳定代码。
    fn from(code: loki_metis_core::DisplayLabelCode) -> Self {
        use loki_metis_core::DisplayLabelCode as Core;
        match code {
            Core::Literal => Self::Literal,
            Core::UnknownModel => Self::UnknownModel,
            Core::UnknownReasoningEffort => Self::UnknownReasoningEffort,
            Core::ReasoningNone => Self::ReasoningNone,
            Core::ReasoningMinimal => Self::ReasoningMinimal,
            Core::ReasoningLow => Self::ReasoningLow,
            Core::ReasoningMedium => Self::ReasoningMedium,
            Core::ReasoningHigh => Self::ReasoningHigh,
            Core::ReasoningXHigh => Self::ReasoningXHigh,
            Core::UncategorizedProject => Self::UncategorizedProject,
            Core::Project => Self::Project,
            Core::UnknownThread => Self::UnknownThread,
            Core::Thread => Self::Thread,
            Core::UnnamedRoot => Self::UnnamedRoot,
            Core::Remainder => Self::Remainder,
        }
    }
}

/// 标识当前客户端索引在本产品 app-data 中的安全相对位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IndexLocationCodeDto {
    /// Codex 索引位置。
    Codex,
    /// Claude Code 索引位置。
    ClaudeCode,
    /// Grok Build CLI 索引位置。
    GrokBuildCli,
}

#[cfg(test)]
mod tests {
    use super::{DisplayLabelCodeDto, LanguagePreferenceDto, UiMessageCodeDto};

    /// 验证语言偏好只接受三个批准的稳定 wire 值。
    #[test]
    fn language_preference_uses_stable_wire_values() {
        assert_eq!(
            serde_json::from_str::<LanguagePreferenceDto>("\"system\"").unwrap(),
            LanguagePreferenceDto::System
        );
        assert_eq!(
            serde_json::from_str::<LanguagePreferenceDto>("\"zh-CN\"").unwrap(),
            LanguagePreferenceDto::ZhCn
        );
        assert_eq!(
            serde_json::from_str::<LanguagePreferenceDto>("\"en-US\"").unwrap(),
            LanguagePreferenceDto::EnUs
        );
        assert_eq!(
            serde_json::to_string(&LanguagePreferenceDto::System).unwrap(),
            "\"system\""
        );
        assert!(serde_json::from_str::<LanguagePreferenceDto>("\"zh\"").is_err());
    }

    /// 验证前端本地化依赖的可见消息与展示标签代码使用稳定 camelCase wire 契约。
    #[test]
    fn localization_codes_use_stable_wire_shapes() {
        assert_eq!(
            serde_json::to_value(UiMessageCodeDto::PrimaryAlreadySelected).unwrap(),
            serde_json::json!("primaryAlreadySelected")
        );
        assert_eq!(
            serde_json::to_value(UiMessageCodeDto::OverviewWorkbuddyUnavailable).unwrap(),
            serde_json::json!("overviewWorkbuddyUnavailable")
        );
        assert_eq!(
            serde_json::to_value(DisplayLabelCodeDto::UnknownReasoningEffort).unwrap(),
            serde_json::json!("unknownReasoningEffort")
        );
    }
}
