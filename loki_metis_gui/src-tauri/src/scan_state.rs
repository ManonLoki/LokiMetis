//! 管理 GUI 内唯一扫描任务的状态、进度与取消所有权。
//! 该模块不直接处理文件系统与数据库，仅做 DTO 与 shared core 模型映射。

use crate::dto::{
    ScanKindDto, ScanScopeCodeDto, ScanScopeProgressDto, ScanStateDto, ScanStatusDto,
    UiMessageCodeDto,
};
use loki_metis_core::{
    ScanCancellation, ScanCoordinator as CoreScanCoordinator, ScanLease as CoreScanLease,
    ScanLifecycle, ScanScopeCode, ScanScopeProgress, ScanStateError as CoreScanStateError,
    ScanStatus as CoreScanStatus,
};

/// 表示扫描协调器拒绝重复启动或无任务取消的稳定错误。
pub type ScanStateError = CoreScanStateError;

/// 表示一次已启动扫描的运行期租约。
pub type ScanLease = CoreScanLease;

/// 管理当前客户端唯一扫描 writer，并为前端返回可轮询快照。
#[derive(Debug)]
pub struct ScanCoordinator {
    /// core 侧实现唯一 writer 许可与状态机的协调器。
    core: CoreScanCoordinator,
}

impl Default for ScanCoordinator {
    /// 创建尚未运行任何扫描的协调器。
    fn default() -> Self {
        Self {
            core: CoreScanCoordinator::default(),
        }
    }
}

impl ScanCoordinator {
    /// 启动唯一扫描 writer，并返回其必须轮询的状态。
    pub fn start(
        &self,
        kind: ScanKindDto,
        started_at_epoch_ms: i64,
        cancellation: ScanCancellation,
    ) -> Result<ScanLease, ScanStateError> {
        self.core
            .start(to_core_scan_kind(kind), started_at_epoch_ms, cancellation)
    }

    /// 仅为仍拥有当前运行态的会话更新轻量进度。
    pub fn update_progress(
        &self,
        scan_id: &str,
        files_visited: u64,
        calls_indexed: u64,
        progress_basis_points: u16,
        current_scope_code: ScanScopeCodeDto,
        scope_progress: Option<ScanScopeProgressDto>,
        current_scope_label: String,
    ) -> bool {
        self.core.update_progress(
            scan_id,
            files_visited,
            calls_indexed,
            progress_basis_points,
            to_core_scope_code(current_scope_code),
            scope_progress.map(|value| ScanScopeProgress {
                current_root_id: value.current_root_id,
                directories_scanned: value.directories_scanned,
                roots_discovered: value.roots_discovered,
                roots_completed: value.roots_completed,
                roots_total: value.roots_total,
            }),
            current_scope_label,
        )
    }

    /// 请求取消当前扫描；实际停止由 worker 在受控边界轮询确认。
    #[cfg(test)]
    pub fn request_cancel(&self) -> Result<ScanStatusDto, ScanStateError> {
        Ok(to_dto_scan_status(self.core.request_cancel()?))
    }

    /// 标记 worker 已确认取消，并把覆盖语义留给本机索引模块汇报。
    pub fn finish_cancelled(&self, scan_id: &str, finished_at_epoch_ms: i64) -> bool {
        self.core.finish_cancelled(scan_id, finished_at_epoch_ms)
    }

    /// 标记扫描成功，并把进度固定为 100%。
    pub fn finish_completed(&self, scan_id: &str, finished_at_epoch_ms: i64) -> bool {
        self.core.finish_completed(scan_id, finished_at_epoch_ms)
    }

    /// 标记扫描失败；调用方只传入已经脱敏且带客户端归属的稳定说明。
    pub fn finish_failed(&self, scan_id: &str, finished_at_epoch_ms: i64, message: &str) -> bool {
        self.core
            .finish_failed(scan_id, finished_at_epoch_ms, message)
    }

    /// 仅判定当前是否有扫描在运行，不构造完整 DTO 快照。
    pub fn is_running(&self) -> bool {
        self.core.is_running()
    }

    /// 返回前端可安全轮询的当前状态副本。
    pub fn snapshot(&self) -> ScanStatusDto {
        to_dto_scan_status(self.core.snapshot())
    }
}

/// 把 DTO 扫描范围映射为 core 的扫描范围枚举。
fn to_core_scan_kind(kind: ScanKindDto) -> loki_metis_core::ScanKind {
    match kind {
        ScanKindDto::Quick => loki_metis_core::ScanKind::Quick,
        ScanKindDto::FullDevice => loki_metis_core::ScanKind::FullDevice,
    }
}

/// 把 DTO 扫描阶段代码映射为 core 的扫描阶段代码。
fn to_core_scope_code(code: ScanScopeCodeDto) -> ScanScopeCode {
    match code {
        ScanScopeCodeDto::RegisteredRoots => ScanScopeCode::RegisteredRoots,
        ScanScopeCodeDto::LocalFixedVolumes => ScanScopeCode::LocalFixedVolumes,
        ScanScopeCodeDto::DiscoveringVolumes => ScanScopeCode::DiscoveringVolumes,
        ScanScopeCodeDto::DiscoveryFinished => ScanScopeCode::DiscoveryFinished,
        ScanScopeCodeDto::IndexingRoots => ScanScopeCode::IndexingRoots,
    }
}

/// 把 core 的扫描阶段代码映射为 DTO 的扫描阶段代码。
fn to_dto_scope_code(code: ScanScopeCode) -> ScanScopeCodeDto {
    match code {
        ScanScopeCode::RegisteredRoots => ScanScopeCodeDto::RegisteredRoots,
        ScanScopeCode::LocalFixedVolumes => ScanScopeCodeDto::LocalFixedVolumes,
        ScanScopeCode::DiscoveringVolumes => ScanScopeCodeDto::DiscoveringVolumes,
        ScanScopeCode::DiscoveryFinished => ScanScopeCodeDto::DiscoveryFinished,
        ScanScopeCode::IndexingRoots => ScanScopeCodeDto::IndexingRoots,
    }
}

/// 把 core 的扫描生命周期映射为 DTO 的扫描状态。
fn to_dto_scan_state(state: ScanLifecycle) -> ScanStateDto {
    match state {
        ScanLifecycle::Idle => ScanStateDto::Idle,
        ScanLifecycle::Running => ScanStateDto::Running,
        ScanLifecycle::Completed => ScanStateDto::Completed,
        ScanLifecycle::Cancelled => ScanStateDto::Cancelled,
        ScanLifecycle::Failed => ScanStateDto::Failed,
    }
}

/// 结合生命周期与取消请求状态，得出对应的稳定本地化消息代码。
fn to_dto_message_code(state: ScanLifecycle, is_cancel_requested: bool) -> UiMessageCodeDto {
    match (state, is_cancel_requested) {
        (ScanLifecycle::Running, true) => UiMessageCodeDto::ScanCancelling,
        (ScanLifecycle::Idle, _) => UiMessageCodeDto::ScanIdle,
        (ScanLifecycle::Running, false) => UiMessageCodeDto::ScanRunning,
        (ScanLifecycle::Completed, _) => UiMessageCodeDto::ScanCompleted,
        (ScanLifecycle::Cancelled, _) => UiMessageCodeDto::ScanCancelled,
        (ScanLifecycle::Failed, _) => UiMessageCodeDto::ScanFailed,
    }
}

/// 把 core 的完整扫描状态映射为 DTO 扫描状态。
fn to_dto_scan_status(status: CoreScanStatus) -> ScanStatusDto {
    let can_cancel = status.can_cancel();
    ScanStatusDto {
        scan_id: status.scan_id,
        kind: match status.kind {
            loki_metis_core::ScanKind::Quick => ScanKindDto::Quick,
            loki_metis_core::ScanKind::FullDevice => ScanKindDto::FullDevice,
        },
        state: to_dto_scan_state(status.state),
        progress_basis_points: status.progress_basis_points,
        current_scope_label: status.current_scope_label,
        current_scope_code: to_dto_scope_code(status.current_scope_code),
        scope_progress: ScanScopeProgressDto {
            current_root_id: status.scope_progress.current_root_id,
            directories_scanned: status.scope_progress.directories_scanned,
            roots_discovered: status.scope_progress.roots_discovered,
            roots_completed: status.scope_progress.roots_completed,
            roots_total: status.scope_progress.roots_total,
        },
        can_cancel,
        files_visited: status.files_visited,
        calls_indexed: status.calls_indexed,
        started_at_epoch_ms: status.started_at_epoch_ms,
        finished_at_epoch_ms: status.finished_at_epoch_ms,
        message: status.message,
        message_code: to_dto_message_code(status.state, status.is_cancel_requested),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 为 adapter 状态测试创建与真实 worker 相同类型的取消令牌。
    fn test_cancellation() -> ScanCancellation {
        ScanCancellation::new()
    }

    /// 验证协调器拒绝第二个 writer，并在首个任务完成后允许下一次扫描。
    #[tokio::test]
    async fn allows_only_one_scan_writer() {
        let coordinator = ScanCoordinator::default();
        let lease = coordinator
            .start(ScanKindDto::Quick, 1, test_cancellation())
            .expect("first scan starts");

        assert!(matches!(
            coordinator.start(ScanKindDto::FullDevice, 2, test_cancellation()),
            Err(ScanStateError::AlreadyRunning)
        ));

        assert!(coordinator.finish_completed(&lease.scan_id, 3));
        coordinator
            .start(ScanKindDto::FullDevice, 4, test_cancellation())
            .expect("completed scan releases writer ownership");
    }

    /// 验证取消请求会设置取消语义，同时保留运行态等待 worker 确认。
    #[tokio::test]
    async fn signals_cancellation_without_claiming_early_completion() {
        let coordinator = ScanCoordinator::default();
        let lease = coordinator
            .start(ScanKindDto::Quick, 1, test_cancellation())
            .expect("scan starts");

        let status = coordinator
            .request_cancel()
            .expect("running scan can be cancelled");

        assert_eq!(lease.scan_id, "scan-1");
        assert_eq!(status.state, ScanStateDto::Running);
        assert_eq!(status.message_code, UiMessageCodeDto::ScanCancelling);
        assert!(!status.can_cancel);

        assert!(coordinator.finish_cancelled(&lease.scan_id, 2));
        assert_eq!(coordinator.snapshot().state, ScanStateDto::Cancelled);
    }

    /// 验证并发发布乱序时不会让用户可见进度或阶段标签倒退。
    #[tokio::test]
    async fn ignores_out_of_order_progress_updates() {
        let coordinator = ScanCoordinator::default();
        let lease = coordinator
            .start(ScanKindDto::FullDevice, 1, test_cancellation())
            .expect("scan starts");
        assert!(coordinator.update_progress(
            &lease.scan_id,
            4,
            7,
            6_250,
            ScanScopeCodeDto::IndexingRoots,
            Some(ScanScopeProgressDto {
                current_root_id: Some("root-current".to_owned()),
                roots_completed: 4,
                roots_total: 7,
                ..ScanScopeProgressDto::default()
            }),
            "已完成 4 / 7 个数据根".to_owned(),
        ));
        assert!(!coordinator.update_progress(
            &lease.scan_id,
            0,
            0,
            100,
            ScanScopeCodeDto::DiscoveringVolumes,
            Some(ScanScopeProgressDto {
                directories_scanned: 1,
                ..ScanScopeProgressDto::default()
            }),
            "迟到的发现进度".to_owned(),
        ));

        let status = coordinator.snapshot();
        assert_eq!(status.progress_basis_points, 6_250);
        assert_eq!(status.files_visited, 4);
        assert_eq!(status.calls_indexed, 7);
        assert_eq!(status.current_scope_label, "已完成 4 / 7 个数据根");
        assert_eq!(status.current_scope_code, ScanScopeCodeDto::IndexingRoots);
        assert_eq!(status.scope_progress.roots_completed, 4);
        assert_eq!(
            status.scope_progress.current_root_id.as_deref(),
            Some("root-current")
        );
    }

    /// 验证失败状态保留调用方提供的客户端归属，且不回退到固定 Codex 文案。
    #[tokio::test]
    async fn keeps_client_specific_sanitized_failure_message() {
        let coordinator = ScanCoordinator::default();
        let lease = coordinator
            .start(ScanKindDto::Quick, 1, test_cancellation())
            .expect("scan starts");
        assert!(coordinator.finish_failed(
            &lease.scan_id,
            2,
            &loki_metis_core::local_scan_client_failure_message(
                "Claude Code",
                loki_metis_core::local_scan_task_error_message(),
            ),
        ));

        let status = coordinator.snapshot();
        assert_eq!(status.state, ScanStateDto::Failed);
        assert!(status.message.starts_with("Claude Code 扫描失败"));
        assert!(!status.message.contains("Codex 原始文件"));
    }

    /// 验证 adapter 传入旧 scan_id 时不会改写随后一轮的 DTO 快照。
    #[tokio::test]
    async fn rejects_late_progress_from_previous_scan_id() {
        let coordinator = ScanCoordinator::default();
        let first = coordinator
            .start(ScanKindDto::Quick, 1, test_cancellation())
            .expect("first scan starts");
        assert!(coordinator.finish_completed(&first.scan_id, 2));
        let second = coordinator
            .start(ScanKindDto::FullDevice, 1, test_cancellation())
            .expect("next scan starts");

        assert!(!coordinator.update_progress(
            &first.scan_id,
            12,
            34,
            8_000,
            ScanScopeCodeDto::IndexingRoots,
            None,
            "旧扫描迟到".to_owned(),
        ));

        let status = coordinator.snapshot();
        assert_eq!(status.scan_id.as_deref(), Some(second.scan_id.as_str()));
        assert_eq!(status.state, ScanStateDto::Running);
        assert_eq!(status.progress_basis_points, 0);
        assert_eq!(status.files_visited, 0);
        assert_eq!(status.calls_indexed, 0);
    }
}
