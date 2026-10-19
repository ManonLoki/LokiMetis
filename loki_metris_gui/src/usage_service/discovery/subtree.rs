//! 手动所选子树保留独立 deadline 与取消交付门，不占用索引 writer。
use std::{panic::{AssertUnwindSafe,catch_unwind},path::{Path,PathBuf},sync::{Arc,Mutex,atomic::{AtomicBool,Ordering}},time::{Duration,Instant}};
use loki_metris_core::*;
use crate::backend::local_index::{CancellationToken,ClaudeDiscoveredRoot,DiscoveryProgress,FullDiscoveryOptions,discover_claude_full_device_with_progress,discover_full_device_with_progress};
/// 只保存手动添加流程的预约状态，不用锁跨越原生目录选择或用户等待。
static MANUAL_ADD_BUSY: AtomicBool = AtomicBool::new(false);

/// 选择器等待期间也保留唯一发现入口，避免第二个流程夺走协调器。
pub(super) fn manual_add_busy()->bool {MANUAL_ADD_BUSY.load(Ordering::Acquire)}

/// 手动子树深搜从后台任务预约起计算的固定总墙钟预算。
pub(super) const MANUAL_SUBTREE_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(60);
/// deadline 与用户取消监督的短轮询间隔。
const MANUAL_SUBTREE_BOUNDARY_POLL: Duration = Duration::from_millis(10);
/// 手动子树深搜最多检查的目录数。
const MANUAL_SUBTREE_DIRECTORY_LIMIT: u64 = 100_000;
/// 手动子树深搜最多读取的目录项数。
const MANUAL_SUBTREE_ENTRY_LIMIT: u64 = 1_000_000;
/// 手动子树深搜只使用一个有限预算批次。
const MANUAL_SUBTREE_BATCH_LIMIT: u64 = 1;
/// 手动子树候选签名在整个任务中最多递归检查的目录数。
const MANUAL_SUBTREE_SIGNATURE_DIRECTORY_LIMIT: u64 = 8_192;
/// 手动子树候选签名在整个任务中最多处理的目录项数。
const MANUAL_SUBTREE_SIGNATURE_ENTRY_LIMIT: u64 = 100_000;
/// 手动子树候选签名在整个任务中最多打开的文件数。
const MANUAL_SUBTREE_SIGNATURE_FILE_LIMIT: u64 = 256;
/// 手动子树候选签名在整个任务中最多读取的前缀字节数。
const MANUAL_SUBTREE_SIGNATURE_BYTE_LIMIT: u64 = 16 * 1024 * 1024;
/// 手动子树超过固定 deadline 时公开的稳定脱敏错误码。
const MANUAL_SUBTREE_DEADLINE_ERROR: &str = "manual_subtree_discovery_deadline_exceeded";
/// 手动子树 worker panic 或提前失效时公开的稳定脱敏错误码。
const MANUAL_SUBTREE_WORKER_ERROR: &str = "manual_subtree_discovery_worker_unavailable";
/// 有限预算或访问缺口导致无法证明完整覆盖时公开的稳定错误码。
const MANUAL_SUBTREE_COVERAGE_ERROR: &str = "manual_subtree_discovery_coverage_incomplete";

/// 线性化手动子树的 deadline、取消与候选交付，关闭后迟到 worker 只能本地收敛。
#[derive(Clone)]
pub(super) struct ManualSubtreeRunControl {
    /// 共享的发现协调器。
    coordinator: Arc<RootDiscoveryCoordinator>,
    /// 本次手动子树扫描的取消句柄。
    cancellation: CancellationToken,
    /// 候选交付闸门。
    delivery: Arc<Mutex<ManualSubtreeDeliveryGate>>,
}

/// 为锁外事件交付保留当前世代；关闭即递增世代并阻止当前批次继续。
#[derive(Debug)]
struct ManualSubtreeDeliveryGate {
    /// 是否仍接受交付；关闭后为 false。
    accepting: bool,
    /// 交付代次；关闭时递增，使锁外的在途交付失效。
    generation: u64,
}

/// 保存一个已写入共享状态、可在锁外发送事件的候选。
struct ManualCandidateDelivery {
    /// 已写入共享状态的候选。
    candidate: RootCandidate,
    /// 写入时的交付代次。
    generation: u64,
    /// 本次是新增候选而非更新已有候选。
    inserted: bool,
}

impl ManualSubtreeRunControl {
    /// 为已经进入 Running 的手动任务建立一次性交付门。
    pub(super) fn new(coordinator: Arc<RootDiscoveryCoordinator>, cancellation: CancellationToken) -> Self {
        Self {
            coordinator,
            cancellation,
            delivery: Arc::new(Mutex::new(ManualSubtreeDeliveryGate {
                accepting: true,
                generation: 0,
            })),
        }
    }

    /// 复核 deadline 与取消；返回 false 后所有后续写入永久失效。
    fn poll(&self, deadline: Instant) -> bool {
        let mut gate = self.lock_delivery();
        self.check_boundary(&mut gate, Instant::now(), deadline)
    }

    /// 用锁内注入时刻复用同一边界裁决，仅供确定性并发回归。
    #[cfg(test)]
    fn poll_at(&self, now: Instant, deadline: Instant) -> bool {
        let mut gate = self.lock_delivery();
        self.check_boundary(&mut gate, now, deadline)
    }

    /// 在交付门内更新无路径进度，确保 deadline 后的回调不能覆盖终态。
    fn publish_progress(&self, progress: DiscoveryProgress, deadline: Instant) -> bool {
        let mut gate = self.lock_delivery();
        if !self.check_boundary(&mut gate, Instant::now(), deadline) {
            return false;
        }
        self.coordinator.update_progress(RootDiscoveryProgress {
            volumes_completed: 0,
            volumes_total: 1,
            directories_checked: progress.directories_scanned,
            file_names_checked: 0,
            candidates_found: progress.roots_discovered,
            permission_denied: 0,
            io_errors: 0,
            skipped: 0,
        });
        true
    }

    /// 在锁内核对 deadline 并先提交候选，使锁外 emit 永远引用可查询后端事实。
    fn prepare_candidate_delivery(
        &self,
        client: SourceClientKind,
        evidence: RootCandidateEvidence,
        path: &Path,
        deadline: Instant,
    ) -> Option<ManualCandidateDelivery> {
        let mut gate = self.lock_delivery();
        if !self.check_boundary(&mut gate, Instant::now(), deadline) {
            return None;
        }
        let candidate = RootCandidate {
            id: stable_id(client.candidate_namespace(), &path_key(path)),
            client,
            absolute_path: path.to_string_lossy().into_owned(),
            strategy: RootDiscoveryStrategy::MetadataTraversal,
            evidence,
        };
        let inserted = self.coordinator.submit_candidate(candidate.clone());
        Some(ManualCandidateDelivery {
            candidate,
            generation: gate.generation,
            inserted,
        })
    }

    /// emit 返回后重新核对世代与 deadline，关闭期间不再开始下一次交付。
    fn finish_candidate_delivery(&self, generation: u64, deadline: Instant) -> bool {
        let mut gate = self.lock_delivery();
        if !gate.accepting || gate.generation != generation {
            self.cancellation.cancel();
            return false;
        }
        self.check_boundary(&mut gate, Instant::now(), deadline)
    }

    /// 正常 worker 终态只允许提交一次；覆盖不完整不得伪称完整。
    fn complete(&self, coverage: &CoverageReport, deadline: Instant) -> bool {
        let mut gate = self.lock_delivery();
        if !self.check_boundary(&mut gate, Instant::now(), deadline) {
            return false;
        }
        Self::close_gate(&mut gate);
        let mut progress = self.coordinator.snapshot().progress;
        progress.permission_denied = coverage.permission_denied_count;
        progress.skipped = coverage.skipped_count;
        self.coordinator.update_progress(progress);
        match coverage.state {
            CoverageState::Complete => finish_manual_subtree_discovery(&self.coordinator, false),
            CoverageState::Cancelled => finish_manual_subtree_discovery(&self.coordinator, true),
            CoverageState::Partial => self.coordinator.fail(MANUAL_SUBTREE_COVERAGE_ERROR),
            CoverageState::Failed => self.coordinator.fail(MANUAL_SUBTREE_WORKER_ERROR),
        }
        true
    }

    /// 捕获 worker panic，不让协调器永久停留在 Running。
    pub(super) fn fail_worker(&self, deadline: Instant) {
        let mut gate = self.lock_delivery();
        if self.check_boundary(&mut gate, Instant::now(), deadline) {
            Self::close_gate(&mut gate);
            self.cancellation.cancel();
            self.coordinator.fail(MANUAL_SUBTREE_WORKER_ERROR);
        }
    }

    /// owner 关闭与用户取消共享同一不可逆终态，并向阻塞岛持续发布取消 token。
    pub(super) fn cancel(&self) {
        let mut gate = self.lock_delivery();
        self.cancellation.cancel();
        if !gate.accepting {
            return;
        }
        Self::close_gate(&mut gate);
        let _ = self.coordinator.request_cancel();
        finish_manual_subtree_discovery(&self.coordinator, true);
    }

    /// 在同一锁内裁决当前调用是否还可写共享状态。
    fn check_boundary(
        &self,
        gate: &mut ManualSubtreeDeliveryGate,
        now: Instant,
        deadline: Instant,
    ) -> bool {
        if !gate.accepting {
            self.cancellation.cancel();
            return false;
        }
        if self.coordinator.is_cancel_requested() || self.cancellation.is_cancelled() {
            Self::close_gate(gate);
            self.cancellation.cancel();
            let _ = self.coordinator.request_cancel();
            finish_manual_subtree_discovery(&self.coordinator, true);
            return false;
        }
        if now >= deadline {
            Self::close_gate(gate);
            self.cancellation.cancel();
            let _ = self.coordinator.request_cancel();
            self.coordinator.fail(MANUAL_SUBTREE_DEADLINE_ERROR);
            return false;
        }
        true
    }

    /// 不可逆关闭交付门并使已发出的世代预留全部失效。
    fn close_gate(gate: &mut ManualSubtreeDeliveryGate) {
        gate.accepting = false;
        gate.generation = gate.generation.saturating_add(1);
    }

    /// 取得交付门；测试 panic 污染后仍保留 deadline 关闭能力。
    fn lock_delivery(&self) -> std::sync::MutexGuard<'_, ManualSubtreeDeliveryGate> {
        self.delivery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// 独立于 blocking 线程池调度监督手动深搜，并在所有 emit 确认完成前保持存活。
pub(super) async fn supervise_manual_subtree_boundaries<ShutdownRequested>(
    control: ManualSubtreeRunControl,
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
                .min(MANUAL_SUBTREE_BOUNDARY_POLL),
        )
        .await;
    }
}

/// 手动添加流程的短临界区预约；所有返回、错误和 panic 展开路径都会自动释放。
pub(super) struct ManualAddReservation;

impl ManualAddReservation {
    /// 原子预约唯一手动添加流程，已有流程时立即失败而不等待。
    pub(super) fn acquire() -> Result<Self, &'static str> {
        MANUAL_ADD_BUSY
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| "manual add is already in progress")
    }
}

impl Drop for ManualAddReservation {
    /// 释放预约，使目录选择取消和所有错误路径都不会永久占用入口。
    fn drop(&mut self) {
        MANUAL_ADD_BUSY.store(false, Ordering::Release);
    }
}

/// 阻塞岛内执行子树发现，逐步上报进度并把命中候选实时推送给界面。
pub(super) fn run_manual_subtree_discovery(
    control: ManualSubtreeRunControl,
    source_client: SourceClientKind,
    selected_path: PathBuf,
    deadline: Instant,
) {
    let options = manual_subtree_discovery_options(selected_path);
    if !control.poll(deadline) {
        return;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let (candidates, coverage) = match source_client {
            SourceClientKind::Codex => {
                let progress_control = control.clone();
                let result = discover_full_device_with_progress(
                    &options,
                    &control.cancellation,
                    move |progress| {
                        let _ = progress_control.publish_progress(progress, deadline);
                    },
                );
                (
                    result
                        .roots
                        .into_iter()
                        .map(|root| (root.path, RootCandidateEvidence::CodexRollout))
                        .collect(),
                    result.coverage,
                )
            }
            SourceClientKind::ClaudeCode => {
                let progress_control = control.clone();
                let result = discover_claude_full_device_with_progress(
                    &options,
                    &control.cancellation,
                    move |progress| {
                        let _ = progress_control.publish_progress(progress, deadline);
                    },
                );
                (claude_manual_candidates(result.roots), result.coverage)
            }
            SourceClientKind::GrokBuildCli => {
                let progress_control = control.clone();
                let result = crate::backend::local_index::discover_grok_full_device_with_progress(
                    &options,
                    &control.cancellation,
                    move |progress| {
                        let _ = progress_control.publish_progress(progress, deadline);
                    },
                );
                (
                    result
                        .roots
                        .into_iter()
                        .map(|root| (root.path, root.evidence))
                        .collect(),
                    result.coverage,
                )
            }
            SourceClientKind::WorkBuddy => {
                unreachable!("手动添加入口只接受 AgentClientKindDto 的三个批准客户端")
            }
        };
        deliver_discovered_candidates(&control, source_client, candidates, deadline, |_| {});
        coverage
    }));
    match result {
        Ok(coverage) => {
            let _ = control.complete(&coverage, deadline);
        }
        Err(_) => control.fail_worker(deadline),
    }
}

/// 为手动深搜建立固定、有限且不随所选目录规模增长的遍历与签名预算。
fn manual_subtree_discovery_options(selected_path: PathBuf) -> FullDiscoveryOptions {
    FullDiscoveryOptions {
        search_roots: vec![selected_path],
        max_directories: MANUAL_SUBTREE_DIRECTORY_LIMIT,
        max_entries: MANUAL_SUBTREE_ENTRY_LIMIT,
        max_traversal_batches: MANUAL_SUBTREE_BATCH_LIMIT,
        max_signature_directories: MANUAL_SUBTREE_SIGNATURE_DIRECTORY_LIMIT,
        max_signature_entries: MANUAL_SUBTREE_SIGNATURE_ENTRY_LIMIT,
        max_signature_files: MANUAL_SUBTREE_SIGNATURE_FILE_LIMIT,
        max_signature_bytes: MANUAL_SUBTREE_SIGNATURE_BYTE_LIMIT,
        ..FullDiscoveryOptions::default()
    }
}

/// 保留 Claude 严格签名实际命中的主 transcript / subagent 证据。
fn claude_manual_candidates(
    roots: Vec<ClaudeDiscoveredRoot>,
) -> Vec<(PathBuf, RootCandidateEvidence)> {
    roots
        .into_iter()
        .map(|root| (root.path, root.evidence))
        .collect()
}

/// 逐项交付已发现候选；候选先成为后端事实，再在锁外发送事件。
fn deliver_discovered_candidates(
    control: &ManualSubtreeRunControl,
    client: SourceClientKind,
    candidates: impl IntoIterator<Item = (PathBuf, RootCandidateEvidence)>,
    deadline: Instant,
    mut on_candidate: impl FnMut(RootCandidate),
) {
    for (path, evidence) in candidates {
        let Some(delivery) = control.prepare_candidate_delivery(client, evidence, &path, deadline)
        else {
            break;
        };
        if delivery.inserted {
            on_candidate(delivery.candidate);
        }
        if !control.finish_candidate_delivery(delivery.generation, deadline) {
            break;
        }
    }
}

/// 用取消感知的卷完成计数结束手动子树任务，避免取消状态伪称覆盖完整。
fn finish_manual_subtree_discovery(
    coordinator: &RootDiscoveryCoordinator,
    shutdown_requested: bool,
) {
    if coordinator.snapshot().lifecycle != RootDiscoveryLifecycle::Running {
        return;
    }
    if shutdown_requested {
        let _ = coordinator.request_cancel();
    }
    let cancelled = coordinator.is_cancel_requested();
    let final_snapshot = coordinator.snapshot();
    coordinator.update_progress(RootDiscoveryProgress {
        volumes_completed: u64::from(!cancelled),
        volumes_total: 1,
        directories_checked: final_snapshot.progress.directories_checked,
        file_names_checked: 0,
        candidates_found: coordinator.candidates().len() as u64,
        permission_denied: final_snapshot.progress.permission_denied,
        io_errors: 0,
        skipped: final_snapshot.progress.skipped,
    });
    coordinator.finish(false, true);
}

#[cfg(test)]
#[path="subtree_tests.rs"]
mod tests;
#[cfg(test)]
#[path="subtree_deadline_tests.rs"]
mod deadline_tests;
