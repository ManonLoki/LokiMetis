//! 可复用的展示标签语义码。
//!
//! 该模块抽离自 GUI 侧 DTO，表示“标签文本类型”本身而非语言文案。
//! CLI、GUI、未来 MCP 共享该码值可避免重复定义。

/// 描述当前标签文本的稳定语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DisplayLabelCode {
    /// 标签已按安全策略保留为字面值。
    Literal,
    /// 模型不可用。
    UnknownModel,
    /// 推理强度不可用。
    UnknownReasoningEffort,
    /// 设备或会话明确标记为“无”。
    ReasoningNone,
    /// 推理强度：最低。
    ReasoningMinimal,
    /// 推理强度：低。
    ReasoningLow,
    /// 推理强度：中。
    ReasoningMedium,
    /// 推理强度：高。
    ReasoningHigh,
    /// 推理强度：很高。
    ReasoningXHigh,
    /// 项目不可用。
    UncategorizedProject,
    /// 项目键可展示。
    Project,
    /// 线程不可用。
    UnknownThread,
    /// 线程键可展示。
    Thread,
    /// 数据根不可用。
    UnnamedRoot,
    /// 统计其余项。
    Remainder,
}
