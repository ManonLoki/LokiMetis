//! Claude Code 数据根发现：只承认直属 `projects` 中的有界 transcript 结构签名。
//! 整体结构和思路与父模块 `discovery.rs`/`discovery/quick.rs`/
//! `discovery/full_device.rs` 几乎一一对应（快速三类候选 + 主动全设备
//! 递归遍历 + 有界签名探测），差异只在于“什么样的文件/目录结构才算
//! 一个合法的 Claude 数据源”：Codex 认的是 `sessions/rollout-*.jsonl`，
//! 这里认的是 `projects/<project>/<session>.jsonl`
//! （见下方 `is_uuid_jsonl_name`）以及固定的 `subagents/agent-*.jsonl`。

mod full;
mod signature;

#[cfg(test)]
mod tests;

pub use full::discover_claude_full_device_with_progress;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use loki_metis_core::{CoverageReport, RootCandidateEvidence};

use super::super::discovery::{path_key, stable_id};
use super::super::{
    CancellationToken, DiscoveryMethod, FullDiscoveryOptions, LocalPathStatus, RegisteredRoot,
    classify_local_path, is_obviously_network_path,
};
use signature::{
    Inspection, SignatureBudget, coverage_state, inspect_root, reject_link_components, safe_alias,
};

pub(crate) use signature::{
    Inspection as ClaudeRootInspection, SignatureBudget as ClaudeSignatureBudget,
    inspect_root as inspect_claude_root,
};

/// 汇总 Claude Code 快速发现的显式候选，不在构造阶段访问文件系统。
#[derive(Debug, Clone, Default)]
pub struct ClaudeDiscoveryInputs {
    /// 当前用户主目录；存在时只检查其 `.claude` 直接候选。
    pub home_dir: Option<PathBuf>,
    /// 当前进程生效的 `CLAUDE_CONFIG_DIR`。
    pub claude_config_dir: Option<PathBuf>,
    /// 仅属于 Claude Code 客户端的已登记根。
    pub registered_roots: Vec<RegisteredRoot>,
}

impl ClaudeDiscoveryInputs {
    /// 从进程环境生成候选；不会读取 `.credentials.json` 或其他文件。
    pub fn from_process_environment(registered_roots: Vec<RegisteredRoot>) -> Self {
        let home_dir = super::super::current_user_home();
        let claude_config_dir = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from);
        Self {
            home_dir,
            claude_config_dir,
            registered_roots,
        }
    }
}

/// 一个通过 Claude Code transcript 签名确认的数据根。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeDiscoveredRoot {
    /// 仅 backend 使用的规范化访问路径。
    pub path: PathBuf,
    /// 按规范化访问位置生成或从 registry 恢复的稳定根 ID。
    pub root_id: String,
    /// 不包含完整绝对路径的展示别名。
    pub alias: String,
    /// 根进入发现流程的受控入口。
    pub discovery_method: DiscoveryMethod,
    /// 通过严格签名时实际命中的 transcript 结构类型。
    pub evidence: RootCandidateEvidence,
}

/// Claude Code 发现结果；字段与 Codex 覆盖模型一致，但根类型不可混用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeDiscoveryResult {
    /// 本次确认的发现根。
    pub roots: Vec<ClaudeDiscoveredRoot>,
    /// 已确认不再合格、应从 registry 移除的根 ID。
    pub confirmed_invalid_root_ids: Vec<String>,
    /// 未能得出确定结论、应保留旧登记的根 ID。
    pub unconfirmed_root_ids: Vec<String>,
    /// 本次发现的覆盖结论。
    pub coverage: CoverageReport,
    /// 本次发现遍历的目录数。
    pub directories_scanned: u64,
    /// 因符号链接跳过的计数。
    pub symlink_skipped_count: u64,
    /// 因网络路径跳过的计数。
    pub network_skipped_count: u64,
}

/// 待结构签名探测的候选根，携带其已知身份（若为已登记根）与发现方式。
struct Candidate {
    /// 若为已登记根，其既有稳定根 ID。
    existing_root_id: Option<String>,
    /// 候选访问路径。
    path: PathBuf,
    /// 候选的安全展示别名。
    alias: String,
    /// 候选的发现方式。
    method: DiscoveryMethod,
}

/// 只快速检查默认、当前环境与已启用登记根，不递归搜索其他位置。
pub fn discover_claude_quick(
    inputs: &ClaudeDiscoveryInputs,
    cancellation: &CancellationToken,
) -> ClaudeDiscoveryResult {
    let candidates = quick_candidates(inputs);
    let options = FullDiscoveryOptions::default();
    inspect_candidates(candidates, cancellation, &options)
}

/// 用户已登记根优先消费共享签名预算；环境与默认自动候选随后补充。
fn quick_candidates(inputs: &ClaudeDiscoveryInputs) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    for root in &inputs.registered_roots {
        if root.enabled {
            candidates.push(Candidate {
                existing_root_id: root.root_id.clone(),
                path: root.path.clone(),
                alias: root.alias.clone(),
                method: DiscoveryMethod::Registered,
            });
        }
    }
    if let Some(config_dir) = &inputs.claude_config_dir {
        candidates.push(Candidate {
            existing_root_id: None,
            path: config_dir.clone(),
            alias: safe_alias(config_dir),
            method: DiscoveryMethod::Environment,
        });
    }
    if let Some(home_dir) = &inputs.home_dir {
        let path = home_dir.join(".claude");
        candidates.push(Candidate {
            existing_root_id: None,
            alias: safe_alias(&path),
            path,
            method: DiscoveryMethod::DefaultHome,
        });
    }
    candidates
}

/// 对每个候选做结构签名探测，汇总为最终发现结果与覆盖统计。
fn inspect_candidates(
    candidates: Vec<Candidate>,
    cancellation: &CancellationToken,
    options: &FullDiscoveryOptions,
) -> ClaudeDiscoveryResult {
    let candidate_count = u64::try_from(candidates.len()).unwrap_or(u64::MAX);
    let existing_ids = candidates
        .iter()
        .filter_map(|candidate| candidate.existing_root_id.clone())
        .collect::<BTreeSet<_>>();
    let mut roots = BTreeMap::<PathBuf, ClaudeDiscoveredRoot>::new();
    let mut confirmed_invalid_ids = BTreeSet::new();
    let mut unconfirmed_ids = BTreeSet::new();
    let mut confirmed_invalid_paths = BTreeSet::new();
    let mut unconfirmed_paths = BTreeSet::new();
    let mut inspected_paths = BTreeSet::new();
    let mut permission_denied_count = 0_u64;
    let mut skipped_count = 0_u64;
    let mut symlink_skipped_count = 0_u64;
    let mut network_skipped_count = 0_u64;
    let mut cancelled = false;
    let mut budget_exhausted = false;
    let mut budget = SignatureBudget::new(options, cancellation);

    for candidate in candidates {
        if cancellation.is_cancelled() {
            cancelled = true;
            break;
        }
        if is_obviously_network_path(&candidate.path) {
            network_skipped_count = network_skipped_count.saturating_add(1);
            skipped_count = skipped_count.saturating_add(1);
            if let Some(id) = candidate.existing_root_id {
                confirmed_invalid_ids.insert(id);
            }
            continue;
        }
        match reject_link_components(&candidate.path) {
            Ok(true) => {
                symlink_skipped_count = symlink_skipped_count.saturating_add(1);
                skipped_count = skipped_count.saturating_add(1);
                if let Some(id) = candidate.existing_root_id {
                    confirmed_invalid_ids.insert(id);
                }
                continue;
            }
            Ok(false) => {}
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                permission_denied_count = permission_denied_count.saturating_add(1);
                if let Some(id) = candidate.existing_root_id {
                    unconfirmed_ids.insert(id);
                }
                continue;
            }
            Err(_) => {
                skipped_count = skipped_count.saturating_add(1);
                if let Some(id) = candidate.existing_root_id {
                    unconfirmed_ids.insert(id);
                }
                continue;
            }
        }
        match classify_local_path(&candidate.path) {
            LocalPathStatus::ConfirmedLocal => {}
            LocalPathStatus::RejectedNetwork => {
                network_skipped_count = network_skipped_count.saturating_add(1);
                skipped_count = skipped_count.saturating_add(1);
                if let Some(id) = candidate.existing_root_id {
                    confirmed_invalid_ids.insert(id);
                }
                continue;
            }
            LocalPathStatus::Missing => {
                // 缺少默认 `.claude` 是普通未使用状态，不应污染一个有效
                // `CLAUDE_CONFIG_DIR` 的覆盖结论。
                if candidate
                    .method
                    .counts_as_coverage_gap(candidate.existing_root_id.is_some())
                {
                    skipped_count = skipped_count.saturating_add(1);
                }
                if let Some(id) = candidate.existing_root_id {
                    confirmed_invalid_ids.insert(id);
                }
                continue;
            }
            LocalPathStatus::Indeterminate => {
                skipped_count = skipped_count.saturating_add(1);
                if let Some(id) = candidate.existing_root_id {
                    unconfirmed_ids.insert(id);
                }
                continue;
            }
        }
        let normalized = match fs::canonicalize(&candidate.path) {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if candidate
                    .method
                    .counts_as_coverage_gap(candidate.existing_root_id.is_some())
                {
                    skipped_count = skipped_count.saturating_add(1);
                }
                if let Some(id) = candidate.existing_root_id {
                    confirmed_invalid_ids.insert(id);
                }
                continue;
            }
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                permission_denied_count = permission_denied_count.saturating_add(1);
                if let Some(id) = candidate.existing_root_id {
                    unconfirmed_ids.insert(id);
                }
                continue;
            }
            Err(_) => {
                skipped_count = skipped_count.saturating_add(1);
                if let Some(id) = candidate.existing_root_id {
                    unconfirmed_ids.insert(id);
                }
                continue;
            }
        };
        if !inspected_paths.insert(normalized.clone()) {
            if let Some(root) = roots.get_mut(&normalized) {
                if candidate.method.priority() > root.discovery_method.priority() {
                    root.alias = candidate.alias;
                    root.discovery_method = candidate.method;
                }
                if candidate.method == DiscoveryMethod::Registered
                    && let Some(id) = candidate.existing_root_id
                {
                    root.root_id = id;
                }
            } else if confirmed_invalid_paths.contains(&normalized) {
                if let Some(id) = candidate.existing_root_id {
                    confirmed_invalid_ids.insert(id);
                }
            } else if unconfirmed_paths.contains(&normalized)
                && let Some(id) = candidate.existing_root_id
            {
                unconfirmed_ids.insert(id);
            }
            continue;
        }

        match inspect_root(&normalized, &mut budget) {
            Inspection::Found(evidence) => {
                let root_id = candidate
                    .existing_root_id
                    .unwrap_or_else(|| stable_id("claude-root", &path_key(&normalized)));
                roots.insert(
                    normalized.clone(),
                    ClaudeDiscoveredRoot {
                        path: normalized,
                        root_id,
                        alias: candidate.alias,
                        discovery_method: candidate.method,
                        evidence,
                    },
                );
            }
            Inspection::NotRoot | Inspection::Rejected => {
                if candidate
                    .method
                    .counts_as_coverage_gap(candidate.existing_root_id.is_some())
                {
                    skipped_count = skipped_count.saturating_add(1);
                }
                confirmed_invalid_paths.insert(normalized);
                if let Some(id) = candidate.existing_root_id {
                    confirmed_invalid_ids.insert(id);
                }
            }
            Inspection::Indeterminate => {
                skipped_count = skipped_count.saturating_add(1);
                unconfirmed_paths.insert(normalized);
                if let Some(id) = candidate.existing_root_id {
                    unconfirmed_ids.insert(id);
                }
            }
            Inspection::Cancelled => {
                cancelled = true;
                break;
            }
            Inspection::BudgetExhausted => {
                budget_exhausted = true;
                skipped_count = skipped_count.saturating_add(1);
                if let Some(id) = candidate.existing_root_id {
                    unconfirmed_ids.insert(id);
                }
                break;
            }
        }
    }

    if budget_exhausted {
        for id in existing_ids {
            let confirmed = roots.values().any(|root| root.root_id == id)
                || confirmed_invalid_ids.contains(&id);
            if !confirmed {
                unconfirmed_ids.insert(id);
            }
        }
    }
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
            roots_scanned: candidate_count,
            roots_discovered: u64::try_from(roots.len()).unwrap_or(u64::MAX),
            permission_denied_count,
            skipped_count,
            warning_count: 0,
        },
        roots,
        confirmed_invalid_root_ids: confirmed_invalid_ids.into_iter().collect(),
        unconfirmed_root_ids: unconfirmed_ids.into_iter().collect(),
        directories_scanned: 0,
        symlink_skipped_count,
        network_skipped_count,
    }
}
