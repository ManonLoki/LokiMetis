//! 扫描任务生命周期相关的纯业务模型与快照结构。
//! 该模块仅定义状态与快照，不承载锁、异步或 I/O 依赖。

use thiserror::Error;

use crate::{ScanKind, scan_idle_status_message, scan_scope_registered_roots_label};

/// 表示扫描启动被拒绝的稳定错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ScanStateError {
    /// 已存在运行中的扫描任务，当前 writer 不可重入。
    #[error("scan is already running")]
    AlreadyRunning,
    /// 当前没有可取消的扫描任务。
    #[error("no cancellable scan is running")]
    NoActiveScan,
}

/// 表示一次已发起扫描的会话令牌。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanLease {
    /// 当前扫描会话唯一 ID。
    pub scan_id: String,
}

/// 表示扫描生命周期稳定状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanLifecycle {
    /// 当前无运行任务。
    Idle,
    /// 扫描正在执行。
    Running,
    /// 扫描执行完成。
    Completed,
    /// 扫描被取消。
    Cancelled,
    /// 扫描失败。
    Failed,
}

impl Default for ScanLifecycle {
    /// 空闲状态作为默认生命周期。
    fn default() -> Self {
        Self::Idle
    }
}

/// 表示可显示阶段及其展示标签类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanScopeCode {
    /// 已登记和默认数据根。
    RegisteredRoots,
    /// 用户授权的本地固定卷。
    LocalFixedVolumes,
    /// 正在发现本地固定卷。
    DiscoveringVolumes,
    /// 本地固定卷发现完成。
    DiscoveryFinished,
    /// 正在索引已确认根。
    IndexingRoots,
}

/// 表示扫描阶段内可展示的计数快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanScopeProgress {
    /// 当前正在索引的数据根稳定 ID；发现阶段为空。
    pub current_root_id: Option<String>,
    /// 已扫描目录数。
    pub directories_scanned: u64,
    /// 已确认根数。
    pub roots_discovered: u64,
    /// 已完成根数。
    pub roots_completed: u64,
    /// 待完成根总数。
    pub roots_total: u64,
}

impl Default for ScanScopeProgress {
    /// 计数归零作为阶段快照默认值。
    fn default() -> Self {
        Self {
            current_root_id: None,
            directories_scanned: 0,
            roots_discovered: 0,
            roots_completed: 0,
            roots_total: 0,
        }
    }
}

/// 表示一次可轮询扫描状态快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanStatus {
    /// 当前扫描 ID，空闲时为空。
    pub scan_id: Option<String>,
    /// 扫描模式。
    pub kind: ScanKind,
    /// 扫描生命周期。
    pub state: ScanLifecycle,
    /// 0–10,000 进度基点。
    pub progress_basis_points: u16,
    /// 当前范围展示文本。
    pub current_scope_label: String,
    /// 当前范围编码。
    pub current_scope_code: ScanScopeCode,
    /// 当前阶段计数快照。
    pub scope_progress: ScanScopeProgress,
    /// 任务是否可接受取消。
    pub can_cancel: bool,
    /// 已访问文件计数。
    pub files_visited: u64,
    /// 本轮新增调用计数。
    pub calls_indexed: u64,
    /// 启动时间。
    pub started_at_epoch_ms: Option<i64>,
    /// 结束时间。
    pub finished_at_epoch_ms: Option<i64>,
    /// 状态文案。
    pub message: String,
    /// 是否已发起取消请求。
    pub is_cancel_requested: bool,
}

impl Default for ScanStatus {
    /// 以“尚未开始扫描”为可复用的空闲基线。
    fn default() -> Self {
        Self {
            scan_id: None,
            kind: ScanKind::Quick,
            state: ScanLifecycle::Idle,
            progress_basis_points: 0,
            current_scope_label: scan_scope_registered_roots_label().to_owned(),
            current_scope_code: ScanScopeCode::RegisteredRoots,
            scope_progress: ScanScopeProgress::default(),
            can_cancel: false,
            files_visited: 0,
            calls_indexed: 0,
            started_at_epoch_ms: None,
            finished_at_epoch_ms: None,
            message: scan_idle_status_message().to_owned(),
            is_cancel_requested: false,
        }
    }
}
