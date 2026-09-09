//! 在 GUI backend 内协调单 writer 扫描、逐目录取消、进度与覆盖报告；
//! 实际的目录遍历与逐文件索引收在 [`traversal`] 子模块内。

mod traversal;

use std::sync::{Arc, Mutex};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use loki_metis_core::CoverageReport;
pub use loki_metis_core::ScanCancellation;

use super::{DEFAULT_MAX_JSONL_LINE_BYTES, LocalError, LocalErrorKind};

pub use ScanCancellation as CancellationToken;

pub use traversal::scan_discovered_roots;

/// 标识扫描来自快速已知根还是用户主动全设备发现候选。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanMode {
    /// 扫描默认、环境与用户登记根。
    Quick,
    /// 扫描用户主动全设备发现返回的候选根。
    FullDevice,
}

impl ScanMode {
    /// 返回 scan_runs 使用的稳定模式标签。
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::FullDevice => "full_device",
        }
    }
}

/// 配置一次本机扫描；调用方在 async 任务内直接 await 扫描壳。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanConfig {
    /// 扫描入口类型。
    pub mode: ScanMode,
    /// 单条 JSONL 的内存上限。
    pub max_line_bytes: usize,
    /// 一次索引阶段最多进入的目录数量。
    pub max_directories: u64,
    /// 一次索引阶段最多处理的目录项数量。
    pub max_entries: u64,
    /// 一次索引任务最多执行首记录签名探测的文件数量。
    pub max_signature_files: u64,
    /// 一次索引任务最多读取用于首记录签名探测的字节数量。
    pub max_signature_bytes: u64,
    /// 本轮按修改时间挑选候选文件的下界；已有 checkpoint 且发生变化的文件不受限制。
    pub scan_since_epoch_ms: i64,
    /// 解析结果允许入库的发生时间下界，取派生库的永久保留窗口；
    /// 不再复用 `scan_since_epoch_ms`，避免追加或重建时丢弃窗口外的真实调用。
    pub ingest_since_epoch_ms: i64,
    /// 即使来源检查点未变化，也强制从头生成新的来源 generation。
    pub force_rebuild: bool,
    /// 调用方记录的开始时间；为空时 adapter 使用当前系统时间。
    pub started_at_epoch_ms: Option<i64>,
}

impl Default for ScanConfig {
    /// 返回快速扫描与 1 MiB 单行上限。
    fn default() -> Self {
        Self {
            mode: ScanMode::Quick,
            max_line_bytes: DEFAULT_MAX_JSONL_LINE_BYTES,
            max_directories: u64::MAX,
            max_entries: u64::MAX,
            max_signature_files: u64::MAX,
            max_signature_bytes: u64::MAX,
            scan_since_epoch_ms: i64::MIN,
            ingest_since_epoch_ms: i64::MIN,
            force_rebuild: false,
            started_at_epoch_ms: None,
        }
    }
}

/// 提供不含绝对路径的扫描进度快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanProgress {
    /// 当前扫描运行 ID。
    pub scan_id: String,
    /// 当前正在索引的数据根稳定 ID；不包含绝对路径。
    pub current_root_id: String,
    /// 已完成数据根数量。
    pub roots_completed: u64,
    /// 本次声明的数据根总数。
    pub roots_total: u64,
    /// 已处理 rollout 文件数量。
    pub files_scanned: u64,
    /// 本次首次写入来源 generation 的调用数量。
    pub calls_added: u64,
    /// 当前累计格式、权限和跳过警告数量。
    pub warning_count: u64,
}

/// 汇总一次扫描、覆盖与最终 canonical 调用计数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanSummary {
    /// 当前扫描运行 ID。
    pub scan_id: String,
    /// 不含主机路径的覆盖结论。
    pub coverage: CoverageReport,
    /// 已处理 rollout 文件数量。
    pub files_scanned: u64,
    /// 因文件 checkpoint 无变化而跳过读取的数量。
    pub unchanged_files: u64,
    /// 因截断、替换或解析器升级重建的来源数量。
    pub rebuilt_files: u64,
    /// 本次首次写入来源 generation 的调用数量。
    pub calls_added: u64,
    /// 当前全部启用来源按逻辑调用 ID 去重后的数量。
    pub call_count: u64,
}

/// 在一个进程内保证同时最多只有一个本机索引 writer。
#[derive(Clone, Default)]
pub struct ScanCoordinator {
    /// 跨克隆共享的内部状态。
    inner: Arc<CoordinatorInner>,
}

/// 独占写扫描协调器的共享内部状态：是否有扫描在跑，及其取消令牌。
#[derive(Debug)]
struct CoordinatorInner {
    /// 公平串行化显式批次与周期任务的唯一 writer 名额。
    slot: Arc<Semaphore>,
    /// 与关闭门禁一起线性化当前扫描登记，避免 shutdown 漏掉刚取得名额的任务。
    state: Mutex<CoordinatorState>,
}

impl CoordinatorInner {
    /// 取得登记表锁；测试 panic 污染锁后仍保留取消与关闭能力。
    fn lock_state(&self) -> std::sync::MutexGuard<'_, CoordinatorState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// 必须在同一同步锁内读取和更新的扫描登记与不可逆关闭状态。
#[derive(Debug, Default)]
struct CoordinatorState {
    /// 当前扫描的取消令牌；空闲时为空。
    current: Option<CancellationToken>,
    /// 关闭一旦开始即永久为 true，之后不得再登记扫描。
    shutting_down: bool,
}

impl Default for CoordinatorInner {
    /// 创建一个可被显式批次异步等待、周期任务非阻塞尝试的 writer 名额。
    fn default() -> Self {
        Self {
            slot: Arc::new(Semaphore::new(1)),
            state: Mutex::new(CoordinatorState::default()),
        }
    }
}

/// 表示一次独占写扫描许可；释放时自动清理 active 状态。
// RAII（Resource Acquisition Is Initialization）模式：这个结构体本身
// 就代表“持有写扫描权限”这件事，构造它（try_start 成功）即获得权限，
// 它被 drop（无论是正常执行完、提前 return、还是 panic 展开栈）时，
// 下面的 `impl Drop` 会自动执行清理，不需要调用方手动记得“扫描完了要
// 释放锁”——不可能出现忘记释放导致永久卡死的 bug。
#[derive(Debug)]
pub struct ScanPermit {
    /// 关联的协调器内部状态，drop 时清理 active 标记。
    inner: Arc<CoordinatorInner>,
    /// 本次扫描的取消令牌。
    cancellation: CancellationToken,
    /// 持有期间阻止其它客户端或批次进入同一索引 writer。
    _slot: OwnedSemaphorePermit,
}

impl ScanCoordinator {
    /// 尝试取得唯一 writer 许可；已有扫描时返回稳定 busy 错误。
    pub fn try_start(&self) -> Result<ScanPermit, LocalError> {
        let slot = Arc::clone(&self.inner.slot)
            .try_acquire_owned()
            .map_err(|_| {
                LocalError::new(LocalErrorKind::ScanBusy, "a local scan is already running")
            })?;
        self.activate(slot)
    }

    /// 异步等待唯一 writer，供一次显式批次可靠地排在当前任务之后而不忙轮询。
    pub async fn start_when_available(&self) -> Result<ScanPermit, LocalError> {
        let slot = Arc::clone(&self.inner.slot)
            .acquire_owned()
            .await
            .map_err(|_| {
                LocalError::new(LocalErrorKind::ScanBusy, "local scan coordinator is closed")
            })?;
        self.activate(slot)
    }

    /// 在已取得 writer 名额后原子检查关闭门禁、安装取消令牌并构造 RAII 许可。
    fn activate(&self, slot: OwnedSemaphorePermit) -> Result<ScanPermit, LocalError> {
        let cancellation = CancellationToken::new();
        let mut state = self.inner.lock_state();
        if state.shutting_down {
            return Err(LocalError::new(
                LocalErrorKind::ScanBusy,
                "local scan coordinator is closed",
            ));
        }
        state.current = Some(cancellation.clone());
        drop(state);
        Ok(ScanPermit {
            inner: Arc::clone(&self.inner),
            cancellation,
            _slot: slot,
        })
    }

    /// 请求当前活动扫描取消；没有活动扫描时返回 false。
    #[cfg(test)]
    pub fn cancel_active(&self) -> bool {
        let state = self.inner.lock_state();
        if let Some(cancellation) = state.current.as_ref() {
            cancellation.cancel();
            true
        } else {
            false
        }
    }

    /// 返回当前是否有 writer 持有扫描许可。
    pub fn is_running(&self) -> bool {
        self.inner.slot.available_permits() == 0
    }

    /// 关闭 writer 入口并请求当前扫描取消；等待中的显式批次会立即失败。
    pub fn shutdown(&self) {
        let mut state = self.inner.lock_state();
        state.shutting_down = true;
        self.inner.slot.close();
        if let Some(cancellation) = state.current.as_ref() {
            cancellation.cancel();
        }
    }
}

impl ScanPermit {
    /// 返回交给发现和文件解析的共享取消令牌。
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }
}

impl Drop for ScanPermit {
    /// 先取消仍可能运行的阻塞岛，再释放 writer 并清除当前句柄。
    fn drop(&mut self) {
        self.cancellation.cancel();
        let mut state = self.inner.lock_state();
        state.current = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证 coordinator 拒绝第二个 writer，并能取消当前扫描。
    #[test]
    fn coordinator_enforces_single_writer_and_cancels_active_scan() {
        let coordinator = ScanCoordinator::default();
        let permit = coordinator.try_start().expect("first writer is accepted");
        let cancellation = permit.cancellation_token();

        assert_eq!(
            coordinator
                .try_start()
                .expect_err("second writer is rejected")
                .kind(),
            LocalErrorKind::ScanBusy
        );
        assert!(coordinator.cancel_active());
        assert!(cancellation.is_cancelled());
        drop(permit);
        assert!(!coordinator.is_running());
    }

    /// 验证许可释放会取消仍持有其令牌的工作，并让正常协调器重新可用。
    #[test]
    fn dropping_permit_cancels_worker_token_and_releases_slot() {
        let coordinator = ScanCoordinator::default();
        let permit = coordinator.try_start().expect("writer starts");
        let cancellation = permit.cancellation_token();

        drop(permit);

        assert!(cancellation.is_cancelled());
        assert!(coordinator.try_start().is_ok());
    }

    /// 验证显式批次会异步等待已有 writer 释放，而不是返回 busy 或忙轮询。
    #[tokio::test]
    async fn explicit_batch_waits_for_current_writer() {
        let coordinator = ScanCoordinator::default();
        let permit = coordinator.try_start().expect("first writer is accepted");
        let waiting_coordinator = coordinator.clone();
        let waiter = tokio::spawn(async move {
            waiting_coordinator
                .start_when_available()
                .await
                .expect("waiting writer is eventually accepted")
        });
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());
        drop(permit);
        let next = waiter.await.expect("waiter task completes");
        assert!(coordinator.is_running());
        drop(next);
        assert!(!coordinator.is_running());
    }

    /// 验证关闭会取消当前任务、唤醒等待者，并永久拒绝新的 writer。
    #[tokio::test]
    async fn shutdown_cancels_active_writer_and_closes_waiters() {
        let coordinator = ScanCoordinator::default();
        let permit = coordinator.try_start().expect("writer starts");
        let cancellation = permit.cancellation_token();
        let waiting_coordinator = coordinator.clone();
        let waiter = tokio::spawn(async move { waiting_coordinator.start_when_available().await });
        tokio::task::yield_now().await;

        coordinator.shutdown();

        assert!(cancellation.is_cancelled());
        assert!(waiter.await.expect("waiter joins").is_err());
        drop(permit);
        assert!(coordinator.try_start().is_err());
    }

    /// 验证关闭在线性化上先于 token 登记时，已取得的槽位也不能漏登记并继续扫描。
    #[test]
    fn acquired_slot_cannot_activate_after_shutdown_wins_registration_race() {
        let coordinator = ScanCoordinator::default();
        let slot = Arc::clone(&coordinator.inner.slot)
            .try_acquire_owned()
            .expect("writer slot is acquired before registration");

        coordinator.shutdown();

        assert_eq!(
            coordinator
                .activate(slot)
                .expect_err("shutdown rejects the delayed registration")
                .kind(),
            LocalErrorKind::ScanBusy
        );
        assert!(!coordinator.cancel_active());
        assert!(coordinator.try_start().is_err());
    }
}
