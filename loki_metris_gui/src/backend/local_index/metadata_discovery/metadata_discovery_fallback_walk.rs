// 本文件是 mod.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：使用给定总预算执行兜底遍历，供生产固定预算与 deadline 合同测试复用。；Spotlight 等系统索引结果必须服从当前用户目录或本地卷边界。；全盘策略：排除名单前缀或任意祖先目录名命中跳过名单。

/// 使用给定总预算执行兜底遍历，供生产固定预算与 deadline 合同测试复用。
fn discover_metadata_roots_in_with_callback_and_budget(
    coordinator: &RootDiscoveryCoordinator,
    volumes: LocalVolumeRoots,
    priority_roots: Vec<PathBuf>,
    on_candidate: &impl Fn(RootCandidate),
    budget: MetadataTraversalBudget,
) -> MetadataDiscoverySummary {
    let total = u64::try_from(volumes.search_roots.len()).unwrap_or(u64::MAX);
    let excluded_roots = volumes.excluded_roots.clone();
    if coordinator.snapshot().lifecycle != loki_metris_core::RootDiscoveryLifecycle::Running
        && !coordinator.start(
            RootDiscoveryStrategy::MetadataTraversal,
            RootDiscoveryPlatform::Other,
            RootDiscoveryScope::FullLocalVolumes,
            total,
        )
    {
        return MetadataDiscoverySummary::default();
    }
    let mut progress = record_volume_scope_gaps(coordinator, &volumes, total);
    let mut queues = volumes
        .search_roots
        .into_iter()
        .map(|root| VolumeQueue {
            pending: priority_queue(&root, &priority_roots)
                .into_iter()
                .filter(|path| !is_discovery_path_excluded(path, &root, &excluded_roots))
                .collect(),
            completed: false,
            traversal_root: root,
        })
        .collect::<Vec<_>>();
    let mut visited = queues
        .iter()
        .flat_map(|queue| queue.pending.iter().cloned())
        .collect::<HashSet<_>>();
    let mut next_volume = 0_usize;
    let worker_pool = MetadataWorkerPool::new(adaptive_worker_count(), coordinator, budget);
    let mut traversal_stop = None;
    let mut coverage_limited = false;

    'traversal: while queues.iter().any(|queue| !queue.completed) {
        if let Some(stop_reason) = budget.stop_reason(coordinator) {
            traversal_stop = Some(stop_reason);
            break;
        }
        let jobs = take_fair_jobs(&mut queues, adaptive_worker_count(), &mut next_volume);
        if jobs.is_empty() {
            mark_completed_queues(&mut queues, &mut progress);
            continue;
        }
        let batch = worker_pool.inspect(jobs);
        if let Some(stop_reason) = batch.stop_reason {
            traversal_stop = Some(stop_reason);
            break;
        }
        for (volume, joined) in batch.results {
            if let Some(stop_reason) = budget.stop_reason(coordinator) {
                traversal_stop = Some(stop_reason);
                break 'traversal;
            }
            progress.directories_checked = progress.directories_checked.saturating_add(1);
            let Ok(result) = joined else {
                progress.io_errors = progress.io_errors.saturating_add(1);
                traversal_stop = Some(TraversalStopReason::WorkerUnavailable);
                break 'traversal;
            };
            progress.file_names_checked = progress
                .file_names_checked
                .saturating_add(result.file_names_checked);
            progress.permission_denied = progress
                .permission_denied
                .saturating_add(result.permission_denied);
            progress.io_errors = progress.io_errors.saturating_add(result.io_errors);
            progress.skipped = progress.skipped.saturating_add(result.skipped);
            match result.stop_reason {
                Some(TraversalStopReason::Cancelled | TraversalStopReason::DeadlineExceeded) => {
                    traversal_stop = result.stop_reason;
                    break 'traversal;
                }
                Some(TraversalStopReason::DirectoryEntryLimit) => coverage_limited = true,
                Some(TraversalStopReason::WorkerUnavailable) => {
                    traversal_stop = result.stop_reason;
                    break 'traversal;
                }
                None => {}
            }
            let traversal_root = queues[volume].traversal_root.clone();
            for child in result.children {
                if let Some(stop_reason) = budget.stop_reason(coordinator) {
                    traversal_stop = Some(stop_reason);
                    break 'traversal;
                }
                if is_discovery_path_excluded(&child, &traversal_root, &excluded_roots) {
                    progress.skipped = progress.skipped.saturating_add(1);
                    continue;
                }
                if let Some(stop_reason) = budget.stop_reason(coordinator) {
                    traversal_stop = Some(stop_reason);
                    break 'traversal;
                }
                if visited.insert(child.clone()) {
                    queues[volume].pending.push_back(child);
                }
            }
            for (path, evidence) in result.candidates {
                if let Some(stop_reason) = budget.stop_reason(coordinator) {
                    traversal_stop = Some(stop_reason);
                    break 'traversal;
                }
                if path_has_full_discovery_skip_directory_name(&path)
                    || excluded_roots
                        .iter()
                        .any(|excluded| path.starts_with(excluded))
                {
                    progress.skipped = progress.skipped.saturating_add(1);
                    continue;
                }
                if let Some(stop_reason) = budget.stop_reason(coordinator) {
                    traversal_stop = Some(stop_reason);
                    break 'traversal;
                }
                submit_candidate(
                    coordinator,
                    path,
                    evidence,
                    RootDiscoveryStrategy::MetadataTraversal,
                    on_candidate,
                );
            }
        }
        mark_completed_queues(&mut queues, &mut progress);
        coordinator.update_progress(progress);
    }
    coordinator.update_progress(progress);
    traversal_stop = traversal_stop.or_else(|| budget.stop_reason(coordinator));
    if traversal_stop == Some(TraversalStopReason::Cancelled) {
        coordinator.finish(false, true);
    } else if let Some(error_code) = traversal_stop.and_then(TraversalStopReason::error_code) {
        coordinator.fail(error_code);
    } else if coverage_limited {
        coordinator.fail(
            TraversalStopReason::DirectoryEntryLimit
                .error_code()
                .expect("directory limit has a stable error code"),
        );
    } else {
        coordinator.finish(false, true);
    }
    MetadataDiscoverySummary {
        system_index_available: false,
        fallback_performed: true,
    }
}

/// Spotlight 等系统索引结果必须服从当前用户目录或本地卷边界。
fn path_is_in_search_scope(path: &Path, volumes: &LocalVolumeRoots) -> bool {
    volumes
        .search_roots
        .iter()
        .any(|root| path.starts_with(root))
}

/// 全盘策略：排除名单前缀或任意祖先目录名命中跳过名单。
fn path_blocked_by_discovery_policy(path: &Path, volumes: &LocalVolumeRoots) -> bool {
    if volumes
        .excluded_roots
        .iter()
        .any(|excluded| path.starts_with(excluded))
    {
        return true;
    }
    path.ancestors()
        .any(path_has_full_discovery_skip_directory_name)
}

/// 每轮从不同卷各取一个目录，确保较大前序卷不会饿死后续卷。
fn take_fair_jobs(
    queues: &mut [VolumeQueue],
    workers: usize,
    next_volume: &mut usize,
) -> Vec<(usize, PathBuf)> {
    let mut jobs = Vec::new();
    if queues.is_empty() {
        return jobs;
    }
    let mut consecutive_empty = 0_usize;
    while jobs.len() < workers && consecutive_empty < queues.len() {
        let index = *next_volume % queues.len();
        *next_volume = (index + 1) % queues.len();
        if let Some(path) = queues[index].pending.pop_front() {
            jobs.push((index, path));
            consecutive_empty = 0;
        } else {
            consecutive_empty += 1;
        }
    }
    jobs
}

/// 根据逻辑 CPU 自适应选择一到两个只读 worker。
fn adaptive_worker_count() -> usize {
    std::thread::available_parallelism()
        .map_or(1, usize::from)
        .clamp(1, loki_metris_core::LOCAL_DISCOVERY_WORKER_LIMIT)
}

/// 把队列耗尽的卷计入完成数。
fn mark_completed_queues(queues: &mut [VolumeQueue], progress: &mut RootDiscoveryProgress) {
    for queue in queues {
        if !queue.completed && queue.pending.is_empty() {
            queue.completed = true;
            progress.volumes_completed = progress.volumes_completed.saturating_add(1);
        }
    }
}

/// 只按文件名与父目录结构推导候选根。
fn candidate_from_file_name(path: &Path) -> Option<(PathBuf, RootCandidateEvidence)> {
    let name = path.file_name()?.to_str()?;
    if name.starts_with("rollout-") && name.ends_with(".jsonl") {
        for ancestor in path.ancestors().skip(1) {
            let component = ancestor.file_name()?.to_str()?;
            if matches!(component, "sessions" | "archived_sessions") {
                let root = ancestor.parent()?.to_path_buf();
                return allowed_root(&root).then_some((root, RootCandidateEvidence::CodexRollout));
            }
        }
    }
    if !name.ends_with(".jsonl") {
        return None;
    }
    let stem = name.strip_suffix(".jsonl")?;
    let parent = path.parent()?;
    if super::claude::path_rules::is_uuid(stem)
        && parent.parent()?.file_name()?.to_str()? == "projects"
    {
        let root = parent.parent()?.parent()?.to_path_buf();
        return allowed_root(&root).then_some((root, RootCandidateEvidence::ClaudeTranscript));
    }
    if super::claude::path_rules::is_subagent_jsonl_name(name)
        && parent.file_name()?.to_str()? == "subagents"
    {
        let project_root = parent.parent()?.parent()?;
        if project_root.parent()?.file_name()?.to_str()? == "projects" {
            let root = project_root.parent()?.parent()?.to_path_buf();
            return allowed_root(&root).then_some((root, RootCandidateEvidence::ClaudeSubagent));
        }
    }
    if name == "updates.jsonl" {
        let session_dir = parent;
        let cwd_dir = session_dir.parent()?;
        let sessions = cwd_dir.parent()?;
        if sessions.file_name()?.to_str()? == "sessions" {
            let root = sessions.parent()?.to_path_buf();
            return allowed_root(&root)
                .then_some((root, RootCandidateEvidence::GrokSessionUpdates));
        }
    }
    None
}

/// 拒绝不得误作数据源的辅助目录名称。
fn allowed_root(path: &Path) -> bool {
    !matches!(
        path.file_name()
            .and_then(|name| name.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("logs" | "log" | "cache" | "caches" | "test" | "tests")
    )
}

/// 按规范路径去重并提交候选，日志不得调用此函数参数的 Display。
fn submit_candidate(
    coordinator: &RootDiscoveryCoordinator,
    path: PathBuf,
    evidence: RootCandidateEvidence,
    strategy: RootDiscoveryStrategy,
    on_candidate: &impl Fn(RootCandidate),
) {
    if coordinator.is_cancel_requested() {
        return;
    }
    let client = match evidence {
        RootCandidateEvidence::CodexRollout => SourceClientKind::Codex,
        RootCandidateEvidence::ClaudeTranscript | RootCandidateEvidence::ClaudeSubagent => {
            SourceClientKind::ClaudeCode
        }
        RootCandidateEvidence::GrokSessionUpdates => SourceClientKind::Grok,
        RootCandidateEvidence::WorkBuddyProjectJsonl => SourceClientKind::WorkBuddy,
    };
    let key = path.to_string_lossy().to_string();
    let candidate = RootCandidate {
        id: stable_id(client.candidate_namespace(), &key),
        client,
        absolute_path: key,
        strategy,
        evidence,
    };
    if coordinator.submit_candidate(candidate.clone()) {
        if coordinator.is_cancel_requested() {
            coordinator.remove_candidates(std::slice::from_ref(&candidate.id));
            return;
        }
        on_candidate(candidate);
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod contract_tests;
