//! 快速发现：只检查默认、当前环境与已启用登记根，不做递归卷遍历。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use loki_metis_core::{CoverageReport, CoverageState};

use super::inspection::{
    RootInspection, SignatureProbeContext, inspect_root, reject_symlink_components, safe_path_alias,
};
use super::{DiscoveredRoot, DiscoveryInputs, DiscoveryResult, FullDiscoveryOptions};
use crate::backend::local_index::{
    CancellationToken, DiscoveryMethod, LocalPathStatus, classify_local_path,
    is_obviously_network_path,
};

/// 保存一个非递归根候选及其可选历史 registry 身份。
struct DiscoveryCandidate {
    /// 已持久化根的精确 ID；默认、环境或新选择候选为空。
    existing_root_id: Option<String>,
    /// adapter 内只读访问路径。
    path: PathBuf,
    /// 安全展示别名。
    alias: String,
    /// 候选进入发现流程的入口。
    method: DiscoveryMethod,
}

/// 只检查默认、当前环境与已启用登记根，并在有界签名探测中响应同一扫描取消令牌。
pub fn discover_quick(
    inputs: &DiscoveryInputs,
    cancellation: &CancellationToken,
) -> DiscoveryResult {
    let candidates = quick_candidates(inputs);
    let options = FullDiscoveryOptions::default();
    discover_candidates(candidates, cancellation, &options)
}

/// 按信任优先级构造候选：用户已登记根先消费共享签名预算，其次才是
/// 显式环境根和可选默认根，避免异常自动候选长期饿死用户维护的数据源。
fn quick_candidates(inputs: &DiscoveryInputs) -> Vec<DiscoveryCandidate> {
    let mut candidates = Vec::new();
    for registered in &inputs.registered_roots {
        if registered.enabled {
            candidates.push(DiscoveryCandidate {
                existing_root_id: registered.root_id.clone(),
                path: registered.path.clone(),
                alias: registered.alias.clone(),
                method: DiscoveryMethod::Registered,
            });
        }
    }
    if let Some(codex_home) = &inputs.codex_home {
        candidates.push(DiscoveryCandidate {
            existing_root_id: None,
            path: codex_home.clone(),
            alias: safe_path_alias(codex_home),
            method: DiscoveryMethod::Environment,
        });
    }
    if let Some(home_dir) = &inputs.home_dir {
        let path = home_dir.join(".codex");
        candidates.push(DiscoveryCandidate {
            existing_root_id: None,
            alias: safe_path_alias(&path),
            path,
            method: DiscoveryMethod::DefaultHome,
        });
    }
    candidates
}

/// 对一组非递归候选执行去重和有界 rollout 结构签名检查。
fn discover_candidates(
    candidates: Vec<DiscoveryCandidate>,
    cancellation: &CancellationToken,
    options: &FullDiscoveryOptions,
) -> DiscoveryResult {
    let candidate_count = u64::try_from(candidates.len()).unwrap_or(u64::MAX);
    let existing_root_ids = candidates
        .iter()
        .filter_map(|candidate| candidate.existing_root_id.clone())
        .collect::<BTreeSet<_>>();
    let mut roots = BTreeMap::<PathBuf, DiscoveredRoot>::new();
    let mut confirmed_invalid_root_ids = BTreeSet::<String>::new();
    let mut unconfirmed_root_ids = BTreeSet::<String>::new();
    let mut confirmed_invalid_paths = BTreeSet::<PathBuf>::new();
    let mut unconfirmed_paths = BTreeSet::<PathBuf>::new();
    let mut inspected_paths = BTreeSet::<PathBuf>::new();
    let mut permission_denied_count = 0_u64;
    let mut skipped_count = 0_u64;
    let mut symlink_skipped_count = 0_u64;
    let mut network_skipped_count = 0_u64;
    let mut user_cancelled = false;
    let mut budget_exhausted = false;
    let mut signature_probe = SignatureProbeContext::new(options, cancellation);

    for candidate in candidates {
        let DiscoveryCandidate {
            existing_root_id,
            path,
            alias,
            method,
        } = candidate;
        if cancellation.is_cancelled() {
            user_cancelled = true;
            break;
        }
        if is_obviously_network_path(&path) {
            network_skipped_count = network_skipped_count.saturating_add(1);
            skipped_count = skipped_count.saturating_add(1);
            if let Some(root_id) = existing_root_id {
                confirmed_invalid_root_ids.insert(root_id);
            }
            continue;
        }
        match reject_symlink_components(&path) {
            Ok(true) => {
                symlink_skipped_count = symlink_skipped_count.saturating_add(1);
                skipped_count = skipped_count.saturating_add(1);
                if let Some(root_id) = existing_root_id {
                    confirmed_invalid_root_ids.insert(root_id);
                }
                continue;
            }
            Ok(false) => {}
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                permission_denied_count = permission_denied_count.saturating_add(1);
                if let Some(root_id) = existing_root_id {
                    unconfirmed_root_ids.insert(root_id);
                }
                continue;
            }
            Err(_) => {
                skipped_count = skipped_count.saturating_add(1);
                if let Some(root_id) = existing_root_id {
                    unconfirmed_root_ids.insert(root_id);
                }
                continue;
            }
        }
        match classify_local_path(&path) {
            LocalPathStatus::ConfirmedLocal => {}
            LocalPathStatus::RejectedNetwork => {
                network_skipped_count = network_skipped_count.saturating_add(1);
                skipped_count = skipped_count.saturating_add(1);
                if let Some(root_id) = existing_root_id {
                    confirmed_invalid_root_ids.insert(root_id);
                }
                continue;
            }
            LocalPathStatus::Missing => {
                // 默认 `.codex` 是可选自动候选；尚未创建是正常状态，不应
                // 让一个有效的显式环境根永久显示 Partial。
                if method.counts_as_coverage_gap(existing_root_id.is_some()) {
                    skipped_count = skipped_count.saturating_add(1);
                }
                if let Some(root_id) = existing_root_id {
                    confirmed_invalid_root_ids.insert(root_id);
                }
                continue;
            }
            LocalPathStatus::Indeterminate => {
                skipped_count = skipped_count.saturating_add(1);
                if let Some(root_id) = existing_root_id {
                    unconfirmed_root_ids.insert(root_id);
                }
                continue;
            }
        }
        let normalized = match fs::canonicalize(&path) {
            Ok(normalized) => normalized,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if method.counts_as_coverage_gap(existing_root_id.is_some()) {
                    skipped_count = skipped_count.saturating_add(1);
                }
                if let Some(root_id) = existing_root_id {
                    confirmed_invalid_root_ids.insert(root_id);
                }
                continue;
            }
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                permission_denied_count = permission_denied_count.saturating_add(1);
                if let Some(root_id) = existing_root_id {
                    unconfirmed_root_ids.insert(root_id);
                }
                continue;
            }
            Err(_) => {
                skipped_count = skipped_count.saturating_add(1);
                if let Some(root_id) = existing_root_id {
                    unconfirmed_root_ids.insert(root_id);
                }
                continue;
            }
        };
        if !inspected_paths.insert(normalized.clone()) {
            if let Some(root) = roots.get_mut(&normalized) {
                if method.priority() > root.discovery_method.priority() {
                    root.alias = alias;
                    root.discovery_method = method;
                }
                if method == DiscoveryMethod::Registered
                    && let Some(root_id) = existing_root_id
                {
                    root.root_id = root_id;
                }
            } else if confirmed_invalid_paths.contains(&normalized)
                && let Some(root_id) = existing_root_id
            {
                confirmed_invalid_root_ids.insert(root_id);
            } else if unconfirmed_paths.contains(&normalized)
                && let Some(root_id) = existing_root_id
            {
                unconfirmed_root_ids.insert(root_id);
            }
            continue;
        }
        match inspect_root(
            &normalized,
            alias,
            method,
            existing_root_id.as_deref(),
            Some(&mut signature_probe),
        ) {
            RootInspection::Found(root) => {
                roots.insert(normalized.clone(), root);
            }
            RootInspection::NotRoot | RootInspection::RejectedSignature => {
                if method.counts_as_coverage_gap(existing_root_id.is_some()) {
                    skipped_count = skipped_count.saturating_add(1);
                }
                confirmed_invalid_paths.insert(normalized);
                if let Some(root_id) = existing_root_id {
                    confirmed_invalid_root_ids.insert(root_id);
                }
            }
            RootInspection::Indeterminate => {
                skipped_count = skipped_count.saturating_add(1);
                unconfirmed_paths.insert(normalized);
                if let Some(root_id) = existing_root_id {
                    unconfirmed_root_ids.insert(root_id);
                }
            }
            RootInspection::Cancelled => {
                user_cancelled = true;
                break;
            }
            RootInspection::BudgetExhausted => {
                budget_exhausted = true;
                skipped_count = skipped_count.saturating_add(1);
                if let Some(root_id) = existing_root_id {
                    unconfirmed_root_ids.insert(root_id);
                }
                break;
            }
        }
    }

    let state = if user_cancelled {
        CoverageState::Cancelled
    } else if budget_exhausted || permission_denied_count > 0 || skipped_count > 0 {
        CoverageState::Partial
    } else {
        CoverageState::Complete
    };
    if budget_exhausted {
        // 预算耗尽会在循环中途 break，跳出前尚未处理到的候选根既没有进入
        // roots（确认有效），也没有进入 confirmed_invalid_root_ids（确认失效）。
        // 这些“没被遍历到”的已登记根必须显式标记为 unconfirmed，否则会被
        // 误当成本次未提及、维持旧状态不变的根，从而绕过本次的重新校验。
        for root_id in existing_root_ids {
            let was_confirmed = roots.values().any(|root| root.root_id == root_id)
                || confirmed_invalid_root_ids.contains(&root_id);
            if !was_confirmed {
                unconfirmed_root_ids.insert(root_id);
            }
        }
    }
    let roots = roots.into_values().collect::<Vec<_>>();
    DiscoveryResult {
        coverage: CoverageReport {
            state,
            roots_scanned: candidate_count,
            roots_discovered: u64::try_from(roots.len()).unwrap_or(u64::MAX),
            permission_denied_count,
            skipped_count,
            warning_count: 0,
        },
        roots,
        confirmed_invalid_root_ids: confirmed_invalid_root_ids.into_iter().collect(),
        unconfirmed_root_ids: unconfirmed_root_ids.into_iter().collect(),
        directories_scanned: 0,
        symlink_skipped_count,
        network_skipped_count,
    }
}

#[cfg(test)]
mod tests;
