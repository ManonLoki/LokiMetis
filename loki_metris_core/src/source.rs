//! 定义本机来源覆盖、调用 provenance 与聚合输出。
//! 本文件既承载本机来源数据模型，也承载本机聚合域内可复用业务函数。

use serde::{Deserialize, Serialize};

use crate::{
    Completeness, Confidence, Freshness, ProviderKind, TokenUsage, coverage_completeness,
    empty_token_usage_for_provider,
};

/// 按 provider 选择可复用的本机聚合空值模板。
pub fn empty_local_usage_aggregate_for_provider(provider: ProviderKind) -> LocalUsageAggregate {
    let empty_tokens = empty_token_usage_for_provider(provider);
    LocalUsageAggregate {
        cached_read_call_count: empty_tokens.cached_input_tokens.map(|_| 0),
        tokens: empty_tokens,
        call_count: 0,
        thread_count: 0,
        root_count: 0,
        source_count: 0,
        duplicate_source_count: 0,
        cross_root_duplicate_source_count: 0,
        cache_read_basis_points: None,
        confidence: Confidence::Exact,
    }
}

/// 评估本机索引窗口在扫描状态、覆盖状态与聚合置信度下的事实质量。
pub fn local_fact_quality(
    index_state: LocalIndexState,
    coverage_state: CoverageState,
    aggregate_confidence: Confidence,
) -> (Freshness, Completeness, Confidence) {
    match index_state {
        LocalIndexState::NotScanned | LocalIndexState::NeedsRescan => (
            Freshness::Unknown,
            Completeness::Unknown,
            Confidence::Derived,
        ),
        LocalIndexState::ReadyNoCalls | LocalIndexState::Ready => (
            Freshness::Fresh,
            coverage_completeness(coverage_state),
            aggregate_confidence,
        ),
    }
}

/// 只有旧 parser generation 导致的明确重建状态才需要在启动后立即回补。
pub const fn immediate_reindex_required(index_state: LocalIndexState) -> bool {
    matches!(index_state, LocalIndexState::NeedsRescan)
}

/// 描述一次发现或扫描对用户授权范围的覆盖结论。
// Hash：允许把该类型放进 HashMap/HashSet 的 key 或做去重集合的元素；
// 纯数据的 enum/struct 常见搭配 Debug+Clone+PartialEq+Eq(+Hash) 这一组 derive。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CoverageState {
    /// 已扫描全部声明范围，且没有已知跳过项。
    Complete,
    /// 扫描完成但存在权限、格式或范围跳过项。
    Partial,
    /// 用户取消扫描，保留已发现结果但不得声称完整。
    Cancelled,
    /// 扫描未能生成可用覆盖结果。
    Failed,
}

/// 区分本机索引尚未建立、需要重扫、已扫描空结果与可用调用，避免把采集缺失误作真实零值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LocalIndexState {
    /// 本产品没有任何扫描运行或当前来源，不能把空聚合解释为真实零用量。
    NotScanned,
    /// 已有来源只属于旧 parser generation，必须重建后才能进入当前统计。
    NeedsRescan,
    /// 至少执行过一次扫描，但当前 parser generation 没有可用 canonical 调用。
    ReadyNoCalls,
    /// 当前 parser generation 已包含可参与统计的 canonical 调用。
    Ready,
}

/// 汇总扫描边界与可见跳过计数，不包含绝对路径。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageReport {
    /// 本次扫描的覆盖结论。
    pub state: CoverageState,
    /// 已实际检查的卷级搜索起点或数据根数量，取能够证明覆盖的较大值。
    pub roots_scanned: u64,
    /// 发现的候选数据根数量。
    pub roots_discovered: u64,
    /// 因权限拒绝而跳过的目录数量。
    pub permission_denied_count: u64,
    /// 因网络卷、符号链接、损坏或策略而跳过的数量。
    pub skipped_count: u64,
    /// 无法解析或格式不受支持的记录数量。
    pub warning_count: u64,
}

/// 描述同一逻辑调用出现在哪些只读来源中，不暴露绝对路径。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceProvenance {
    /// adapter 生成的稳定来源 ID。
    pub source_id: String,
    /// adapter 生成的数据根 ID。
    pub root_id: String,
    /// 可安全展示的相对标签或用户别名。
    pub relative_label: String,
    /// 标识来源位于活动还是归档会话区域。
    pub archived: bool,
}

/// 表示从一个 rollout Token 事件规范化出的单次逻辑调用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageCall {
    /// 内容无关且跨活动、归档与复制文件稳定的逻辑调用指纹。
    pub logical_call_id: String,
    /// 调用发生时的 Unix 毫秒时间戳。
    pub occurred_at_epoch_ms: i64,
    /// 上游提供时记录模型名；不含提示词或回答。
    pub model: Option<String>,
    /// 上游提供时记录推理强度。
    pub reasoning_effort: Option<String>,
    /// 内容无关的项目键（哈希）；筛选与分组身份用。
    pub project_key: Option<String>,
    /// 受控路径末段；缺省时展示层回退短哈希或未归类。
    pub project_label: Option<String>,
    /// 内容无关的线程标识。
    pub thread_key: String,
    /// 可选短线程标题；缺省时展示层回退短哈希。
    pub thread_label: Option<String>,
    /// 已校验的单次 Token 用量。
    pub usage: TokenUsage,
    /// 适配器内部的一致性摘要或检查点；不进入 DTO，只用于严格来源的一致性判定。
    // #[serde(skip)]：该字段完全不参与序列化/反序列化，即不会出现在传给
    // 前端的 JSON 里——用于只在 Rust 后端内部流转的敏感或纯内部字段。
    #[serde(skip)]
    pub adapter_consistency_key: Option<String>,
    /// 本次调用事实的置信度。
    pub confidence: Confidence,
    /// 该调用出现过的一个或多个来源。
    pub provenance: Vec<SourceProvenance>,
}

/// Codex 会话级 cumulative Token 兼容快照：每个 `token_count` 事件一条。
///
/// ADR-104 后生产总量只累计 rollout 自有区段的 canonical 调用；本结构继续保留
/// 历史诊断与索引兼容性，不得覆盖窗口、日桶、分组、调用或派生总量。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTokenSnapshot {
    /// 稳定线程键（`stable_id("thread", session_id)`），不是 jsonl 文件名。
    pub thread_key: String,
    /// 事件发生时刻。
    pub occurred_at_epoch_ms: i64,
    /// 该事件的 `total_token_usage.total`（会话累计，含缓存）。
    pub cumulative_total_tokens: u64,
    /// 对应逻辑调用指纹；Ignore 快照用独立稳定 id，供并列时间戳决胜。
    pub logical_call_id: String,
    /// 快照发生时的模型名，仅供历史诊断。
    pub model: Option<String>,
    /// 快照发生时的推理强度，仅供历史诊断。
    pub reasoning_effort: Option<String>,
    /// 快照发生时的项目键，仅供历史诊断。
    pub project_key: Option<String>,
    /// 来源追溯；跨文件副本在 canonical 层合并。
    pub provenance: Vec<SourceProvenance>,
}

/// 表示 canonical 调用集合的本机聚合结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalUsageAggregate {
    /// canonical 调用逐字段求和后的 Token 用量。
    pub tokens: TokenUsage,
    /// canonical 逻辑调用数量。
    pub call_count: u64,
    /// `cached_input_tokens > 0` 的调用数量。
    pub cached_read_call_count: Option<u64>,
    /// 去重后的线程数量。
    pub thread_count: u64,
    /// 当前 canonical 集合 provenance 中去重后的数据根数量。
    pub root_count: u64,
    /// 全部 provenance 中去重后的来源数量。
    pub source_count: u64,
    /// 因活动、归档或跨根复制被合并的重复来源数量。
    pub duplicate_source_count: u64,
    /// 相对单根中最完整的来源观察基线、来自其他根的重复观察数量。
    pub cross_root_duplicate_source_count: u64,
    /// 聚合缓存读取占比的基点数；零输入时为不适用。
    pub cache_read_basis_points: Option<u16>,
    /// 聚合事实的最低置信度。
    pub confidence: Confidence,
}
