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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[derive(Debug, Clone, PartialEq, Eq)]
    /// 表示扫描编排测试中的可复用发现根。
    struct TestDiscoveredRoot {
        path: PathBuf,
        root_id: String,
        alias: String,
    }

    impl DiscoveredRootIdentity for TestDiscoveredRoot {
        /// 返回测试发现根的本机路径。
        fn path(&self) -> &Path {
            &self.path
        }

        /// 返回测试发现根的稳定标识。
        fn root_id(&self) -> &str {
            &self.root_id
        }

        /// 采用登记库中的稳定标识与别名，模拟身份复用。
        fn adopt_identity(&mut self, root_id: &str, alias: &str) {
            self.root_id = root_id.to_owned();
            self.alias = alias.to_owned();
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    /// 保存测试发现结果及其扫描范围元数据。
    struct TestDiscoveryResult {
        roots: Vec<TestDiscoveredRoot>,
        confirmed_invalid_root_ids: Vec<String>,
        unconfirmed_root_ids: Vec<String>,
        coverage: CoverageReport,
        directories_scanned: u64,
        symlink_skipped_count: u64,
        network_skipped_count: u64,
    }

    impl ScanDiscoveryResult for TestDiscoveryResult {
        /// 使用测试发现根作为可登记来源类型。
        type Root = TestDiscoveredRoot;

        /// 将测试发现结果拆成范围、候选和错误部分。
        fn into_parts(
            self,
        ) -> (
            Vec<Self::Root>,
            Vec<String>,
            Vec<String>,
            CoverageReport,
            u64,
            u64,
            u64,
        ) {
            (
                self.roots,
                self.confirmed_invalid_root_ids,
                self.unconfirmed_root_ids,
                self.coverage,
                self.directories_scanned,
                self.symlink_skipped_count,
                self.network_skipped_count,
            )
        }

        /// 从编排后的部分重建测试发现结果。
        fn from_parts(
            roots: Vec<Self::Root>,
            confirmed_invalid_root_ids: Vec<String>,
            unconfirmed_root_ids: Vec<String>,
            coverage: CoverageReport,
            directories_scanned: u64,
            symlink_skipped_count: u64,
            network_skipped_count: u64,
        ) -> Self {
            Self {
                roots,
                confirmed_invalid_root_ids,
                unconfirmed_root_ids,
                coverage,
                directories_scanned,
                symlink_skipped_count,
                network_skipped_count,
            }
        }
    }

    #[derive(Debug, Clone)]
    /// 表示登记库中已有的数据根测试记录。
    struct TestRegisteredRoot {
        path: PathBuf,
        root_id: Option<String>,
        alias: String,
    }

    impl RegisteredRootIdentity for TestRegisteredRoot {
        /// 返回已登记测试根的本机路径。
        fn path(&self) -> &Path {
            &self.path
        }

        /// 返回已登记测试根的可选稳定标识。
        fn root_id(&self) -> Option<&str> {
            self.root_id.as_deref()
        }

        /// 返回已登记测试根的展示别名。
        fn alias(&self) -> &str {
            &self.alias
        }
    }

    #[test]
    /// 验证再次发现已登记路径时复用原稳定标识。
    fn reuses_registered_root_id_when_known_root_is_discovered_again() {
        let mut discovered = vec![TestDiscoveredRoot {
            path: PathBuf::from("/a"),
            root_id: "new-root".to_owned(),
            alias: "candidate".to_owned(),
        }];
        let registered = vec![TestRegisteredRoot {
            path: PathBuf::from("/a"),
            root_id: Some("old-root".to_owned()),
            alias: "历史".to_owned(),
        }];

        reuse_registered_root_identities(&mut discovered, &registered, |_registered, _root| false);

        assert_eq!(discovered[0].root_id, "old-root");
        assert_eq!(discovered[0].alias, "历史");
    }

    #[test]
    /// 验证快速扫描在重新启用前也复用禁用记录的身份。
    fn quick_scan_reuses_disabled_registry_identity_before_registration() {
        let quick = TestDiscoveryResult {
            roots: vec![TestDiscoveredRoot {
                path: PathBuf::from("/canonical/default"),
                root_id: "new-automatic-root".to_owned(),
                alias: ".codex".to_owned(),
            }],
            confirmed_invalid_root_ids: Vec::new(),
            unconfirmed_root_ids: Vec::new(),
            coverage: CoverageReport {
                state: crate::CoverageState::Complete,
                roots_scanned: 1,
                roots_discovered: 1,
                permission_denied_count: 0,
                skipped_count: 0,
                warning_count: 0,
            },
            directories_scanned: 0,
            symlink_skipped_count: 0,
            network_skipped_count: 0,
        };
        let disabled_history = vec![TestRegisteredRoot {
            path: PathBuf::from("/historical/physical-alias"),
            root_id: Some("disabled-root".to_owned()),
            alias: "用户保留别名".to_owned(),
        }];

        let restored = scan_discovery_for_scan_kind(
            ScanKind::Quick,
            quick,
            None,
            &disabled_history,
            |_registered, _root| true,
        );

        assert_eq!(restored.roots[0].root_id, "disabled-root");
        assert_eq!(restored.roots[0].alias, "用户保留别名");
    }

    #[test]
    /// 验证多批发现按路径合并且保留完全扫描范围。
    fn merges_scan_discovery_results_by_path_preserving_full_scope() {
        let known = TestDiscoveryResult {
            roots: vec![TestDiscoveredRoot {
                path: PathBuf::from("/known"),
                root_id: "known".to_owned(),
                alias: "known".to_owned(),
            }],
            confirmed_invalid_root_ids: vec!["bad".to_owned()],
            unconfirmed_root_ids: vec!["uncertain".to_owned()],
            coverage: CoverageReport {
                state: crate::CoverageState::Partial,
                roots_scanned: 2,
                roots_discovered: 1,
                permission_denied_count: 1,
                skipped_count: 2,
                warning_count: 0,
            },
            directories_scanned: 1,
            symlink_skipped_count: 0,
            network_skipped_count: 0,
        };
        let full = TestDiscoveryResult {
            roots: vec![TestDiscoveredRoot {
                path: PathBuf::from("/full"),
                root_id: "full".to_owned(),
                alias: "full".to_owned(),
            }],
            confirmed_invalid_root_ids: Vec::new(),
            unconfirmed_root_ids: Vec::new(),
            coverage: CoverageReport {
                state: crate::CoverageState::Complete,
                roots_scanned: 6,
                roots_discovered: 1,
                permission_denied_count: 0,
                skipped_count: 3,
                warning_count: 0,
            },
            directories_scanned: 3,
            symlink_skipped_count: 1,
            network_skipped_count: 1,
        };
        let merged = merge_discovery_results(known, full);

        assert_eq!(merged.roots.len(), 2);
        assert_eq!(merged.confirmed_invalid_root_ids, ["bad"]);
        assert_eq!(merged.unconfirmed_root_ids, ["uncertain"]);
        assert_eq!(merged.coverage.roots_discovered, 2);
        assert_eq!(merged.coverage.roots_scanned, 6);
        assert_eq!(merged.directories_scanned, 4);
        assert_eq!(merged.symlink_skipped_count, 1);
        assert_eq!(merged.network_skipped_count, 1);
    }

    #[test]
    /// 验证编排结果只保留具有活动登记路径的扫描目标。
    fn keeps_scan_targets_with_active_registration_paths() {
        let discovered = vec![
            TestDiscoveredRoot {
                path: PathBuf::from("/active"),
                root_id: "active".to_owned(),
                alias: "active".to_owned(),
            },
            TestDiscoveredRoot {
                path: PathBuf::from("/inactive"),
                root_id: "inactive".to_owned(),
                alias: "inactive".to_owned(),
            },
        ];
        let registered = vec![TestRegisteredRoot {
            path: PathBuf::from("/active"),
            root_id: Some("root-active".to_owned()),
            alias: "active-alias".to_owned(),
        }];

        let filtered = discovery_roots_for_active_registration(&discovered, &registered);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].root_id, "active");
    }

    /// 验证首次待验证候选不会混入历史失效根删除集合，既有失效根仍被移除。
    #[test]
    fn retains_pending_validation_roots_while_removing_invalid_history() {
        let invalid = ["ready-invalid".to_owned(), "pending-invalid".to_owned()];
        let pending = ["pending-invalid".to_owned()];

        assert_eq!(
            confirmed_invalid_roots_to_remove(&invalid, &pending),
            ["ready-invalid"]
        );
    }
}
