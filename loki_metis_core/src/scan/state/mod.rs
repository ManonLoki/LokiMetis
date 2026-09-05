//! 扫描任务生命周期协调器的共享业务实现。
//! 该模块管理扫描状态机和取消语义，不依赖 GUI、文件系统或外部进程；
//! 临界区只做纯内存状态更新（不含 I/O 或等待），因此用 std 同步锁即可，
//! 不需要把 Tokio 拉进 runtime-neutral 的 core 生产依赖。

mod types;

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub use types::{
    ScanLease, ScanLifecycle, ScanScopeCode, ScanScopeProgress, ScanStateError, ScanStatus,
};

use crate::{
    ScanKind, scan_cancelled_status_message, scan_cancelling_status_message,
    scan_completed_status_message, scan_running_status_message,
    scan_scope_local_fixed_volumes_label, scan_scope_registered_roots_label,
};

/// 扫描会话 id 的固定前缀，仅用于人类可读排查，不作为解析契约。
const SCAN_ID_PREFIX: &str = "scan-";

#[derive(Debug)]
/// 保存单个客户端扫描任务的可变生命周期与进度快照。
struct ScanInner {
    status: ScanStatus,
    cancellation: Option<Arc<AtomicBool>>,
}

/// 管理单客户端唯一扫描 writer 与取消状态。
#[derive(Debug)]
pub struct ScanCoordinator {
    /// 使用异步锁序列化状态更新，避免在持有 worker 时直接做 I/O。
    inner: Mutex<ScanInner>,
}

impl Default for ScanCoordinator {
    /// 创建未运行扫描的默认协调器。
    fn default() -> Self {
        Self {
            inner: Mutex::new(ScanInner {
                status: ScanStatus::default(),
                cancellation: None,
            }),
        }
    }
}

impl ScanCoordinator {
    /// 启动唯一扫描 writer；返回可用于取消的会话。
    pub async fn start(
        &self,
        kind: ScanKind,
        started_at_epoch_ms: i64,
    ) -> Result<ScanLease, ScanStateError> {
        let mut inner = self.inner.lock().unwrap();
        if inner.status.state == ScanLifecycle::Running {
            return Err(ScanStateError::AlreadyRunning);
        }

        let scan_id = format!("{SCAN_ID_PREFIX}{started_at_epoch_ms}");
        let cancellation = Arc::new(AtomicBool::new(false));
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

    /// 只在当前运行态上更新非阻塞可见进度，不允许回退。
    pub async fn update_progress(
        &self,
        files_visited: u64,
        calls_indexed: u64,
        progress_basis_points: u16,
        current_scope_code: ScanScopeCode,
        scope_progress: Option<ScanScopeProgress>,
        current_scope_label: String,
    ) {
        let mut inner = self.inner.lock().unwrap();
        if inner.status.state != ScanLifecycle::Running {
            return;
        }

        let normalized_progress = progress_basis_points.min(10_000);
        if normalized_progress < inner.status.progress_basis_points {
            return;
        }

        inner.status.files_visited = files_visited;
        inner.status.calls_indexed = calls_indexed;
        inner.status.progress_basis_points = normalized_progress;
        inner.status.current_scope_code = current_scope_code;
        inner.status.current_scope_label = current_scope_label;
        if let Some(scope_progress) = scope_progress {
            inner.status.scope_progress = scope_progress;
        }
    }

    /// 请求取消运行中的任务。
    pub async fn request_cancel(&self) -> Result<ScanStatus, ScanStateError> {
        let mut inner = self.inner.lock().unwrap();
        if inner.status.state != ScanLifecycle::Running {
            return Err(ScanStateError::NoActiveScan);
        }

        let cancellation = inner
            .cancellation
            .as_ref()
            .ok_or(ScanStateError::NoActiveScan)?;
        cancellation.store(true, Ordering::Release);

        inner.status.is_cancel_requested = true;
        inner.status.message = scan_cancelling_status_message().to_owned();

        Ok(inner.status.clone())
    }

    /// 标记扫描已被用户取消。
    pub async fn finish_cancelled(&self, finished_at_epoch_ms: i64) {
        self.finish(
            ScanLifecycle::Cancelled,
            finished_at_epoch_ms,
            scan_cancelled_status_message(),
        )
        .await;
    }

    /// 标记扫描成功完成。
    pub async fn finish_completed(&self, finished_at_epoch_ms: i64) {
        self.finish(
            ScanLifecycle::Completed,
            finished_at_epoch_ms,
            scan_completed_status_message(),
        )
        .await;
    }

    /// 标记扫描失败。
    pub async fn finish_failed(&self, finished_at_epoch_ms: i64, message: &str) {
        self.finish(ScanLifecycle::Failed, finished_at_epoch_ms, message)
            .await;
    }

    /// 返回可轮询的状态快照。
    pub async fn snapshot(&self) -> ScanStatus {
        self.inner.lock().unwrap().status.clone()
    }

    /// 原子结束扫描并记录终态时间与安全消息。
    async fn finish(&self, state: ScanLifecycle, finished_at_epoch_ms: i64, message: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.status.state = state;
        if state == ScanLifecycle::Completed {
            inner.status.progress_basis_points = 10_000;
        }
        inner.status.is_cancel_requested = false;
        inner.status.finished_at_epoch_ms = Some(finished_at_epoch_ms);
        inner.status.message = message.to_owned();
        inner.cancellation = None;
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

    /// 验证不允许重入启动同一扫描 writer。
    #[tokio::test]
    async fn disallow_concurrent_writer_starts() {
        let coordinator = ScanCoordinator::default();
        coordinator
            .start(ScanKind::Quick, 1)
            .await
            .expect("first run starts");

        assert_eq!(
            coordinator.start(ScanKind::FullDevice, 2).await,
            Err(ScanStateError::AlreadyRunning)
        );

        coordinator.finish_completed(3).await;
        assert!(coordinator.start(ScanKind::FullDevice, 4).await.is_ok());
    }

    /// 验证取消会发起信号且返回运行中快照。
    #[tokio::test]
    async fn request_cancel_marks_snapshot_as_requested() {
        let coordinator = ScanCoordinator::default();
        coordinator
            .start(ScanKind::Quick, 1)
            .await
            .expect("run starts");

        let status = coordinator
            .request_cancel()
            .await
            .expect("cancel request succeeds");

        assert!(status.is_cancel_requested);
        assert!(!status.can_cancel());
        assert_eq!(status.state, ScanLifecycle::Running);
        assert_eq!(status.scan_id, Some("scan-1".to_owned()));
    }

    /// 验证进度发布不会把基点倒退。
    #[tokio::test]
    async fn ignore_out_of_order_progress_updates() {
        let coordinator = ScanCoordinator::default();
        coordinator
            .start(ScanKind::FullDevice, 1)
            .await
            .expect("run starts");

        coordinator
            .update_progress(
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
            )
            .await;

        coordinator
            .update_progress(
                0,
                0,
                100,
                ScanScopeCode::DiscoveringVolumes,
                Some(ScanScopeProgress {
                    directories_scanned: 1,
                    ..ScanScopeProgress::default()
                }),
                "过期阶段".to_owned(),
            )
            .await;

        let status = coordinator.snapshot().await;
        assert_eq!(status.progress_basis_points, 6_250);
        assert_eq!(status.current_scope_label, "已完成 4 / 7 个数据根");
        assert_eq!(status.scope_progress.roots_completed, 4);
    }

    /// 验证失败文案透传，且失败不会重置为固定完成语。
    #[tokio::test]
    async fn finish_failed_keeps_client_specific_message() {
        let coordinator = ScanCoordinator::default();
        coordinator
            .start(ScanKind::Quick, 1)
            .await
            .expect("run starts");

        coordinator
            .finish_failed(
                2,
                &crate::local_scan_client_failure_message(
                    "Codex",
                    crate::local_scan_task_error_message(),
                ),
            )
            .await;

        let status = coordinator.snapshot().await;
        assert_eq!(status.state, ScanLifecycle::Failed);
        assert!(status.message.contains("Codex 扫描失败"));
    }
}
