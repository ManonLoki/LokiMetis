//! 扫描编排中的共享纯业务规则：已知根校验与主动发现结果的合并。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::{CoverageReport, ScanKind};

/// 表示可复用的发现根身份（如 root_id 和 alias）。
pub trait DiscoveredRootIdentity {
    /// 返回根所在路径。用于按路径聚合与候选匹配。
    fn path(&self) -> &Path;
    /// 返回本轮登记、激活与完成状态使用的稳定根 ID。
    fn root_id(&self) -> &str;
    /// 使用历史登记数据覆盖本次候选的身份信息。
    fn adopt_identity(&mut self, root_id: &str, alias: &str);
}

/// 表示 registry 可持有的历史根身份。
pub trait RegisteredRootIdentity {
    /// 历史登记根路径。
    fn path(&self) -> &Path;
    /// 历史登记根 ID；仅当存在时可用于继承。
    fn root_id(&self) -> Option<&str>;
    /// 历史登记根展示别名。
    fn alias(&self) -> &str;
}

/// 一次数据根发现（快速重验证或全设备主动发现）的结果。
///
/// 各 Agent 的覆盖模型相同，只有根类型不同；泛型参数保证不同 Agent 的
/// 根不能混入同一结果。结果不包含绝对路径之外的用户内容，路径只留在根内部。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryOutcome<Root> {
    /// 本次确认符合该 Agent 结构签名的数据根。
    pub roots: Vec<Root>,
    /// 已完整检查且确认不再符合签名、应从 registry 移除的历史根精确 ID。
    pub confirmed_invalid_root_ids: Vec<String>,
    /// 因权限、I/O 或预算未能得出结论、应保留旧登记的历史根精确 ID。
    pub unconfirmed_root_ids: Vec<String>,
    /// 使用 core 稳定类型表达的覆盖结论。
    pub coverage: CoverageReport,
    /// 主动发现实际读取过目录项的目录数量。
    pub directories_scanned: u64,
    /// 因符号链接策略跳过的路径数量。
    pub symlink_skipped_count: u64,
    /// 因默认网络卷排除策略跳过的路径数量。
    pub network_skipped_count: u64,
}

/// 在全设备发现阶段，优先复用历史登记根的稳定身份（包含停用项）。
pub fn reuse_registered_root_identities<R, Registered, MatchPath>(
    roots: &mut [R],
    registered_roots: &[Registered],
    matches_candidate: MatchPath,
) where
    R: DiscoveredRootIdentity,
    Registered: RegisteredRootIdentity,
    MatchPath: Fn(&Registered, &R) -> bool,
{
    for root in roots {
        let exact_match = registered_roots
            .iter()
            .filter(|registered| registered.path() == root.path())
            .min_by(|left, right| left.root_id().cmp(&right.root_id()));

        let physical_match = registered_roots
            .iter()
            .filter(|registered| matches_candidate(registered, root));

        let registered_root = exact_match
            .or_else(|| physical_match.min_by(|left, right| left.root_id().cmp(&right.root_id())));

        let Some(registered_root) = registered_root else {
            continue;
        };

        if let Some(root_id) = registered_root.root_id() {
            root.adopt_identity(root_id, registered_root.alias());
        }
    }
}

/// 合并“已知有效验证结果”与“主动全设备发现结果”。
///
/// 同一路径以已知验证结果为准；失效/待确认 ID 去重后保持有序；遍历计数饱和相加，
/// `roots_discovered` 以合并后的根数量为准。
pub fn merge_discovery_results<Root: DiscoveredRootIdentity>(
    known_validation: DiscoveryOutcome<Root>,
    full_discovery: DiscoveryOutcome<Root>,
) -> DiscoveryOutcome<Root> {
    let mut merged_roots = full_discovery
        .roots
        .into_iter()
        .map(|root| (root.path().to_path_buf(), root))
        .collect::<BTreeMap<_, _>>();
    for root in known_validation.roots {
        merged_roots.insert(root.path().to_path_buf(), root);
    }

    let confirmed_invalid_root_ids = known_validation
        .confirmed_invalid_root_ids
        .into_iter()
        .chain(full_discovery.confirmed_invalid_root_ids)
        .collect::<BTreeSet<_>>();
    let unconfirmed_root_ids = known_validation
        .unconfirmed_root_ids
        .into_iter()
        .chain(full_discovery.unconfirmed_root_ids)
        .collect::<BTreeSet<_>>();

    let mut merged_coverage =
        crate::merge_coverage_reports(known_validation.coverage, &full_discovery.coverage);
    merged_coverage.roots_discovered = u64::try_from(merged_roots.len()).unwrap_or(u64::MAX);

    DiscoveryOutcome {
        roots: merged_roots.into_values().collect(),
        confirmed_invalid_root_ids: confirmed_invalid_root_ids.into_iter().collect(),
        unconfirmed_root_ids: unconfirmed_root_ids.into_iter().collect(),
        coverage: merged_coverage,
        directories_scanned: known_validation
            .directories_scanned
            .saturating_add(full_discovery.directories_scanned),
        symlink_skipped_count: known_validation
            .symlink_skipped_count
            .saturating_add(full_discovery.symlink_skipped_count),
        network_skipped_count: known_validation
            .network_skipped_count
            .saturating_add(full_discovery.network_skipped_count),
    }
}

/// 按扫描模式返回可直接用于索引的发现结果。
///
/// Quick 与 FullDevice 都先用包含停用项的历史 registry 回填已知根身份，避免
/// 默认/环境自动候选绕过用户禁用、别名或主根选择；FullDevice 再对主动发现
/// 结果执行同样回填并合并。未提供全量发现结果时回退为已知结果。
pub fn scan_discovery_for_scan_kind<Root, Registered, MatchPath>(
    kind: ScanKind,
    mut known_validation: DiscoveryOutcome<Root>,
    full_discovery: Option<DiscoveryOutcome<Root>>,
    registered_roots: &[Registered],
    matches_candidate: MatchPath,
) -> DiscoveryOutcome<Root>
where
    Root: DiscoveredRootIdentity,
    Registered: RegisteredRootIdentity,
    MatchPath: Fn(&Registered, &Root) -> bool,
{
    reuse_registered_root_identities(
        &mut known_validation.roots,
        registered_roots,
        |registered, root| matches_candidate(registered, root),
    );
    match kind {
        ScanKind::Quick => known_validation,
        ScanKind::FullDevice => {
            let Some(mut full_discovery) = full_discovery else {
                return known_validation;
            };
            reuse_registered_root_identities(
                &mut full_discovery.roots,
                registered_roots,
                matches_candidate,
            );
            merge_discovery_results(known_validation, full_discovery)
        }
    }
}

/// 按已知活动注册根路径过滤可继续扫描的发现根。
pub fn discovery_roots_for_active_registration<Discovered, Registered>(
    discovered_roots: &[Discovered],
    active_registered_roots: &[Registered],
) -> Vec<Discovered>
where
    Discovered: DiscoveredRootIdentity + Clone,
    Registered: RegisteredRootIdentity,
{
    discovered_roots
        .iter()
        .filter(|discovered_root| {
            active_registered_roots
                .iter()
                .any(|registered_root| registered_root.path() == discovered_root.path())
        })
        .cloned()
        .collect()
}

/// 从严格重验证判定失效的根中排除本轮刚认领的首次验证候选。
///
/// 已就绪历史根确认失去签名后可以移除；首次候选必须继续完成激活状态
/// 收口，以便稳定展示为 `ValidationFailed`，不能在用户看到后直接消失。
pub fn confirmed_invalid_roots_to_remove(
    confirmed_invalid_root_ids: &[String],
    pending_validation_root_ids: &[String],
) -> Vec<String> {
    confirmed_invalid_root_ids
        .iter()
        .filter(|root_id| !pending_validation_root_ids.contains(*root_id))
        .cloned()
        .collect()
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - orchestration_tests.rs：本模块的单元测试
include!("orchestration_tests.rs");
