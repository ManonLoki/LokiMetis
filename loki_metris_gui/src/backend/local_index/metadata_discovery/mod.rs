//! 平台范围只凭卷、目录项与文件名发现候选；显式环境根另走有界严格签名。

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[cfg(test)]
use std::fs;

use loki_metris_core::{
    CoverageReport, RootCandidate, RootCandidateEvidence, RootDiscoveryCoordinator,
    RootDiscoveryPlatform, RootDiscoveryProgress, RootDiscoveryScope, RootDiscoveryStrategy,
    SourceClientKind, append_platform_discovery_excludes_for_volumes, is_discovery_path_excluded,
    path_has_full_discovery_skip_directory_name, stable_id,
};

use super::current_user_home;
use super::{
    CancellationToken, ClaudeDiscoveryInputs, DiscoveryInputs, GrokDiscoveryInputs,
    LocalVolumeRoots, discover_claude_quick, discover_grok_quick, discover_quick,
    validate_local_plain_directory,
};

mod platform_index;
mod scope;
mod worker_pool;

use scope::{discovery_scope, platform_priority_roots, priority_queue};
use worker_pool::{MetadataTraversalBudget, MetadataWorkerPool, TraversalStopReason};

/// 执行结果只包含非路径计数；候选直接提交到运行时协调器。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MetadataDiscoverySummary {
    /// 系统索引是否可用。
    pub system_index_available: bool,
    /// 是否执行了普通元数据兜底。
    pub fallback_performed: bool,
}

/// 显式环境根的严格签名结果与必须向 UI 保留的覆盖缺口。
#[derive(Debug, Default)]
struct ExplicitEnvironmentInspection {
    /// 通过严格签名确认的候选路径及其证据类型。
    candidates: Vec<(PathBuf, RootCandidateEvidence)>,
    /// 权限拒绝计数。
    permission_denied: u64,
    /// 因链接、非本地卷等原因跳过的计数。
    skipped: u64,
}

/// 单个卷内待遍历目录的广度优先队列及其完成标记。
#[derive(Debug)]
struct VolumeQueue {
    /// 待处理的目录队列。
    pending: VecDeque<PathBuf>,
    /// 标识本卷是否已遍历完成。
    completed: bool,
    /// 该卷发现遍历的根路径。
    traversal_root: PathBuf,
}

/// 执行发现并在候选首次插入时立即通知调用方。
pub(crate) fn discover_metadata_roots_with_callback(
    coordinator: &RootDiscoveryCoordinator,
    discovery_kind: RootDiscoveryScope,
    on_candidate: impl Fn(RootCandidate),
) -> MetadataDiscoverySummary {
    let mut volumes = discovery_scope(discovery_kind);
    if discovery_kind == RootDiscoveryScope::FullLocalVolumes {
        volumes.excluded_roots = append_platform_discovery_excludes_for_volumes(
            std::mem::take(&mut volumes.excluded_roots),
            &volumes.search_roots,
        );
    }
    let total = u64::try_from(volumes.search_roots.len()).unwrap_or(u64::MAX);
    let strategy = RootDiscoveryStrategy::current();
    if coordinator.snapshot().lifecycle != loki_metris_core::RootDiscoveryLifecycle::Running
        && !coordinator.start(
            strategy,
            RootDiscoveryPlatform::current(),
            discovery_kind,
            total,
        )
    {
        return MetadataDiscoverySummary::default();
    }
    if discovery_kind == RootDiscoveryScope::UserPriority {
        let inspection = explicit_environment_candidates(
            coordinator,
            std::env::var_os("CODEX_HOME").map(PathBuf::from),
            std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from),
            std::env::var_os("GROK_HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
            current_user_home(),
        );
        for (path, evidence) in inspection.candidates.iter().cloned() {
            submit_candidate(
                coordinator,
                path,
                evidence,
                RootDiscoveryStrategy::MetadataTraversal,
                &on_candidate,
            );
        }
        record_explicit_environment_gaps(coordinator, &inspection);
    }
    if total == 0 {
        record_volume_scope_gaps(coordinator, &volumes, total);
        if discovery_kind == RootDiscoveryScope::FullLocalVolumes
            && coordinator.candidates().is_empty()
        {
            coordinator.fail("no_local_volumes");
        } else {
            coordinator.finish(false, false);
        }
        return MetadataDiscoverySummary::default();
    }
    let (system_index_available, indexed_paths) = platform_index::query(&volumes, coordinator);
    submit_indexed_path_candidates(
        coordinator,
        &volumes,
        strategy,
        indexed_paths,
        &on_candidate,
    );
    coordinator.begin_fallback(system_index_available);
    let mut summary = discover_metadata_roots_in_with_callback(
        coordinator,
        volumes,
        platform_priority_roots(),
        &on_candidate,
    );
    if coordinator.snapshot().lifecycle != loki_metris_core::RootDiscoveryLifecycle::Failed {
        coordinator.finish(system_index_available, true);
    }
    summary.system_index_available = system_index_available;
    summary
}

/// 逐项校验系统索引结果；取消后不得继续访问路径或向协调器提交新候选。
fn submit_indexed_path_candidates(
    coordinator: &RootDiscoveryCoordinator,
    volumes: &LocalVolumeRoots,
    strategy: RootDiscoveryStrategy,
    indexed_paths: impl IntoIterator<Item = PathBuf>,
    on_candidate: &impl Fn(RootCandidate),
) {
    for path in indexed_paths {
        if coordinator.is_cancel_requested() {
            break;
        }
        if path_blocked_by_discovery_policy(&path, volumes)
            || !path_is_in_search_scope(&path, volumes)
        {
            continue;
        }
        if let Some((candidate, evidence)) = candidate_from_file_name(&path)
            && validate_local_plain_directory(&candidate).is_ok()
            && !coordinator.is_cancel_requested()
        {
            submit_candidate(coordinator, candidate, evidence, strategy, on_candidate);
        }
    }
}

/// 有界核对 Codex/Claude 环境根与 Grok 的环境根或默认 `~/.grok`。
fn explicit_environment_candidates(
    coordinator: &RootDiscoveryCoordinator,
    codex_home: Option<PathBuf>,
    claude_config_dir: Option<PathBuf>,
    grok_home: Option<PathBuf>,
    home_dir: Option<PathBuf>,
) -> ExplicitEnvironmentInspection {
    if codex_home.is_none()
        && claude_config_dir.is_none()
        && grok_home.is_none()
        && home_dir.is_none()
    {
        return ExplicitEnvironmentInspection::default();
    }
    let cancellation = CancellationToken::new();
    if coordinator.is_cancel_requested() {
        cancellation.cancel();
    }
    let finished = Arc::new(AtomicBool::new(false));
    std::thread::scope(|scope| {
        let cancellation_for_monitor = cancellation.clone();
        let finished_for_monitor = Arc::clone(&finished);
        let monitor = scope.spawn(move || {
            while !finished_for_monitor.load(Ordering::Acquire) {
                if coordinator.is_cancel_requested() {
                    cancellation_for_monitor.cancel();
                    break;
                }
                std::thread::park_timeout(Duration::from_millis(10));
            }
        });
        let candidates = inspect_explicit_environment_candidates(
            codex_home,
            claude_config_dir,
            grok_home,
            home_dir,
            &cancellation,
        );
        finished.store(true, Ordering::Release);
        monitor.thread().unpark();
        let _ = monitor.join();
        candidates
    })
}

/// 使用现有 Quick 签名预算核对环境根；Grok 在无 `GROK_HOME` 时仍核对默认 `~/.grok`。
/// Codex 与 Claude 互不依赖，并发核对以缩短有界扫描的墙钟时间。
fn inspect_explicit_environment_candidates(
    codex_home: Option<PathBuf>,
    claude_config_dir: Option<PathBuf>,
    grok_home: Option<PathBuf>,
    home_dir: Option<PathBuf>,
    cancellation: &CancellationToken,
) -> ExplicitEnvironmentInspection {
    let codex_home = codex_home.filter(|path| path.is_absolute());
    let claude_config_dir = claude_config_dir.filter(|path| path.is_absolute());
    let grok_home = grok_home.filter(|path| path.is_absolute());
    let home_dir = home_dir.filter(|path| path.is_absolute());
    let (codex_result, claude_result, grok_result) = std::thread::scope(|scope| {
        let codex_handle = codex_home.map(|codex_home| {
            let cancellation = cancellation.clone();
            scope.spawn(move || {
                discover_quick(
                    &DiscoveryInputs {
                        codex_home: Some(codex_home),
                        ..DiscoveryInputs::default()
                    },
                    &cancellation,
                )
            })
        });
        let claude_handle = claude_config_dir.map(|claude_config_dir| {
            let cancellation = cancellation.clone();
            scope.spawn(move || {
                discover_claude_quick(
                    &ClaudeDiscoveryInputs {
                        claude_config_dir: Some(claude_config_dir),
                        ..ClaudeDiscoveryInputs::default()
                    },
                    &cancellation,
                )
            })
        });
        let grok_handle = (grok_home.is_some() || home_dir.is_some()).then(|| {
            let cancellation = cancellation.clone();
            scope.spawn(move || {
                discover_grok_quick(
                    &GrokDiscoveryInputs {
                        grok_home,
                        home_dir,
                        registered_roots: Vec::new(),
                    },
                    &cancellation,
                )
            })
        });
        (
            codex_handle.map(|handle| handle.join().expect("codex quick discovery panicked")),
            claude_handle.map(|handle| handle.join().expect("claude quick discovery panicked")),
            grok_handle.map(|handle| handle.join().expect("grok quick discovery panicked")),
        )
    });

    let mut inspection = ExplicitEnvironmentInspection::default();
    if let Some(result) = codex_result {
        merge_coverage(&mut inspection, &result.coverage);
        inspection.candidates.extend(
            result
                .roots
                .into_iter()
                .map(|root| (root.path, RootCandidateEvidence::CodexRollout)),
        );
    }
    if let Some(result) = claude_result {
        merge_coverage(&mut inspection, &result.coverage);
        inspection.candidates.extend(
            result
                .roots
                .into_iter()
                .map(|root| (root.path, root.evidence)),
        );
    }
    if let Some(result) = grok_result {
        merge_coverage(&mut inspection, &result.coverage);
        inspection.candidates.extend(
            result
                .roots
                .into_iter()
                .map(|root| (root.path, root.evidence)),
        );
    }
    inspection
}

/// 把一次 Quick 发现的权限拒绝与跳过计数并入显式环境根的核对结果。
fn merge_coverage(inspection: &mut ExplicitEnvironmentInspection, coverage: &CoverageReport) {
    inspection.permission_denied = inspection
        .permission_denied
        .saturating_add(coverage.permission_denied_count);
    inspection.skipped = inspection.skipped.saturating_add(coverage.skipped_count);
}

/// 把环境精确根的权限、网络、链接或预算缺口并入同一个发现任务。
fn record_explicit_environment_gaps(
    coordinator: &RootDiscoveryCoordinator,
    inspection: &ExplicitEnvironmentInspection,
) {
    let mut progress = coordinator.snapshot().progress;
    progress.permission_denied = progress
        .permission_denied
        .saturating_add(inspection.permission_denied);
    progress.skipped = progress.skipped.saturating_add(inspection.skipped);
    coordinator.update_progress(progress);
}

/// 把卷预检在遍历前拒绝的网络、未知或链接根保留在任务进度中。
fn record_volume_scope_gaps(
    coordinator: &RootDiscoveryCoordinator,
    volumes: &LocalVolumeRoots,
    total: u64,
) -> RootDiscoveryProgress {
    let mut progress = coordinator.snapshot().progress;
    progress.volumes_total = total;
    progress.skipped = progress.skipped.saturating_add(
        volumes
            .network_skipped_count
            .saturating_add(volumes.other_skipped_count),
    );
    coordinator.update_progress(progress);
    progress
}

/// 使用显式卷边界执行发现，供多卷公平性测试复用。
#[cfg(test)]
pub(crate) fn discover_metadata_roots_in(
    coordinator: &RootDiscoveryCoordinator,
    volumes: LocalVolumeRoots,
) -> MetadataDiscoverySummary {
    discover_metadata_roots_in_with_callback(coordinator, volumes, Vec::new(), &|_| {})
}

/// 内部实现：跨多卷公平轮询遍历目录，命中即通过回调实时上报候选。
fn discover_metadata_roots_in_with_callback(
    coordinator: &RootDiscoveryCoordinator,
    volumes: LocalVolumeRoots,
    priority_roots: Vec<PathBuf>,
    on_candidate: &impl Fn(RootCandidate),
) -> MetadataDiscoverySummary {
    discover_metadata_roots_in_with_callback_and_budget(
        coordinator,
        volumes,
        priority_roots,
        on_candidate,
        MetadataTraversalBudget::new(),
    )
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - metadata_discovery_fallback_walk.rs：使用给定总预算执行兜底遍历，供生产固定预算与 deadline 合同测试复用。；Spotlight 等系统索引结果必须服从当前用户目录或本地卷边界。；全盘策略：排除名单前缀或任意祖先目录名命中跳过名单。
include!("metadata_discovery_fallback_walk.rs");
