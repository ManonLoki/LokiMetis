//! 统一的扫描调度门禁、根标识校验与覆盖聚合策略。
//! 这些规则不依赖具体 GUI 运行时，实现可复用共享。

use thiserror::Error;

use crate::{
    Completeness, CoverageReport, CoverageState, RetentionDays, TimeStandard, WindowBoundaries,
    retention_cutoff_epoch_ms,
};

mod discovery_batch;
mod manual_source_add;
pub mod messages;
mod scan_gate;
mod source_root_alias;
mod source_root_mutation;
mod source_visit;
mod workbuddy_scan_source;

pub use discovery_batch::{
    DiscoveryBatchIndexDecision, DiscoveryBatchKind, discovery_batch_index_decision,
};
pub use manual_source_add::{
    ManualSourceAddDecision, ManualSourceInspectOutcome, decide_manual_source_add,
};
pub use scan_gate::{
    PeriodicQuickScanError, PeriodicScanTick, decide_periodic_scan_tick,
    ensure_periodic_quick_scan_allowed,
};
pub use source_root_alias::source_root_alias_from_path;
pub use source_root_mutation::{
    SourceRootMutationFeedback, SourceRootMutationKind, SourceRootMutationMessageCode,
    SourceRootMutationOutcome, source_root_add_outcome, source_root_mutation_feedback,
    source_root_primary_outcome, source_root_remove_outcome, source_root_rename_outcome,
    source_root_set_enabled_outcome, source_root_toggle_enabled_outcome,
};
pub use source_visit::{SourceFileObservation, retain_ingestable_calls, source_file_needs_visit};
pub use workbuddy_scan_source::{
    WORKBUDDY_HOME_DIR_NAME, WORKBUDDY_PROJECTS_DIR_NAME, WorkbuddyScanSource,
    discover_workbuddy_scan_source, list_scan_source_clients, list_workbuddy_scan_sources,
    workbuddy_home_from_user_home, workbuddy_scan_source_candidate,
};

/// 表示扫描触发的来源类型，决定是否允许后台自动触发。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanStartOrigin {
    /// 用户在向导或数据源页明确点击触发的扫描。
    ExplicitUser,
    /// 完成初始化后认领的首次 Codex 快速扫描。
    InitialAutomatic,
    /// 周期性后台快速扫描。
    PeriodicAutomatic,
}

/// 数据源发现任务固定最多使用两个目录 I/O worker。
pub const LOCAL_DISCOVERY_WORKER_LIMIT: usize = 2;

/// 全部本机 Agent 的 Token 索引共享一个进程级执行名额。
pub const LOCAL_INDEX_WORKER_LIMIT: usize = 1;

/// 表示一次 Token 索引允许回看的本地自然日范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalIndexScanWindow {
    /// 只处理观测日本地日，供已有索引后的自动轮询使用。
    Today,
    /// 处理观测日及前 29 个本地自然日，供首次和用户显式回补使用。
    ThirtyDays,
}

/// 保存 adapter 执行一次索引所需的日期下界。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalIndexScanPolicy {
    /// 本轮按修改时间挑选候选文件的下界；只用于决定是否打开从未索引过的文件，
    /// 已有 checkpoint 且发生变化的文件不受它限制（见 [`source_file_needs_visit`]）。
    pub scan_since_epoch_ms: i64,
    /// 派生数据库永久保留窗口的下界，同时是解析结果允许入库的唯一日期下界
    /// （见 [`retain_ingestable_calls`]）。
    pub retention_since_epoch_ms: i64,
    /// 本轮采用的业务窗口。
    pub window: LocalIndexScanWindow,
}

/// 根据触发来源和当前 generation 是否已有派生用量决定本轮索引窗口。
pub fn local_index_scan_policy(
    origin: ScanStartOrigin,
    has_current_usage: bool,
    observed_at_epoch_ms: i64,
) -> LocalIndexScanPolicy {
    local_index_scan_policy_with_retention(
        origin,
        has_current_usage,
        observed_at_epoch_ms,
        RetentionDays::new(30).unwrap_or_default(),
        TimeStandard::Local,
        &jiff::tz::TimeZone::system(),
    )
}

/// 扫描回看窗口仍按既有 1/30 日规则，派生保留下界改用已保存天数与时间标准。
pub fn local_index_scan_policy_with_retention(
    origin: ScanStartOrigin,
    has_current_usage: bool,
    observed_at_epoch_ms: i64,
    retention_days: RetentionDays,
    time_standard: TimeStandard,
    device_tz: &jiff::tz::TimeZone,
) -> LocalIndexScanPolicy {
    let boundaries = WindowBoundaries::for_local_today(observed_at_epoch_ms);
    let window = if origin == ScanStartOrigin::PeriodicAutomatic && has_current_usage {
        LocalIndexScanWindow::Today
    } else {
        LocalIndexScanWindow::ThirtyDays
    };
    let retention_since_epoch_ms = retention_cutoff_epoch_ms(
        retention_days,
        observed_at_epoch_ms,
        &time_standard,
        device_tz,
    )
    .unwrap_or(boundaries.thirty_days);
    LocalIndexScanPolicy {
        scan_since_epoch_ms: match window {
            LocalIndexScanWindow::Today => boundaries.today,
            LocalIndexScanWindow::ThirtyDays => boundaries.thirty_days,
        },
        retention_since_epoch_ms,
        window,
    }
}

/// 表示扫描方式，影响是否允许作为后台触发来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanKind {
    /// 扫描已登记根，耗时可控且有界。
    Quick,
    /// 全设备发现扫描，耗时长，不允许后台自动触发。
    FullDevice,
}

/// 表示扫描触发门禁拒绝原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ScanStartAccessError {
    /// 初始化未完成前只能允许用户主动触发。
    #[error("initialization has not completed")]
    InitializationRequired,
    /// 后台触发仅允许有界快速扫描。
    #[error("automatic scans only support quick mode")]
    AutomaticScansMustBeQuick,
}

/// 对扫描启动进行来源/初始化/模式统一校验。
pub fn ensure_scan_start_allowed(
    origin: ScanStartOrigin,
    initialization_completed: bool,
    kind: ScanKind,
) -> Result<(), ScanStartAccessError> {
    if matches!(origin, ScanStartOrigin::ExplicitUser) {
        return Ok(());
    }
    if !initialization_completed {
        return Err(ScanStartAccessError::InitializationRequired);
    }
    if kind != ScanKind::Quick {
        return Err(ScanStartAccessError::AutomaticScansMustBeQuick);
    }
    Ok(())
}

/// 返回扫描启动门禁拒绝后的用户提示文案。
pub const fn scan_start_access_error_message(error: ScanStartAccessError) -> &'static str {
    match error {
        ScanStartAccessError::InitializationRequired => "请先完成初始化向导。",
        ScanStartAccessError::AutomaticScansMustBeQuick => "后台扫描只允许有界快速扫描。",
    }
}

/// 表示通用业务入口门禁：初始化未完成时拒绝执行。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BusinessAccessError {
    /// 初始化未完成。
    #[error("initialization has not completed")]
    InitializationRequired,
}

/// 对依赖完成初始化的业务入口进行统一校验。
pub fn ensure_business_access(initialization_completed: bool) -> Result<(), BusinessAccessError> {
    if initialization_completed {
        Ok(())
    } else {
        Err(BusinessAccessError::InitializationRequired)
    }
}

/// 返回业务入口门禁拒绝后的用户提示文案。
pub const fn business_access_error_message(error: BusinessAccessError) -> &'static str {
    match error {
        BusinessAccessError::InitializationRequired => "请先完成初始化向导。",
    }
}

/// 表示源根来源类型，用于稳定的标识前缀校验。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceClientKind {
    /// Codex 稳定标识前缀 `root-`。
    Codex,
    /// Claude Code 稳定标识前缀 `claude-root-`。
    ClaudeCode,
    /// Grok Build CLI 稳定标识前缀 `grok-root-`。
    GrokBuildCli,
    /// WorkBuddy 稳定标识前缀 `workbuddy-root-`。
    WorkBuddy,
}

impl SourceClientKind {
    /// 返回来源根 ID 的稳定前缀。
    fn stable_root_id_prefix(self) -> &'static str {
        match self {
            Self::Codex => "root-",
            Self::ClaudeCode => "claude-root-",
            Self::GrokBuildCli => "grok-root-",
            Self::WorkBuddy => "workbuddy-root-",
        }
    }

    /// 返回登记稳定根 ID 时交给 `stable_id` 的命名空间。
    pub const fn root_id_namespace(self) -> &'static str {
        match self {
            Self::Codex => "root",
            Self::ClaudeCode => "claude-root",
            Self::GrokBuildCli => "grok-root",
            Self::WorkBuddy => "workbuddy-root",
        }
    }

    /// 返回未确认候选根 `stable_id` 使用的命名空间。
    pub const fn candidate_namespace(self) -> &'static str {
        match self {
            Self::Codex => "candidate-codex",
            Self::ClaudeCode => "candidate-claude",
            Self::GrokBuildCli => "candidate-grok",
            Self::WorkBuddy => "candidate-workbuddy",
        }
    }

    /// 返回默认来源根展示名称。
    pub const fn default_source_root_alias(self) -> &'static str {
        match self {
            Self::Codex => "Codex 数据根",
            Self::ClaudeCode => "Claude Code 数据根",
            Self::GrokBuildCli => "Grok 数据根",
            Self::WorkBuddy => "WorkBuddy 数据根",
        }
    }

    /// 返回在来源根列表中展示的环境标签。
    pub const fn source_environment_label(self) -> &'static str {
        match self {
            Self::Codex => "当前 CODEX_HOME",
            Self::ClaudeCode => "当前 CLAUDE_CONFIG_DIR",
            Self::GrokBuildCli => "当前 GROK_HOME",
            Self::WorkBuddy => "当前 WORKBUDDY_HOME",
        }
    }

    /// 返回客户端当前 parser generation（用于跨 CLI/MCP 复用一致的解析边界）。
    ///
    /// 这是读写唯一来源：GUI JSONL writer 的 generation 常量必须等于本值，
    /// 概览/统计/调用/Collect 打开索引也必须用本值。writer 单独加一代而
    /// 此处未加，会导致重建后调用已入库、读取仍按旧代当成空索引。
    pub const fn parser_version(self) -> u32 {
        match self {
            Self::Codex => 8,
            Self::ClaudeCode => 4,
            Self::GrokBuildCli => 3,
            Self::WorkBuddy => 1,
        }
    }
}

/// 表示仅部分客户端可设置主目录。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PrimarySourceRootSupportError {
    /// 当前客户端不允许设置主目录。
    #[error("client does not support primary source root")]
    NotSupported,
}

/// 校验客户端是否允许设置 Codex 主数据目录。
pub fn ensure_primary_source_root_supported(
    client: SourceClientKind,
) -> Result<(), PrimarySourceRootSupportError> {
    if matches!(client, SourceClientKind::Codex) {
        Ok(())
    } else {
        Err(PrimarySourceRootSupportError::NotSupported)
    }
}

/// 表示源根 ID 规则校验失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SourceRootIdValidationError {
    /// 不是预期前缀或长度/字符规则的稳定标识。
    #[error("invalid source root id")]
    Invalid,
}

/// 校验来源根标识：必须是对应客户端固定前缀 + 16 位十六进制。
pub fn validate_source_root_id(
    client: SourceClientKind,
    root_id: &str,
) -> Result<(), SourceRootIdValidationError> {
    let suffix = root_id
        .strip_prefix(client.stable_root_id_prefix())
        .unwrap_or_default();
    if suffix.len() == 16 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(SourceRootIdValidationError::Invalid)
    }
}

/// 表示候选根发现的三态校验失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SourceRootDiscoveryVerificationError {
    /// 发现结果不是单一根，不可用于直接业务主绑定。
    #[error("discovery must contain exactly one candidate root")]
    NotSingleCandidate,
    /// 发现报告包含未确认或失效根，当前身份不可复用。
    #[error("discovery contains unverifiable candidates")]
    DiscoveryNotVerifiable,
    /// 发现的候选根 ID 与调用者要求不一致。
    #[error("discovered root id does not match the requested root")]
    RootIdMismatch,
}

/// 要求 discovery 结果是“单个且可复用根”；可传入期望的 root id 做一致性复核。
pub fn ensure_single_verified_source_root<Root, InvalidRootId, UnconfirmedRootId>(
    discovered_roots: &[Root],
    get_root_id: impl Fn(&Root) -> &str,
    confirmed_invalid_root_ids: &[InvalidRootId],
    unconfirmed_root_ids: &[UnconfirmedRootId],
    requested_root_id: Option<&str>,
) -> Result<(), SourceRootDiscoveryVerificationError>
where
    InvalidRootId: AsRef<str>,
    UnconfirmedRootId: AsRef<str>,
{
    if !confirmed_invalid_root_ids.is_empty() || !unconfirmed_root_ids.is_empty() {
        return Err(SourceRootDiscoveryVerificationError::DiscoveryNotVerifiable);
    }

    if discovered_roots.len() != 1 {
        return Err(SourceRootDiscoveryVerificationError::NotSingleCandidate);
    }

    if let Some(requested_root_id) = requested_root_id {
        let discovered_root_id = get_root_id(&discovered_roots[0]);
        if discovered_root_id != requested_root_id {
            return Err(SourceRootDiscoveryVerificationError::RootIdMismatch);
        }
    }

    Ok(())
}

/// 把“单候选且可复用”验证与结果提取合并为一个纯业务操作。
pub fn extract_single_verified_source_root<'a, Root, InvalidRootId, UnconfirmedRootId>(
    discovered_roots: &'a [Root],
    get_root_id: impl Fn(&Root) -> &str,
    confirmed_invalid_root_ids: &[InvalidRootId],
    unconfirmed_root_ids: &[UnconfirmedRootId],
    requested_root_id: Option<&str>,
) -> Result<&'a Root, SourceRootDiscoveryVerificationError>
where
    InvalidRootId: AsRef<str>,
    UnconfirmedRootId: AsRef<str>,
{
    ensure_single_verified_source_root(
        discovered_roots,
        get_root_id,
        confirmed_invalid_root_ids,
        unconfirmed_root_ids,
        requested_root_id,
    )?;

    discovered_roots
        .first()
        .ok_or(SourceRootDiscoveryVerificationError::NotSingleCandidate)
}

/// 表示源根别名校验失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SourceRootAliasValidationError {
    /// 空值、超长、控制字符或路径分隔符不允许。
    #[error("invalid source root alias")]
    Invalid,
}

/// 规范化并校验源根展示别名（仅保留短文本，不含路径语义字符）。
pub fn normalize_source_root_alias(alias: &str) -> Result<String, SourceRootAliasValidationError> {
    let alias = alias.trim();
    if alias.is_empty()
        || alias.chars().count() > 64
        || alias.chars().any(char::is_control)
        || alias.contains('/')
        || alias.contains('\\')
    {
        Err(SourceRootAliasValidationError::Invalid)
    } else {
        Ok(alias.to_owned())
    }
}

/// 表示筛选字段 ID 校验失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum UsageFilterIdValidationError {
    /// 值为空、过长或包含非法字符。
    #[error("invalid usage filter id")]
    Invalid,
}

/// 校验固定格式的调用筛选 ID（ASCII 技术标签或内部匿名 ID）。
pub fn is_safe_usage_filter_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
}

/// 校验调用筛选 ID，返回稳定错误类型供适配层做文案映射。
pub fn validate_usage_filter_id(value: &str) -> Result<(), UsageFilterIdValidationError> {
    if is_safe_usage_filter_id(value) {
        Ok(())
    } else {
        Err(UsageFilterIdValidationError::Invalid)
    }
}

/// 在展示路径中允许直接透传的技术标签。
pub fn safe_technical_label(value: &str) -> Option<String> {
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

/// 校验模型标签安全可展示后返回去除首尾空白的副本，否则返回 `None`。
/// 各适配器共用同一条规则，集中一份避免两侧各自漂移。
pub fn safe_model_label(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()
        && trimmed.len() <= 128
        && !trimmed.contains(['/', '\\'])
        && !trimmed.chars().any(char::is_control))
    .then(|| trimmed.to_owned())
}

/// 在展示路径中允许直接透传的数据根别名。
pub fn safe_root_label(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value.chars().count() > 64
        || value.chars().any(char::is_control)
        || value.contains('/')
        || value.contains('\\')
    {
        None
    } else {
        Some(value.to_owned())
    }
}

/// 在分页或列表显示上保留内部标识短尾，避免完整 ID 外泄。
pub fn safe_short_value(key: &str) -> String {
    key.chars()
        .rev()
        .take(8)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

/// 将覆盖覆盖状态映射为展示完整度：仅完整扫描为 Complete，其余为 Partial。
pub const fn coverage_completeness(state: CoverageState) -> Completeness {
    match state {
        CoverageState::Complete => Completeness::Complete,
        CoverageState::Partial | CoverageState::Cancelled | CoverageState::Failed => {
            Completeness::Partial
        }
    }
}

/// 合并已确认/全设备发现覆盖报告与扫描覆盖报告。
pub fn merge_coverage_reports(discovery: CoverageReport, scan: &CoverageReport) -> CoverageReport {
    let state = if discovery.state == CoverageState::Cancelled
        || scan.state == CoverageState::Cancelled
    {
        CoverageState::Cancelled
    } else if discovery.state == CoverageState::Failed || scan.state == CoverageState::Failed {
        CoverageState::Failed
    } else if discovery.state == CoverageState::Partial || scan.state == CoverageState::Partial {
        CoverageState::Partial
    } else {
        CoverageState::Complete
    };
    CoverageReport {
        state,
        roots_scanned: discovery.roots_scanned.max(scan.roots_scanned),
        roots_discovered: discovery.roots_discovered,
        permission_denied_count: discovery
            .permission_denied_count
            .saturating_add(scan.permission_denied_count),
        skipped_count: discovery.skipped_count.saturating_add(scan.skipped_count),
        warning_count: discovery.warning_count.saturating_add(scan.warning_count),
    }
}

/// 返回清空索引后不声称已完整覆盖的默认覆盖报告。
pub fn empty_coverage() -> CoverageReport {
    CoverageReport {
        state: CoverageState::Partial,
        roots_scanned: 0,
        roots_discovered: 0,
        permission_denied_count: 0,
        skipped_count: 0,
        warning_count: 0,
    }
}

/// 返回启动时可复用的初始覆盖报告。
pub fn initial_coverage() -> CoverageReport {
    empty_coverage()
}

#[cfg(test)]
mod policy_tests;
