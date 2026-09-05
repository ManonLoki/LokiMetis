//! 数据源页的数据根、元数据发现与手动登记结果。

use loki_metis_core::CoverageReport;
use serde::{Deserialize, Serialize};

use super::{AgentClientKindDto, ScanStatusDto, UiMessageCodeDto};

/// 标识数据根进入 registry 的稳定来源类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceDiscoveryCodeDto {
    /// 客户端默认数据根。
    DefaultRoot,
    /// 当前进程继承的 Codex Home。
    CodexEnvironment,
    /// 当前进程继承的 Claude 配置目录。
    ClaudeEnvironment,
    /// 当前进程继承的 Grok home。
    GrokEnvironment,
    /// 用户通过原生选择器登记。
    UserRegistered,
    /// 用户主动全设备发现。
    FullDevice,
    /// 用户确认的元数据发现候选。
    MetadataDiscovery,
}

/// 描述一个授权数据根的安全可见状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRootDto {
    /// adapter 生成的稳定根 ID。
    pub id: String,
    /// 用户别名或安全路径末段。
    pub alias: String,
    /// 数据根是否参与扫描。
    pub enabled: bool,
    /// 首次确认、索引中、就绪或验证失败的状态。
    pub activation_state: RootActivationStateDto,
    /// Codex 根是否为当前唯一主数据目录。
    pub is_primary: bool,
    /// 快速发现、自定义或全设备发现来源的中文名称。
    pub discovery_label: String,
    /// 数据根来源的稳定本地化代码。
    pub discovery_code: SourceDiscoveryCodeDto,
    /// 当前根的 rollout 文件数。
    pub file_count: u64,
    /// 当前根跳过的文件或目录数。
    pub skipped_count: u64,
    /// 当前根扫描错误数。
    pub error_count: u64,
    /// 当前根被去重的来源数量。
    pub duplicate_count: u64,
    /// 最近成功扫描时的 Unix 毫秒时间戳。
    pub last_scan_at_epoch_ms: Option<i64>,
}

/// 数据根首次索引激活状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RootActivationStateDto {
    /// 已确认但尚未读取。
    ConfirmedUnindexed,
    /// 显式索引正在运行。
    Indexing,
    /// 已验证并可周期更新。
    Ready,
    /// 没有读到合法用量记录。
    ValidationFailed,
}

/// 元数据发现任务生命周期。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RootDiscoveryStateDto {
    /// 尚未运行。
    Idle,
    /// 正在发现。
    Running,
    /// 完整遍历结束。
    Complete,
    /// 存在明确覆盖缺口。
    Partial,
    /// 用户取消。
    Cancelled,
    /// 无法继续。
    Failed,
}

/// 元数据发现使用的平台策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RootDiscoveryStrategyDto {
    /// Windows Search。
    WindowsSearch,
    /// macOS Spotlight。
    MacOsSpotlight,
    /// 普通元数据遍历。
    MetadataTraversal,
}

/// 当前客户端运行平台。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RootDiscoveryPlatformDto {
    /// Windows。
    Windows,
    /// macOS。
    MacOs,
    /// 当前未提供专属优先列表的平台。
    Other,
}

/// 用户选择的数据源发现范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RootDiscoveryScopeDto {
    /// 当前平台的优先用户目录。
    UserPriority,
    /// 所有可确认的本地卷。
    FullLocalVolumes,
    /// 用户手动选择的单个本地子树。
    ManualSubtree,
}

/// 不包含虚假百分比的发现任务状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootDiscoveryStatusDto {
    /// 生命周期。
    pub state: RootDiscoveryStateDto,
    /// 当前平台策略。
    pub strategy: RootDiscoveryStrategyDto,
    /// 当前运行平台。
    pub platform: RootDiscoveryPlatformDto,
    /// 当前发现范围。
    pub scope: RootDiscoveryScopeDto,
    /// 系统索引是否可用。
    pub system_index_available: bool,
    /// 是否执行普通遍历兜底。
    pub fallback_performed: bool,
    /// 已完成卷数。
    pub volumes_completed: u64,
    /// 本地卷总数。
    pub volumes_total: u64,
    /// 已检查目录数。
    pub directories_checked: u64,
    /// 已检查文件名数。
    pub file_names_checked: u64,
    /// 候选数。
    pub candidates_found: u64,
    /// 权限拒绝数。
    pub permission_denied: u64,
    /// I/O 错误数。
    pub io_errors: u64,
    /// 策略跳过数。
    pub skipped: u64,
    /// 稳定错误码。
    pub error_code: Option<String>,
}

/// 单个内存候选；完整路径只供当前确认界面显示。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootCandidateDto {
    /// 临时 ID。
    pub id: String,
    /// 客户端。
    pub client: AgentClientKindDto,
    /// 完整绝对路径。
    pub absolute_path: String,
    /// 发现来源。
    pub strategy: RootDiscoveryStrategyDto,
    /// 稳定结构证据码。
    pub evidence: String,
}

/// 单个候选添加到对应客户端后的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddRootCandidateDto {
    /// 候选所属客户端。
    pub client: AgentClientKindDto,
    /// 登记后的稳定数据根 ID。
    pub root_id: String,
    /// 是否实际新增了数据根。
    pub added: bool,
    /// 当前后台统计激活状态。
    pub background_state: RootActivationStateDto,
}

/// 描述数据根 registry 变更的脱敏结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRootMutationDto {
    /// 标识本产品索引中的 registry 确已变更。
    pub changed: bool,
    /// 明确原始 Codex 文件未被修改的中文说明。
    pub message: String,
    /// 操作结果的稳定本地化代码。
    pub message_code: UiMessageCodeDto,
}

/// 手动添加数据根的稳定结果类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ManualAddOutcomeDto {
    /// 用户取消了原生目录选择。
    Cancelled,
    /// 所选路径已作为合格根新登记。
    Registered,
    /// 所选路径对应根已存在。
    AlreadyRegistered,
    /// 所选路径不是根，已启动子树深搜。
    DeepSearchStarted,
}

/// 手动添加命令结果；路径永不进入响应。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualAddSourceRootDto {
    /// 结果类别。
    pub outcome: ManualAddOutcomeDto,
    /// registry 是否因本次直登而变更。
    pub changed: bool,
    /// 可本地化消息码。
    pub message_code: UiMessageCodeDto,
    /// 深搜启动后的发现状态快照。
    pub discovery: Option<RootDiscoveryStatusDto>,
}

/// 描述数据源页的本机数据根、覆盖与扫描状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcesDto {
    /// 当前授权且不含绝对路径的数据根。
    pub roots: Vec<SourceRootDto>,
    /// 最近一次扫描覆盖结论。
    pub coverage: CoverageReport,
    /// 当前唯一扫描任务状态。
    pub scan: ScanStatusDto,
}
