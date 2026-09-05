//! 单一协调器：按确定性 order 提交 worker 结果并维护共享预算。

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::path::PathBuf;

use loki_metis_core::{CoverageReport, CoverageState};

use super::super::inspection::{
    RootInspection, SignatureProbeContext, inspect_root, is_forbidden_auxiliary_root,
    safe_path_alias,
};
use super::super::{DiscoveredRoot, DiscoveryProgress, DiscoveryResult, FullDiscoveryOptions};
use super::types::{
    DirectoryCursor, EnumerationOutcome, PendingTask, PreflightOutcome, TraversalBudget,
    TraversalItem, WorkerOutcome, WorkerResult, WorkerTask,
};
use super::worker::{crosses_another_search_root, is_excluded, should_publish_progress};
use super::{DIRECTORY_ENTRY_CHUNK, DISCOVERY_ROUND_SIZE};
use crate::backend::local_index::{CancellationToken, DiscoveryMethod, is_obviously_network_path};

/// 单线程提交 worker 结果，唯一持有根集合、共享预算、签名上下文和进度回调。
pub(super) struct DiscoveryCoordinator<'a, F>
where
    F: FnMut(DiscoveryProgress),
{
    /// 调用方批准的卷、排除范围和硬预算。
    pub(super) options: &'a FullDiscoveryOptions,
    /// 全部 worker 与签名探测共享的取消信号。
    pub(super) cancellation: &'a CancellationToken,
    /// 只由协调器调用的脱敏进度回调。
    pub(super) on_progress: F,
    /// 尚未派发的确定性工作队列。
    pub(super) pending: VecDeque<PendingTask>,
    /// 按规范化路径稳定去重和排序的根结果。
    pub(super) roots: BTreeMap<PathBuf, DiscoveredRoot>,
    /// 已经通过预检并计入扫描的规范化目录。
    pub(super) visited: HashSet<PathBuf>,
    /// 实际开始过的本地卷起点。
    pub(super) traversal_roots_started: BTreeSet<PathBuf>,
    /// 32 批次唯一真实遍历预算。
    pub(super) traversal_budget: TraversalBudget,
    /// 整个任务唯一共享的严格结构签名预算。
    pub(super) signature_probe: SignatureProbeContext<'a>,
    /// 已确认普通目录的累计数量。
    pub(super) directories_scanned: u64,
    /// 权限拒绝累计数量。
    pub(super) permission_denied_count: u64,
    /// 全部保守跳过累计数量。
    pub(super) skipped_count: u64,
    /// 链接或 reparse point 跳过数量。
    pub(super) symlink_skipped_count: u64,
    /// 网络路径跳过数量。
    pub(super) network_skipped_count: u64,
    /// 用户是否已经请求取消。
    pub(super) user_cancelled: bool,
    /// 任一共享硬预算是否已经耗尽。
    pub(super) budget_exhausted: bool,
}

impl<'a, F> DiscoveryCoordinator<'a, F>
where
    F: FnMut(DiscoveryProgress),
{
    /// 建立空结果和按既有栈语义反向排列的卷起点，不执行文件系统访问。
    pub(super) fn new(
        options: &'a FullDiscoveryOptions,
        cancellation: &'a CancellationToken,
        on_progress: F,
    ) -> Self {
        let pending = options
            .search_roots
            .iter()
            .rev()
            .cloned()
            .map(|root| {
                PendingTask::Preflight(TraversalItem {
                    path: root.clone(),
                    policy_path: root.clone(),
                    traversal_root: root,
                    is_traversal_root: true,
                })
            })
            .collect();
        Self {
            options,
            cancellation,
            on_progress,
            pending,
            roots: BTreeMap::new(),
            visited: HashSet::new(),
            traversal_roots_started: BTreeSet::new(),
            traversal_budget: TraversalBudget::new(options),
            signature_probe: SignatureProbeContext::new(options, cancellation),
            directories_scanned: 0,
            permission_denied_count: 0,
            skipped_count: options
                .preflight_network_skipped_count
                .saturating_add(options.preflight_other_skipped_count),
            symlink_skipped_count: 0,
            network_skipped_count: options.preflight_network_skipped_count,
            user_cancelled: false,
            budget_exhausted: false,
        }
    }

    /// 生成最多八个 ticket 的下一轮工作，并用预算克隆证明本轮最坏消费可提交。
    pub(super) fn prepare_round(&mut self) -> Vec<WorkerTask> {
        let mut tasks = Vec::new();
        let mut simulated_budget = self.traversal_budget.clone();
        while tasks.len() < DISCOVERY_ROUND_SIZE {
            if self.cancellation.is_cancelled() {
                self.user_cancelled = true;
                break;
            }
            let Some(next) = self.pending.front() else {
                break;
            };
            let entry_quota = match next {
                PendingTask::Preflight(item) => {
                    if !simulated_budget.ensure_directory_capacity() {
                        break;
                    }
                    if is_excluded(
                        &item.policy_path,
                        &item.traversal_root,
                        &self.options.excluded_roots,
                    ) || crosses_another_search_root(
                        &item.policy_path,
                        &item.traversal_root,
                        &self.options.search_roots,
                    ) {
                        let PendingTask::Preflight(item) = self
                            .pending
                            .pop_front()
                            .expect("front task remains available")
                        else {
                            unreachable!("front task kind is unchanged")
                        };
                        self.traversal_roots_started.insert(item.traversal_root);
                        self.skipped_count = self.skipped_count.saturating_add(1);
                        continue;
                    }
                    if !self.options.allow_network_like_paths
                        && is_obviously_network_path(&item.path)
                    {
                        let PendingTask::Preflight(item) = self
                            .pending
                            .pop_front()
                            .expect("front task remains available")
                        else {
                            unreachable!("front task kind is unchanged")
                        };
                        self.traversal_roots_started.insert(item.traversal_root);
                        self.network_skipped_count = self.network_skipped_count.saturating_add(1);
                        self.skipped_count = self.skipped_count.saturating_add(1);
                        continue;
                    }
                    if !simulated_budget.claim_directory() {
                        break;
                    }
                    0
                }
                PendingTask::Enumerate(_) => {
                    let mut quota = 0;
                    while quota < DIRECTORY_ENTRY_CHUNK && simulated_budget.claim_entry() {
                        quota += 1;
                    }
                    if quota == 0 {
                        break;
                    }
                    quota
                }
            };
            let pending = self
                .pending
                .pop_front()
                .expect("prepared task remains available");
            if let PendingTask::Preflight(item) = &pending {
                self.traversal_roots_started
                    .insert(item.traversal_root.clone());
            }
            tasks.push(WorkerTask {
                order: tasks.len(),
                pending,
                entry_quota,
            });
        }
        if tasks.is_empty()
            && !self.pending.is_empty()
            && !self.user_cancelled
            && !self.cancellation.is_cancelled()
        {
            self.budget_exhausted = true;
            self.skipped_count = self.skipped_count.saturating_add(1);
        }
        tasks
    }

    /// 按 ticket 顺序提交一轮结果；遇到取消或预算耗尽后丢弃尚未提交的后续结果。
    pub(super) fn commit_round(&mut self, mut results: Vec<WorkerResult>) {
        results.sort_by_key(|result| result.order);
        for result in results {
            if self.user_cancelled || self.budget_exhausted {
                break;
            }
            match result.outcome {
                WorkerOutcome::Preflight(outcome) => self.commit_preflight(outcome),
                WorkerOutcome::Enumerated(outcome) => self.commit_enumeration(*outcome),
                WorkerOutcome::Failed => {
                    self.budget_exhausted = true;
                    self.skipped_count = self.skipped_count.saturating_add(1);
                }
            }
        }
    }

    /// 提交一个目录预检并串行执行严格根签名；命中后绝不派发目录枚举。
    fn commit_preflight(&mut self, outcome: PreflightOutcome) {
        let (normalized, traversal_root, policy_path) = match outcome {
            PreflightOutcome::Ready {
                normalized,
                traversal_root,
                policy_path,
            } => (normalized, traversal_root, policy_path),
            PreflightOutcome::Symlink => {
                self.symlink_skipped_count = self.symlink_skipped_count.saturating_add(1);
                self.skipped_count = self.skipped_count.saturating_add(1);
                return;
            }
            PreflightOutcome::PermissionDenied => {
                self.permission_denied_count = self.permission_denied_count.saturating_add(1);
                return;
            }
            PreflightOutcome::Skipped => {
                self.skipped_count = self.skipped_count.saturating_add(1);
                return;
            }
            PreflightOutcome::NotDirectory => return,
            PreflightOutcome::Cancelled => {
                self.user_cancelled = true;
                return;
            }
        };
        if !self.visited.insert(normalized.clone()) {
            return;
        }
        if !self.traversal_budget.claim_directory() {
            self.budget_exhausted = true;
            self.skipped_count = self.skipped_count.saturating_add(1);
            return;
        }
        self.directories_scanned = self.directories_scanned.saturating_add(1);
        if should_publish_progress(self.directories_scanned) {
            self.publish_progress();
        }
        if self.cancellation.is_cancelled() {
            self.user_cancelled = true;
            return;
        }
        if is_forbidden_auxiliary_root(&normalized) {
            self.skipped_count = self.skipped_count.saturating_add(1);
            return;
        }
        match inspect_root(
            &normalized,
            safe_path_alias(&normalized),
            DiscoveryMethod::FullDevice,
            None,
            Some(&mut self.signature_probe),
        ) {
            RootInspection::Found(root) => {
                self.roots.insert(normalized, root);
                self.publish_progress();
            }
            RootInspection::RejectedSignature | RootInspection::Indeterminate => {
                self.skipped_count = self.skipped_count.saturating_add(1);
                self.pending
                    .push_back(PendingTask::Enumerate(Box::new(DirectoryCursor {
                        path: normalized,
                        traversal_root,
                        policy_path,
                        entries: None,
                        buffered_entry: None,
                        children: Vec::new(),
                    })));
            }
            RootInspection::NotRoot => {
                self.pending
                    .push_back(PendingTask::Enumerate(Box::new(DirectoryCursor {
                        path: normalized,
                        traversal_root,
                        policy_path,
                        entries: None,
                        buffered_entry: None,
                        children: Vec::new(),
                    })));
            }
            RootInspection::Cancelled => self.user_cancelled = true,
            RootInspection::BudgetExhausted => {
                self.budget_exhausted = true;
                self.skipped_count = self.skipped_count.saturating_add(1);
            }
        }
        if self.cancellation.is_cancelled() {
            self.user_cancelled = true;
        }
    }

    /// 提交一个有界目录项片段，扣减真实共享预算并排队继续游标与普通子目录。
    fn commit_enumeration(&mut self, outcome: EnumerationOutcome) {
        for _ in 0..outcome.entries_consumed {
            if !self.traversal_budget.claim_entry() {
                self.budget_exhausted = true;
                self.skipped_count = self.skipped_count.saturating_add(1);
                return;
            }
        }
        self.permission_denied_count = self
            .permission_denied_count
            .saturating_add(outcome.permission_denied_count);
        self.skipped_count = self.skipped_count.saturating_add(outcome.skipped_count);
        self.symlink_skipped_count = self
            .symlink_skipped_count
            .saturating_add(outcome.symlink_skipped_count);
        for child in outcome.children {
            self.pending.push_back(PendingTask::Preflight(child));
        }
        if let Some(continuation) = outcome.continuation {
            self.pending
                .push_front(PendingTask::Enumerate(Box::new(continuation)));
        }
        if outcome.cancelled || self.cancellation.is_cancelled() {
            self.user_cancelled = true;
        }
    }

    /// 只发布累计目录数与根数，不允许 worker 直接产生乱序可见进度。
    fn publish_progress(&mut self) {
        (self.on_progress)(DiscoveryProgress {
            directories_scanned: self.directories_scanned,
            roots_discovered: u64::try_from(self.roots.len()).unwrap_or(u64::MAX),
        });
    }

    /// 汇总终态覆盖并按规范化路径稳定返回根，不泄露任一路径到 DTO。
    pub(super) fn finish(mut self) -> DiscoveryResult {
        self.publish_progress();
        let state = if self.user_cancelled || self.cancellation.is_cancelled() {
            CoverageState::Cancelled
        } else if self.budget_exhausted
            || self.permission_denied_count > 0
            || self.skipped_count > 0
        {
            CoverageState::Partial
        } else {
            CoverageState::Complete
        };
        let roots = self.roots.into_values().collect::<Vec<_>>();
        DiscoveryResult {
            coverage: CoverageReport {
                state,
                roots_scanned: u64::try_from(self.traversal_roots_started.len())
                    .unwrap_or(u64::MAX),
                roots_discovered: u64::try_from(roots.len()).unwrap_or(u64::MAX),
                permission_denied_count: self.permission_denied_count,
                skipped_count: self.skipped_count,
                warning_count: 0,
            },
            roots,
            confirmed_invalid_root_ids: Vec::new(),
            unconfirmed_root_ids: Vec::new(),
            directories_scanned: self.directories_scanned,
            symlink_skipped_count: self.symlink_skipped_count,
            network_skipped_count: self.network_skipped_count,
        }
    }
}
