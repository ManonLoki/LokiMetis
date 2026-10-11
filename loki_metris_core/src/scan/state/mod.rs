//! 扫描任务生命周期协调器的共享业务实现。
//! 该模块管理扫描状态机和取消语义，不依赖 GUI、文件系统或外部进程；
//! 临界区只做纯内存状态更新（不含 I/O 或等待），因此用 std 同步锁即可，
//! 不需要把 Tokio 拉进 runtime-neutral 的 core 生产依赖。

mod types;

use std::sync::Mutex;

pub use types::{
    ScanLease, ScanLifecycle, ScanScopeCode, ScanScopeProgress, ScanStateError, ScanStatus,
};

use crate::{
    ScanCancellation, ScanKind, scan_cancelled_status_message, scan_cancelling_status_message,
    scan_completed_status_message, scan_running_status_message,
    scan_scope_local_fixed_volumes_label, scan_scope_registered_roots_label,
};

/// 扫描会话 id 的固定前缀，仅用于人类可读排查，不作为解析契约。
const SCAN_ID_PREFIX: &str = "scan-";

#[derive(Debug)]
/// 保存单个客户端扫描任务的可变生命周期与进度快照。
struct ScanInner {
    status: ScanStatus,
    cancellation: Option<ScanCancellation>,
    /// 单调区分同一协调器历次会话，避免相同毫秒时间戳重用 ID。
    scan_generation: u64,
}

/// 管理单客户端唯一扫描 writer 与取消状态。
#[derive(Debug)]
pub struct ScanCoordinator {
    /// 用同步锁序列化状态更新；临界区只做纯内存写入，不含 I/O 或等待。
    inner: Mutex<ScanInner>,
}

impl Default for ScanCoordinator {
    /// 创建未运行扫描的默认协调器。
    fn default() -> Self {
        Self {
            inner: Mutex::new(ScanInner {
                status: ScanStatus::default(),
                cancellation: None,
                scan_generation: 0,
            }),
        }
    }
}

impl ScanCoordinator {
    /// 启动唯一扫描 writer；返回可用于取消的会话。
    pub fn start(
        &self,
        kind: ScanKind,
        started_at_epoch_ms: i64,
        cancellation: ScanCancellation,
    ) -> Result<ScanLease, ScanStateError> {
        let mut inner = self.inner.lock().unwrap();
        if inner.status.state == ScanLifecycle::Running {
            return Err(ScanStateError::AlreadyRunning);
        }

        inner.scan_generation = inner.scan_generation.saturating_add(1);
        let scan_id = if inner.scan_generation == 1 {
            format!("{SCAN_ID_PREFIX}{started_at_epoch_ms}")
        } else {
            format!(
                "{SCAN_ID_PREFIX}{started_at_epoch_ms}-{}",
                inner.scan_generation
            )
        };
        let (scope_code, scope_label) = initial_scope_for_kind(kind);

        inner.status = ScanStatus {
            scan_id: Some(scan_id.clone()),
            kind,
            state: ScanLifecycle::Running,
            progress_basis_points: 0,
            current_scope_code: scope_code,
            current_scope_label: scope_label,
            scope_progress: ScanScopeProgress::default(),
            files_visited: 0,
            calls_indexed: 0,
            started_at_epoch_ms: Some(started_at_epoch_ms),
            finished_at_epoch_ms: None,
            message: scan_running_status_message().to_owned(),
            is_cancel_requested: false,
        };
        inner.cancellation = Some(cancellation);

        Ok(ScanLease { scan_id })
    }

    /// 只更新仍由指定会话拥有的运行态进度，不允许迟到会话污染下一轮。
    pub fn update_progress(
        &self,
        scan_id: &str,
        files_visited: u64,
        calls_indexed: u64,
        progress_basis_points: u16,
        current_scope_code: ScanScopeCode,
        scope_progress: Option<ScanScopeProgress>,
        current_scope_label: String,
    ) -> bool {
        let mut inner = self.inner.lock().unwrap();
        if inner.status.state != ScanLifecycle::Running
            || inner.status.scan_id.as_deref() != Some(scan_id)
        {
            return false;
        }

        let normalized_progress = progress_basis_points.min(10_000);
        if normalized_progress < inner.status.progress_basis_points {
            return false;
        }

        inner.status.files_visited = inner.status.files_visited.max(files_visited);
        inner.status.calls_indexed = inner.status.calls_indexed.max(calls_indexed);
        inner.status.progress_basis_points = normalized_progress;
        inner.status.current_scope_code = current_scope_code;
        inner.status.current_scope_label = current_scope_label;
        if let Some(scope_progress) = scope_progress {
            inner.status.scope_progress = scope_progress;
        }
        true
    }

    /// 请求取消运行中的任务。
    pub fn request_cancel(&self) -> Result<ScanStatus, ScanStateError> {
        let mut inner = self.inner.lock().unwrap();
        if inner.status.state != ScanLifecycle::Running {
            return Err(ScanStateError::NoActiveScan);
        }

        let cancellation = inner
            .cancellation
            .as_ref()
            .ok_or(ScanStateError::NoActiveScan)?;
        cancellation.cancel();

        inner.status.is_cancel_requested = true;
        inner.status.message = scan_cancelling_status_message().to_owned();

        Ok(inner.status.clone())
    }

    /// 标记扫描已被用户取消。
    pub fn finish_cancelled(&self, scan_id: &str, finished_at_epoch_ms: i64) -> bool {
        self.finish(
            scan_id,
            ScanLifecycle::Cancelled,
            finished_at_epoch_ms,
            scan_cancelled_status_message(),
        )
    }

    /// 标记扫描成功完成。
    pub fn finish_completed(&self, scan_id: &str, finished_at_epoch_ms: i64) -> bool {
        self.finish(
            scan_id,
            ScanLifecycle::Completed,
            finished_at_epoch_ms,
            scan_completed_status_message(),
        )
    }

    /// 标记扫描失败。
    pub fn finish_failed(&self, scan_id: &str, finished_at_epoch_ms: i64, message: &str) -> bool {
        self.finish(
            scan_id,
            ScanLifecycle::Failed,
            finished_at_epoch_ms,
            message,
        )
    }

    /// 仅判定是否处于运行态；避免调用方为读一个字段克隆整份状态。
    pub fn is_running(&self) -> bool {
        self.inner.lock().unwrap().status.state == ScanLifecycle::Running
    }

    /// 返回可轮询的状态快照。
    pub fn snapshot(&self) -> ScanStatus {
        self.inner.lock().unwrap().status.clone()
    }

    /// 原子结束扫描并记录终态时间与安全消息。
    fn finish(
        &self,
        scan_id: &str,
        state: ScanLifecycle,
        finished_at_epoch_ms: i64,
        message: &str,
    ) -> bool {
        let mut inner = self.inner.lock().unwrap();
        if inner.status.state != ScanLifecycle::Running
            || inner.status.scan_id.as_deref() != Some(scan_id)
        {
            return false;
        }
        inner.status.state = state;
        if state == ScanLifecycle::Completed {
            inner.status.progress_basis_points = 10_000;
        }
        inner.status.is_cancel_requested = false;
        inner.status.finished_at_epoch_ms = Some(finished_at_epoch_ms);
        inner.status.message = message.to_owned();
        inner.cancellation = None;
        true
    }
}

/// 根据扫描种类返回初始范围代码与展示文案。
fn initial_scope_for_kind(kind: ScanKind) -> (ScanScopeCode, String) {
    match kind {
        ScanKind::Quick => (
            ScanScopeCode::RegisteredRoots,
            scan_scope_registered_roots_label().to_owned(),
        ),
        ScanKind::FullDevice => (
            ScanScopeCode::LocalFixedVolumes,
            scan_scope_local_fixed_volumes_label().to_owned(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 为状态机测试创建与生产 worker 共用类型的取消令牌。
    fn test_cancellation() -> ScanCancellation {
        ScanCancellation::new()
    }

    /// 验证不允许重入启动同一扫描 writer。
    #[tokio::test]
    async fn disallow_concurrent_writer_starts() {
        let coordinator = ScanCoordinator::default();
        let lease = coordinator
            .start(ScanKind::Quick, 1, test_cancellation())
            .expect("first run starts");

        assert_eq!(
            coordinator.start(ScanKind::FullDevice, 2, test_cancellation()),
            Err(ScanStateError::AlreadyRunning)
        );

        assert!(coordinator.finish_completed(&lease.scan_id, 3));
        assert!(
            coordinator
                .start(ScanKind::FullDevice, 4, test_cancellation())
                .is_ok()
        );
    }

    /// 验证取消会发起信号且返回运行中快照。
    #[tokio::test]
    async fn request_cancel_marks_snapshot_as_requested() {
        let coordinator = ScanCoordinator::default();
        let cancellation = test_cancellation();
        coordinator
            .start(ScanKind::Quick, 1, cancellation.clone())
            .expect("run starts");

        let status = coordinator
            .request_cancel()
            .expect("cancel request succeeds");

        assert!(status.is_cancel_requested);
        assert!(cancellation.is_cancelled());
        assert!(!status.can_cancel());
        assert_eq!(status.state, ScanLifecycle::Running);
        assert_eq!(status.scan_id, Some("scan-1".to_owned()));
    }

    /// 验证进度发布不会把基点倒退。
    #[tokio::test]
    async fn ignore_out_of_order_progress_updates() {
        let coordinator = ScanCoordinator::default();
        let lease = coordinator
            .start(ScanKind::FullDevice, 1, test_cancellation())
            .expect("run starts");

        assert!(coordinator.update_progress(
            &lease.scan_id,
            4,
            7,
            6_250,
            ScanScopeCode::IndexingRoots,
            Some(ScanScopeProgress {
                roots_completed: 4,
                roots_total: 7,
                ..ScanScopeProgress::default()
            }),
            "已完成 4 / 7 个数据根".to_owned(),
        ));

        assert!(!coordinator.update_progress(
            &lease.scan_id,
            0,
            0,
            100,
            ScanScopeCode::DiscoveringVolumes,
            Some(ScanScopeProgress {
                directories_scanned: 1,
                ..ScanScopeProgress::default()
            }),
            "过期阶段".to_owned(),
        ));

        let status = coordinator.snapshot();
        assert_eq!(status.progress_basis_points, 6_250);
        assert_eq!(status.current_scope_label, "已完成 4 / 7 个数据根");
        assert_eq!(status.scope_progress.roots_completed, 4);
    }

    /// 验证同一进度基点的较旧计数不能覆盖已经观察到的累计值。
    #[tokio::test]
    async fn equal_progress_basis_keeps_monotonic_global_counts() {
        let coordinator = ScanCoordinator::default();
        let lease = coordinator
            .start(ScanKind::Quick, 1, test_cancellation())
            .expect("run starts");

        assert!(coordinator.update_progress(
            &lease.scan_id,
            9,
            6,
            5_000,
            ScanScopeCode::IndexingRoots,
            None,
            "较新累计值".to_owned(),
        ));
        assert!(coordinator.update_progress(
            &lease.scan_id,
            7,
            4,
            5_000,
            ScanScopeCode::IndexingRoots,
            None,
            "同基点后续事件".to_owned(),
        ));

        let status = coordinator.snapshot();
        assert_eq!(status.files_visited, 9);
        assert_eq!(status.calls_indexed, 6);
    }

    /// 验证失败文案透传，且失败不会重置为固定完成语。
    #[tokio::test]
    async fn finish_failed_keeps_client_specific_message() {
        let coordinator = ScanCoordinator::default();
        let lease = coordinator
            .start(ScanKind::Quick, 1, test_cancellation())
            .expect("run starts");

        assert!(coordinator.finish_failed(
            &lease.scan_id,
            2,
            &crate::local_scan_client_failure_message(
                "Codex",
                crate::local_scan_task_error_message(),
            ),
        ));

        let status = coordinator.snapshot();
        assert_eq!(status.state, ScanLifecycle::Failed);
        assert!(status.message.contains("Codex 扫描失败"));
    }

    /// 验证旧会话的迟到进度与终态不能命中随后启动的新会话。
    #[tokio::test]
    async fn stale_scan_id_cannot_update_or_finish_next_run() {
        let coordinator = ScanCoordinator::default();
        let first = coordinator
            .start(ScanKind::Quick, 1, test_cancellation())
            .expect("first run starts");
        assert!(coordinator.finish_completed(&first.scan_id, 2));
        let second = coordinator
            .start(ScanKind::FullDevice, 1, test_cancellation())
            .expect("second run starts even with the same observed timestamp");
        assert_ne!(first.scan_id, second.scan_id);

        assert!(!coordinator.update_progress(
            &first.scan_id,
            99,
            88,
            9_000,
            ScanScopeCode::IndexingRoots,
            None,
            "旧会话迟到进度".to_owned(),
        ));
        assert!(!coordinator.finish_cancelled(&first.scan_id, 3));

        let status = coordinator.snapshot();
        assert_eq!(status.scan_id.as_deref(), Some(second.scan_id.as_str()));
        assert_eq!(status.state, ScanLifecycle::Running);
        assert_eq!(status.progress_basis_points, 0);
        assert_eq!(status.files_visited, 0);
        assert_eq!(status.calls_indexed, 0);
    }
}
