//! Claude Code 有界全设备发现。

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::PathBuf;

use loki_metis_core::CoverageReport;

use super::super::super::discovery::{
    DiscoveryProgress, metadata_is_link_like, path_key, stable_id,
};
use super::super::super::{
    CancellationToken, DiscoveryMethod, FullDiscoveryOptions, is_obviously_network_path,
};
use super::signature::{
    Inspection, SignatureBudget, coverage_state, crosses_search_root, inspect_root, is_excluded,
    reject_link_components, safe_alias,
};
use super::{ClaudeDiscoveredRoot, ClaudeDiscoveryResult};

/// 用户主动发起的有界设备发现；命中根后停止下钻其日志、缓存和运行标记子树。
pub fn discover_claude_full_device_with_progress<F>(
    options: &FullDiscoveryOptions,
    cancellation: &CancellationToken,
    mut on_progress: F,
) -> ClaudeDiscoveryResult
where
    F: FnMut(DiscoveryProgress),
{
    let mut roots = BTreeMap::<PathBuf, ClaudeDiscoveredRoot>::new();
    let mut stack = options
        .search_roots
        .iter()
        .cloned()
        .map(|root| (root.clone(), root))
        .collect::<Vec<_>>();
    let mut visited = HashSet::new();
    let mut traversal_roots_started = BTreeSet::new();
    let mut signature_budget = SignatureBudget::new(options, cancellation);
    let mut directories_scanned = 0_u64;
    let mut remaining_entries = options.max_entries;
    let mut permission_denied_count = 0_u64;
    let mut skipped_count = options
        .preflight_network_skipped_count
        .saturating_add(options.preflight_other_skipped_count);
    let mut symlink_skipped_count = 0_u64;
    let mut network_skipped_count = options.preflight_network_skipped_count;
    let mut cancelled = false;
    let mut budget_exhausted = false;

    while let Some((path, traversal_root)) = stack.pop() {
        if cancellation.is_cancelled() {
            cancelled = true;
            break;
        }
        if directories_scanned >= options.max_directories {
            budget_exhausted = true;
            skipped_count = skipped_count.saturating_add(1);
            break;
        }
        traversal_roots_started.insert(traversal_root.clone());
        if is_excluded(&path, &traversal_root, &options.excluded_roots)
            || crosses_search_root(&path, &traversal_root, &options.search_roots)
        {
            skipped_count = skipped_count.saturating_add(1);
            continue;
        }
        if !options.allow_network_like_paths && is_obviously_network_path(&path) {
            network_skipped_count = network_skipped_count.saturating_add(1);
            skipped_count = skipped_count.saturating_add(1);
            continue;
        }
        if path == traversal_root {
            match reject_link_components(&path) {
                Ok(true) => {
                    symlink_skipped_count = symlink_skipped_count.saturating_add(1);
                    skipped_count = skipped_count.saturating_add(1);
                    continue;
                }
                Ok(false) => {}
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                    permission_denied_count = permission_denied_count.saturating_add(1);
                    continue;
                }
                Err(_) => {
                    skipped_count = skipped_count.saturating_add(1);
                    continue;
                }
            }
        }
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                if error.kind() == std::io::ErrorKind::PermissionDenied {
                    permission_denied_count = permission_denied_count.saturating_add(1);
                } else {
                    skipped_count = skipped_count.saturating_add(1);
                }
                continue;
            }
        };
        if metadata_is_link_like(&metadata) {
            symlink_skipped_count = symlink_skipped_count.saturating_add(1);
            skipped_count = skipped_count.saturating_add(1);
            continue;
        }
        if !metadata.is_dir() {
            continue;
        }
        let normalized = fs::canonicalize(&path).unwrap_or(path.clone());
        if !visited.insert(normalized.clone()) {
            continue;
        }
        directories_scanned = directories_scanned.saturating_add(1);
        if directories_scanned == 1 || directories_scanned.is_multiple_of(128) {
            on_progress(DiscoveryProgress {
                directories_scanned,
                roots_discovered: u64::try_from(roots.len()).unwrap_or(u64::MAX),
            });
        }
        match inspect_root(&normalized, &mut signature_budget) {
            Inspection::Found(evidence) => {
                roots.insert(
                    normalized.clone(),
                    ClaudeDiscoveredRoot {
                        root_id: stable_id("claude-root", &path_key(&normalized)),
                        alias: safe_alias(&normalized),
                        path: normalized,
                        discovery_method: DiscoveryMethod::FullDevice,
                        evidence,
                    },
                );
                on_progress(DiscoveryProgress {
                    directories_scanned,
                    roots_discovered: u64::try_from(roots.len()).unwrap_or(u64::MAX),
                });
                continue;
            }
            Inspection::Rejected | Inspection::Indeterminate => {
                skipped_count = skipped_count.saturating_add(1)
            }
            Inspection::Cancelled => {
                cancelled = true;
                break;
            }
            Inspection::BudgetExhausted => {
                budget_exhausted = true;
                skipped_count = skipped_count.saturating_add(1);
                break;
            }
            Inspection::NotRoot => {}
        }

        let mut entries = match fs::read_dir(&normalized) {
            Ok(entries) => entries,
            Err(error) => {
                if error.kind() == std::io::ErrorKind::PermissionDenied {
                    permission_denied_count = permission_denied_count.saturating_add(1);
                } else {
                    skipped_count = skipped_count.saturating_add(1);
                }
                continue;
            }
        };
        loop {
            if cancellation.is_cancelled() {
                cancelled = true;
                break;
            }
            let Some(entry) = entries.next() else { break };
            if remaining_entries == 0 {
                budget_exhausted = true;
                skipped_count = skipped_count.saturating_add(1);
                break;
            }
            remaining_entries -= 1;
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    skipped_count = skipped_count.saturating_add(1);
                    continue;
                }
            };
            match fs::symlink_metadata(entry.path()) {
                Ok(metadata) if metadata_is_link_like(&metadata) => {
                    symlink_skipped_count = symlink_skipped_count.saturating_add(1);
                    skipped_count = skipped_count.saturating_add(1);
                }
                Ok(metadata) if metadata.is_dir() => {
                    stack.push((entry.path(), traversal_root.clone()))
                }
                Ok(_) => {}
                Err(_) => skipped_count = skipped_count.saturating_add(1),
            }
        }
        if cancelled || budget_exhausted {
            break;
        }
    }

    on_progress(DiscoveryProgress {
        directories_scanned,
        roots_discovered: u64::try_from(roots.len()).unwrap_or(u64::MAX),
    });
    let state = coverage_state(
        cancelled,
        budget_exhausted,
        permission_denied_count,
        skipped_count,
    );
    let roots = roots.into_values().collect::<Vec<_>>();
    ClaudeDiscoveryResult {
        coverage: CoverageReport {
            state,
            roots_scanned: u64::try_from(traversal_roots_started.len()).unwrap_or(u64::MAX),
            roots_discovered: u64::try_from(roots.len()).unwrap_or(u64::MAX),
            permission_denied_count,
            skipped_count,
            warning_count: 0,
        },
        roots,
        confirmed_invalid_root_ids: Vec::new(),
        unconfirmed_root_ids: Vec::new(),
        directories_scanned,
        symlink_skipped_count,
        network_skipped_count,
    }
}
