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
    /// 当前扫描的取消令牌；空闲时为空。
    current: Mutex<Option<CancellationToken>>,
}

impl Default for CoordinatorInner {
    /// 创建一个可被显式批次异步等待、周期任务非阻塞尝试的 writer 名额。
    fn default() -> Self {
        Self {
            slot: Arc::new(Semaphore::new(1)),
            current: Mutex::new(None),
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
        Ok(self.activate(slot))
    }

    /// 异步等待唯一 writer，供一次显式批次可靠地排在当前任务之后而不忙轮询。
    pub async fn start_when_available(&self) -> Result<ScanPermit, LocalError> {
        let slot = Arc::clone(&self.inner.slot)
            .acquire_owned()
            .await
            .map_err(|_| {
                LocalError::new(LocalErrorKind::ScanBusy, "local scan coordinator is closed")
            })?;
        Ok(self.activate(slot))
    }

    /// 在已取得 writer 名额后安装本轮取消令牌并构造 RAII 许可。
    fn activate(&self, slot: OwnedSemaphorePermit) -> ScanPermit {
        let cancellation = CancellationToken::new();
        let mut current = self
            .inner
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *current = Some(cancellation.clone());
        drop(current);
        ScanPermit {
            inner: Arc::clone(&self.inner),
            cancellation,
            _slot: slot,
        }
    }

    /// 请求当前活动扫描取消；没有活动扫描时返回 false。
    #[cfg(test)]
    pub fn cancel_active(&self) -> bool {
        let current = self
            .inner
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(cancellation) = current.as_ref() {
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
}

impl ScanPermit {
    /// 返回交给发现和文件解析的共享取消令牌。
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }
}

impl Drop for ScanPermit {
    /// 释放 writer 并清除取消句柄，避免扫描结束后误取消下一次任务。
    fn drop(&mut self) {
        let mut current = self
            .inner
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *current = None;
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

        assert_eq!(
            coordinator
                .try_start()
                .expect_err("second writer is rejected")
                .kind(),
            LocalErrorKind::ScanBusy
        );
        assert!(coordinator.cancel_active());
        assert!(permit.cancellation_token().is_cancelled());
        drop(permit);
        assert!(!coordinator.is_running());
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
}
