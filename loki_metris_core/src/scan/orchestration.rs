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

/// 拆解发现结果得到的各部分：发现根列表、失效根 ID、待确认根 ID、覆盖报告，
/// 以及已遍历目录数、跳过的符号链接数与跳过的网络路径数。
pub type DiscoveryResultParts<Root> = (
    Vec<Root>,
    Vec<String>,
    Vec<String>,
    CoverageReport,
    u64,
    u64,
    u64,
);

/// 表示可合并的发现结果载荷。
pub trait ScanDiscoveryResult: Sized {
    /// 该发现结果所使用的具体根身份类型。
    type Root: DiscoveredRootIdentity + Clone;

    /// 拆解为发现根列表、失效/待确认根 ID、覆盖报告与遍历计数，供合并逻辑消费。
    fn into_parts(self) -> DiscoveryResultParts<Self::Root>;

    /// 从合并后的各部分重新组装出具体发现结果类型。
    fn from_parts(
        roots: Vec<Self::Root>,
        confirmed_invalid_root_ids: Vec<String>,
        unconfirmed_root_ids: Vec<String>,
        coverage: CoverageReport,
        directories_scanned: u64,
        symlink_skipped_count: u64,
        network_skipped_count: u64,
    ) -> Self;
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
pub fn merge_discovery_results<T: ScanDiscoveryResult>(
    known_validation: T,
    full_discovery: T,
) -> T {
    let (known_roots, known_civ, known_unc, known_coverage, known_dirs, known_sym, known_net) =
        known_validation.into_parts();
    let (full_roots, full_civ, full_unc, full_coverage, full_dirs, full_sym, full_net) =
        full_discovery.into_parts();
    let mut merged_roots = full_roots
        .into_iter()
        .map(|root| (root.path().to_path_buf(), root))
        .collect::<BTreeMap<_, _>>();
    for root in known_roots {
        merged_roots.insert(root.path().to_path_buf(), root);
    }

    let confirmed_invalid_root_ids = known_civ
        .into_iter()
        .chain(full_civ)
        .collect::<BTreeSet<_>>();
    let unconfirmed_root_ids = known_unc
        .into_iter()
        .chain(full_unc)
        .collect::<BTreeSet<_>>();

    let mut merged_coverage = crate::merge_coverage_reports(known_coverage, &full_coverage);
    merged_coverage.roots_discovered = u64::try_from(merged_roots.len()).unwrap_or(u64::MAX);

    T::from_parts(
        merged_roots.into_values().collect(),
        confirmed_invalid_root_ids.into_iter().collect(),
        unconfirmed_root_ids.into_iter().collect(),
        merged_coverage,
        known_dirs.saturating_add(full_dirs),
        known_sym.saturating_add(full_sym),
        known_net.saturating_add(full_net),
    )
}

/// 按扫描模式返回可直接用于索引的发现结果。
///
/// Quick 与 FullDevice 都先用包含停用项的历史 registry 回填已知根身份，避免
/// 默认/环境自动候选绕过用户禁用、别名或主根选择；FullDevice 再对主动发现
/// 结果执行同样回填并合并。未提供全量发现结果时回退为已知结果。
pub fn scan_discovery_for_scan_kind<T, Registered, MatchPath>(
    kind: ScanKind,
    known_validation: T,
    full_discovery: Option<T>,
    registered_roots: &[Registered],
    matches_candidate: MatchPath,
) -> T
where
    T: ScanDiscoveryResult,
    Registered: RegisteredRootIdentity,
    MatchPath: Fn(&Registered, &T::Root) -> bool,
{
    let (
        mut known_roots,
        known_confirmed_invalid_root_ids,
        known_unconfirmed_root_ids,
        known_coverage,
        known_directories_scanned,
        known_symlink_skipped_count,
        known_network_skipped_count,
    ) = known_validation.into_parts();
    reuse_registered_root_identities(&mut known_roots, registered_roots, |registered, root| {
        matches_candidate(registered, root)
    });
    let known_validation = T::from_parts(
        known_roots,
        known_confirmed_invalid_root_ids,
        known_unconfirmed_root_ids,
        known_coverage,
        known_directories_scanned,
        known_symlink_skipped_count,
        known_network_skipped_count,
    );
    match kind {
        ScanKind::Quick => known_validation,
        ScanKind::FullDevice => {
            let Some(full_discovery) = full_discovery else {
                return known_validation;
            };
            let (
                mut full_roots,
                full_confirmed_invalid_root_ids,
                full_unconfirmed_root_ids,
                full_coverage,
                full_directories_scanned,
                full_symlink_skipped_count,
                full_network_skipped_count,
            ) = full_discovery.into_parts();
            reuse_registered_root_identities(&mut full_roots, registered_roots, matches_candidate);
            let restored_full_discovery = T::from_parts(
                full_roots,
                full_confirmed_invalid_root_ids,
                full_unconfirmed_root_ids,
                full_coverage,
                full_directories_scanned,
                full_symlink_skipped_count,
                full_network_skipped_count,
            );
            merge_discovery_results(known_validation, restored_full_discovery)
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
