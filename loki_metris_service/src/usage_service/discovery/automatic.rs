//! 自动发现保留私有 worker 与公开状态之间的一次性交付门。
use std::{sync::{Arc,Mutex},time::{Duration,Instant},panic::{AssertUnwindSafe,catch_unwind}};
use loki_metris_core::*;
use loki_metris_host::local_index::metadata_discovery::{MetadataDiscoverySummary,discover_metadata_roots_with_callback};
/// 自动发现从命令接受任务起共享的总墙钟预算，包含 UserPriority 的显式根前置核对。
pub(super) const ROOT_DISCOVERY_TOTAL_TIMEOUT: Duration = Duration::from_secs(60);
/// 监督线程复核取消、deadline 与私有进度的短轮询间隔。
const ROOT_DISCOVERY_BOUNDARY_POLL: Duration = Duration::from_millis(10);
/// 自动发现超过共同 deadline 时公开的稳定脱敏错误码。
const ROOT_DISCOVERY_DEADLINE_ERROR: &str = "metadata_discovery_deadline_exceeded";
/// 自动发现 worker panic 或提前失效时公开的稳定脱敏错误码。
const ROOT_DISCOVERY_WORKER_ERROR: &str = "metadata_discovery_worker_unavailable";

/// 把一次后台发现的私有协调器与当前共享状态绑定；关闭后所有迟到写入都被拒绝。
#[derive(Clone)]
pub(super) struct RootDiscoveryRunControl {
    /// 页面读取的共享协调器。
    shared: Arc<loki_metris_core::RootDiscoveryCoordinator>,
    /// 本次后台发现私有的协调器。
    worker: Arc<loki_metris_core::RootDiscoveryCoordinator>,
    /// 是否仍接受 worker 写回共享状态；关闭后为 false。
    accepting: Arc<Mutex<bool>>,
}

impl RootDiscoveryRunControl {
    /// 建立仍可交付结果的一次性运行门。
    pub(super) fn new(
        shared: Arc<loki_metris_core::RootDiscoveryCoordinator>,
        worker: Arc<loki_metris_core::RootDiscoveryCoordinator>,
    ) -> Self {
        Self {
            shared,
            worker,
            accepting: Arc::new(Mutex::new(true)),
        }
    }

    /// 同步观察取消或 deadline；返回 false 后本次任务永远不能再写共享状态。
    fn poll(&self, deadline: Instant) -> bool {
        let mut accepting = self.lock_accepting();
        self.check_boundary(&mut accepting, Instant::now(), deadline)
    }

    /// 在线性化门内提交候选与进度，避免 deadline 与“最后一次写入”竞态。
    fn deliver_candidate(&self, candidate: RootCandidate, deadline: Instant) -> bool {
        let mut accepting = self.lock_accepting();
        if !self.check_boundary(&mut accepting, Instant::now(), deadline) {
            return false;
        }
        self.shared.submit_candidate(candidate)
    }

    /// 在正常 worker 终态处关闭门并把私有状态一次性归约到共享协调器。
    fn complete(&self, result: Result<MetadataDiscoverySummary, ()>, deadline: Instant) -> bool {
        let mut accepting = self.lock_accepting();
        if !self.check_boundary(&mut accepting, Instant::now(), deadline) {
            return false;
        }
        *accepting = false;
        let worker_status = self.worker.snapshot();
        match result {
            Err(()) => self.shared.fail(ROOT_DISCOVERY_WORKER_ERROR),
            Ok(_) if worker_status.lifecycle == RootDiscoveryLifecycle::Failed => self.shared.fail(
                worker_status
                    .error_code
                    .as_deref()
                    .unwrap_or(ROOT_DISCOVERY_WORKER_ERROR),
            ),
            Ok(summary) => {
                if worker_status.lifecycle == RootDiscoveryLifecycle::Cancelled {
                    let _ = self.shared.request_cancel();
                }
                self.shared
                    .finish(summary.system_index_available, summary.fallback_performed);
            }
        }
        true
    }

    /// worker 未能启动或 panic 时关闭交付门，不让后续调度保留 Running。
    pub(super) fn fail_worker(&self, deadline: Instant) {
        let mut accepting = self.lock_accepting();
        if self.check_boundary(&mut accepting, Instant::now(), deadline) {
            *accepting = false;
            let _ = self.worker.request_cancel();
            self.shared.fail(ROOT_DISCOVERY_WORKER_ERROR);
        }
    }

    /// owner 关闭时同步终结公开状态，并持续向可能已经启动的私有 worker 发出取消。
    pub(super) fn cancel(&self) {
        let mut accepting = self.lock_accepting();
        let _ = self.worker.request_cancel();
        if !*accepting {
            return;
        }
        *accepting = false;
        let _ = self.shared.request_cancel();
        self.shared.finish(false, false);
    }

    /// 在同一锁内决定本次运行能否继续，确保关闭后没有迟到状态写入窗口。
    fn check_boundary(&self, accepting: &mut bool, now: Instant, deadline: Instant) -> bool {
        if !*accepting {
            let _ = self.worker.request_cancel();
            return false;
        }
        if self.shared.is_cancel_requested() {
            *accepting = false;
            let _ = self.worker.request_cancel();
            self.shared.finish(false, false);
            return false;
        }
        if now >= deadline {
            *accepting = false;
            let _ = self.worker.request_cancel();
            self.shared.fail(ROOT_DISCOVERY_DEADLINE_ERROR);
            return false;
        }
        self.sync_worker_progress();
        true
    }

    /// 只在交付门内把私有 worker 的无路径进度同步到共享协调器。
    fn sync_worker_progress(&self) {
        let status = self.worker.snapshot();
        if status.fallback_performed {
            self.shared.begin_fallback(status.system_index_available);
        }
        self.shared.update_progress(status.progress);
    }

    /// 取得运行门；测试 panic 污染后仍保持超时关闭能力。
    fn lock_accepting(&self) -> std::sync::MutexGuard<'_, bool> {
        self.accepting
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// 独立于 blocking 线程池调度监督绝对 deadline，队列饱和时仍能收敛公开状态。
pub(super) async fn supervise_root_discovery_boundaries<ShutdownRequested>(
    control: RootDiscoveryRunControl,
    deadline: Instant,
    shutdown_requested: ShutdownRequested,
) where
    ShutdownRequested: Fn() -> bool,
{
    loop {
        if shutdown_requested() {
            control.cancel();
            return;
        }
        if !control.poll(deadline) {
            return;
        }
        tokio::time::sleep(
            deadline
                .saturating_duration_since(Instant::now())
                .min(ROOT_DISCOVERY_BOUNDARY_POLL),
        )
        .await;
    }
}

/// 在现有 BackgroundTaskOwner 持有的 blocking 岛内执行私有发现。
pub(super) fn run_owned_root_discovery(
    control: RootDiscoveryRunControl,
    discovery_scope: RootDiscoveryScope,
    deadline: Instant,
) {
    if !control.poll(deadline) {
        return;
    }
    let callback_control = control.clone();
    let result = catch_unwind(AssertUnwindSafe(|| {
        discover_metadata_roots_with_callback(&control.worker, discovery_scope, move |candidate| {
            callback_control.deliver_candidate(candidate, deadline);
        })
    }))
    .map_err(|_| ());
    if let Ok(summary) = &result {
        tracing::info!(
            system_index_available = summary.system_index_available,
            fallback_performed = summary.fallback_performed,
            "root discovery finished"
        );
    }
    let _ = control.complete(result, deadline);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造共享/私有协调器都已启动的一次 UserPriority 运行。
    fn running_control() -> RootDiscoveryRunControl {
        let shared = Arc::new(loki_metris_core::RootDiscoveryCoordinator::default());
        let worker = Arc::new(loki_metris_core::RootDiscoveryCoordinator::default());
        assert!(shared.start(
            RootDiscoveryStrategy::MetadataTraversal,
            RootDiscoveryPlatform::Other,
            RootDiscoveryScope::UserPriority,
            1,
        ));
        assert!(worker.start(
            RootDiscoveryStrategy::MetadataTraversal,
            RootDiscoveryPlatform::Other,
            RootDiscoveryScope::UserPriority,
            1,
        ));
        RootDiscoveryRunControl::new(shared, worker)
    }

    /// blocking worker 尚未获得调度时，独立监督器仍必须让总 deadline 生效。
    #[tokio::test]
    async fn queued_worker_deadline_including_user_priority_prelude_rejects_late_writes() {
        let control = running_control();
        let deadline = Instant::now();

        supervise_root_discovery_boundaries(control.clone(), deadline, || false).await;
        assert!(control.worker.is_cancel_requested());
        let timed_out = control.shared.snapshot();
        assert_eq!(timed_out.lifecycle, RootDiscoveryLifecycle::Failed);
        assert_eq!(
            timed_out.error_code.as_deref(),
            Some(ROOT_DISCOVERY_DEADLINE_ERROR)
        );

        let candidate = RootCandidate {
            id: "late-candidate".to_owned(),
            client: SourceClientKind::Codex,
            absolute_path: "/late".to_owned(),
            strategy: RootDiscoveryStrategy::MetadataTraversal,
            evidence: RootCandidateEvidence::CodexRollout,
        };
        assert!(!control.deliver_candidate(candidate, deadline));
        control
            .worker
            .update_progress(loki_metris_core::RootDiscoveryProgress {
                directories_checked: 99,
                ..loki_metris_core::RootDiscoveryProgress::default()
            });
        assert!(!control.complete(
            Ok(MetadataDiscoverySummary {
                system_index_available: true,
                fallback_performed: true,
            }),
            deadline,
        ));

        assert!(control.shared.candidates().is_empty());
        let after_late_completion = control.shared.snapshot();
        assert_eq!(after_late_completion, timed_out);
    }

    /// gate 等待跨过 deadline 时必须在获锁后重新采样，迟到进度不得提交。
    #[test]
    fn root_discovery_rechecks_deadline_after_waiting_for_delivery_gate() {
        let control = running_control();
        let polling_control = control.clone();
        let held_gate = control.lock_accepting();
        let deadline = Instant::now() + Duration::from_millis(1);
        let polling = std::thread::spawn(move || polling_control.poll(deadline));

        while Instant::now() < deadline {
            std::thread::yield_now();
        }
        drop(held_gate);

        assert!(
            !polling
                .join()
                .expect("deadline poll reaches terminal state")
        );
        let status = control.shared.snapshot();
        assert_eq!(status.lifecycle, RootDiscoveryLifecycle::Failed);
        assert_eq!(
            status.error_code.as_deref(),
            Some(ROOT_DISCOVERY_DEADLINE_ERROR)
        );
    }

    /// 用户取消与 owner 关闭使用同一单向门，迟到 worker 不能把 Cancelled 改回成功。
    #[test]
    fn cancellation_closes_delivery_gate_before_worker_completion() {
        let control = running_control();
        assert!(control.shared.request_cancel());
        let deadline = Instant::now() + Duration::from_secs(1);

        assert!(!control.poll(deadline));
        assert_eq!(
            control.shared.snapshot().lifecycle,
            RootDiscoveryLifecycle::Cancelled
        );
        assert!(!control.complete(Ok(MetadataDiscoverySummary::default()), deadline,));
        assert_eq!(
            control.shared.snapshot().lifecycle,
            RootDiscoveryLifecycle::Cancelled
        );
    }

    /// 正常完成仍应提交候选并保留私有 worker 的真实进度与阶段。
    #[test]
    fn successful_private_worker_state_is_committed_once() {
        let control = running_control();
        let now = Instant::now();
        let deadline = now + Duration::from_secs(1);
        control
            .worker
            .update_progress(loki_metris_core::RootDiscoveryProgress {
                volumes_completed: 1,
                volumes_total: 1,
                directories_checked: 2,
                ..loki_metris_core::RootDiscoveryProgress::default()
            });
        let candidate = RootCandidate {
            id: "candidate".to_owned(),
            client: SourceClientKind::Codex,
            absolute_path: "/candidate".to_owned(),
            strategy: RootDiscoveryStrategy::MetadataTraversal,
            evidence: RootCandidateEvidence::CodexRollout,
        };

        assert!(control.deliver_candidate(candidate, deadline));
        control.worker.finish(false, true);
        assert!(control.complete(
            Ok(MetadataDiscoverySummary {
                system_index_available: false,
                fallback_performed: true,
            }),
            deadline,
        ));

        let status = control.shared.snapshot();
        assert_eq!(status.lifecycle, RootDiscoveryLifecycle::Complete);
        assert_eq!(status.progress.directories_checked, 2);
        assert_eq!(status.progress.candidates_found, 1);
    }
}
