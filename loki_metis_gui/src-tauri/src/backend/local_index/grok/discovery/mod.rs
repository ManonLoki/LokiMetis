//! Grok Build CLI 数据根发现：只承认 `sessions/**/updates.jsonl` 结构签名。

mod full;
mod signature;

pub use full::discover_grok_full_device_with_progress;

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
    Inspection as GrokRootInspection, SignatureBudget as GrokSignatureBudget,
    inspect_root as inspect_grok_root,
};

/// 汇总 Grok 快速发现的显式候选，不在构造阶段访问文件系统。
#[derive(Debug, Clone, Default)]
pub struct GrokDiscoveryInputs {
    /// 当前用户主目录；存在时只检查其 `.grok` 直接候选。
    pub home_dir: Option<PathBuf>,
    /// 当前进程生效的 `GROK_HOME`。
    pub grok_home: Option<PathBuf>,
    /// 仅属于 Grok 客户端的已登记根。
    pub registered_roots: Vec<RegisteredRoot>,
}

impl GrokDiscoveryInputs {
    /// 从进程环境生成候选；不会读取 `auth.json` 或其他文件。
    pub fn from_process_environment(registered_roots: Vec<RegisteredRoot>) -> Self {
        let home_dir = super::super::current_user_home();
        let grok_home = std::env::var_os("GROK_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);
        Self {
            home_dir,
            grok_home,
            registered_roots,
        }
    }
}

/// 一个通过 Grok 会话签名确认的数据根。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrokDiscoveredRoot {
    /// 仅 backend 使用的规范化访问路径。
    pub path: PathBuf,
    /// 按规范化访问位置生成或从 registry 恢复的稳定根 ID。
    pub root_id: String,
    /// 不包含完整绝对路径的展示别名。
    pub alias: String,
    /// 根进入发现流程的受控入口。
    pub discovery_method: DiscoveryMethod,
    /// 通过严格签名时实际命中的结构类型。
    pub evidence: RootCandidateEvidence,
}

/// Grok 发现结果；字段与 Codex/Claude 覆盖模型一致，但根类型不可混用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrokDiscoveryResult {
    /// 本次确认的发现根。
    pub roots: Vec<GrokDiscoveredRoot>,
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

/// 表示待核对的 Grok 本机根候选及其发现来源。
struct Candidate {
    existing_root_id: Option<String>,
    path: PathBuf,
    alias: String,
    method: DiscoveryMethod,
}

/// 只快速检查默认、当前环境与已启用登记根。
pub fn discover_grok_quick(
    inputs: &GrokDiscoveryInputs,
    cancellation: &CancellationToken,
) -> GrokDiscoveryResult {
    inspect_candidates(
        quick_candidates(inputs),
        cancellation,
        &FullDiscoveryOptions::default(),
    )
}

/// 从受信环境根与默认本机目录生成有界快速扫描候选。
fn quick_candidates(inputs: &GrokDiscoveryInputs) -> Vec<Candidate> {
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
    if let Some(grok_home) = &inputs.grok_home {
        candidates.push(Candidate {
            existing_root_id: None,
            path: grok_home.clone(),
            alias: safe_alias(grok_home),
            method: DiscoveryMethod::Environment,
        });
    }
    if let Some(home_dir) = &inputs.home_dir {
        let path = home_dir.join(".grok");
        candidates.push(Candidate {
            existing_root_id: None,
            alias: safe_alias(&path),
            path,
            method: DiscoveryMethod::DefaultHome,
        });
    }
    candidates
}

/// 逐个核对候选的严格 Grok 签名并汇总安全发现结果。
fn inspect_candidates(
    candidates: Vec<Candidate>,
    cancellation: &CancellationToken,
    options: &FullDiscoveryOptions,
) -> GrokDiscoveryResult {
    let existing_ids = candidates
        .iter()
        .filter_map(|candidate| candidate.existing_root_id.clone())
        .collect::<BTreeSet<_>>();
    let mut roots = BTreeMap::<PathBuf, GrokDiscoveredRoot>::new();
    let mut confirmed_invalid_ids = BTreeSet::new();
    let mut unconfirmed_ids = BTreeSet::new();
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
            if let Some(root) = roots.get_mut(&normalized)
                && candidate.method.priority() > root.discovery_method.priority()
            {
                root.alias = candidate.alias;
                root.discovery_method = candidate.method;
                if candidate.method == DiscoveryMethod::Registered
                    && let Some(id) = candidate.existing_root_id
                {
                    root.root_id = id;
                }
            }
            continue;
        }
        match inspect_root(&normalized, &mut budget) {
            Inspection::Found(evidence) => {
                let root_id = candidate
                    .existing_root_id
                    .unwrap_or_else(|| stable_id("grok-root", &path_key(&normalized)));
                roots.insert(
                    normalized.clone(),
                    GrokDiscoveredRoot {
                        path: normalized,
                        root_id,
                        alias: candidate.alias,
                        discovery_method: candidate.method,
                        evidence,
                    },
                );
            }
            Inspection::NotRoot | Inspection::Rejected => {
                if let Some(id) = candidate.existing_root_id {
                    confirmed_invalid_ids.insert(id);
                }
            }
            Inspection::Indeterminate => {
                if let Some(id) = candidate.existing_root_id {
                    unconfirmed_ids.insert(id);
                }
            }
            Inspection::Cancelled => {
                cancelled = true;
                if let Some(id) = candidate.existing_root_id {
                    unconfirmed_ids.insert(id);
                }
                break;
            }
            Inspection::BudgetExhausted => {
                budget_exhausted = true;
                if let Some(id) = candidate.existing_root_id {
                    unconfirmed_ids.insert(id);
                }
                break;
            }
        }
    }

    let _ = existing_ids;
    GrokDiscoveryResult {
        coverage: CoverageReport {
            state: coverage_state(
                cancelled,
                budget_exhausted,
                permission_denied_count,
                skipped_count,
            ),
            roots_scanned: u64::try_from(roots.len()).unwrap_or(u64::MAX),
            roots_discovered: u64::try_from(roots.len()).unwrap_or(u64::MAX),
            permission_denied_count,
            skipped_count,
            warning_count: 0,
        },
        roots: roots.into_values().collect(),
        confirmed_invalid_root_ids: confirmed_invalid_ids.into_iter().collect(),
        unconfirmed_root_ids: unconfirmed_ids.into_iter().collect(),
        directories_scanned: 0,
        symlink_skipped_count,
        network_skipped_count,
    }
}
