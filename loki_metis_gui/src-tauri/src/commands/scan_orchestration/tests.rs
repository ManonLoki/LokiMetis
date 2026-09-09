use super::*;
use std::path::PathBuf;

#[cfg(unix)]
use crate::backend::local_index::RegisterDiscoveredRoot;
use crate::backend::local_index::{LocalError, LocalErrorKind};
use loki_metis_core::LocalScanErrorCategory;
#[cfg(unix)]
use tauri::async_runtime::block_on;

/// 测试辅助：用真实的路径匹配规则为发现根重新绑定已登记身份。
fn rebind_discovered_root_identity_for_local_scan_tests<Discovered, Registered>(
    discovered_roots: &mut [Discovered],
    registered_roots: &[Registered],
) where
    Discovered: DiscoveredRootIdentity,
    Registered: RegisteredRootIdentity,
{
    reuse_registered_root_identities(discovered_roots, registered_roots, |registered, root| {
        crate::backend::local_index::registered_path_matches_candidate(
            registered.path(),
            root.path(),
        )
        .unwrap_or(false)
    });
}

/// 验证主动全设备发现使用已分类卷、保留跳过计数且生产默认不限数量。
#[test]
fn keeps_full_device_discovery_unlimited_and_excludes_mount_entries() {
    let options = full_device_options_from_local_volume_roots(
        crate::backend::local_index::LocalVolumeRoots {
            search_roots: vec![PathBuf::from("/synthetic-local")],
            excluded_roots: vec![PathBuf::from("/synthetic-network")],
            network_skipped_count: 1,
            other_skipped_count: 2,
        },
    );

    assert_eq!(options.max_directories, u64::MAX);
    assert_eq!(options.max_entries, u64::MAX);
    assert_eq!(options.max_traversal_batches, 1);
    assert_eq!(options.max_signature_entries, u64::MAX);
    assert!(!options.allow_network_like_paths);
    assert_eq!(options.search_roots, [PathBuf::from("/synthetic-local")]);
    assert!(
        options
            .excluded_roots
            .contains(&PathBuf::from("/synthetic-network"))
    );
    assert_eq!(options.preflight_network_skipped_count, 1);
    assert_eq!(options.preflight_other_skipped_count, 2);
    #[cfg(target_os = "macos")]
    assert!(options.excluded_roots.contains(&PathBuf::from("/Volumes")));
    #[cfg(target_os = "macos")]
    assert!(options.excluded_roots.contains(&PathBuf::from("/System")));
    #[cfg(target_os = "macos")]
    assert!(
        options
            .excluded_roots
            .contains(&PathBuf::from("/synthetic-local/System"))
    );
    #[cfg(target_os = "linux")]
    assert!(options.excluded_roots.contains(&PathBuf::from("/proc")));
    #[cfg(target_os = "windows")]
    assert!(
        options
            .excluded_roots
            .contains(&PathBuf::from("/synthetic-local\\Windows"))
            || options
                .excluded_roots
                .contains(&PathBuf::from(r"/synthetic-local\Windows"))
            || options.excluded_roots.iter().any(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.eq_ignore_ascii_case("Windows"))
                    && path
                        .parent()
                        .is_some_and(|parent| parent.ends_with("synthetic-local"))
            })
    );
}

/// Claude 扫描错误必须保留稳定类别，但不能带出 adapter 原始说明或路径。
#[test]
fn local_scan_errors_keep_sanitized_stable_categories() {
    let cases = [
        (
            LocalErrorKind::InvalidPath,
            LocalScanErrorCategory::InvalidPath,
        ),
        (
            LocalErrorKind::PermissionDenied,
            LocalScanErrorCategory::PermissionDenied,
        ),
        (
            LocalErrorKind::SourceUnavailable,
            LocalScanErrorCategory::SourceUnavailable,
        ),
        (
            LocalErrorKind::UnsupportedSchema,
            LocalScanErrorCategory::UnsupportedSchema,
        ),
        (LocalErrorKind::Database, LocalScanErrorCategory::Database),
        (
            LocalErrorKind::InvalidUsage,
            LocalScanErrorCategory::InvalidUsage,
        ),
        (LocalErrorKind::ScanBusy, LocalScanErrorCategory::ScanBusy),
        (LocalErrorKind::Overflow, LocalScanErrorCategory::Overflow),
    ];
    for (kind, expected_category) in cases {
        let message =
            local_scan_error_message_from_local(LocalError::new(kind, "/private/sql bait"));
        assert!(message.contains(expected_category.label()));
        assert!(message.contains("原始记录未被修改"));
        assert!(!message.contains("/private"));
        assert!(!message.contains("sql bait"));
    }
}

/// 已在入口取消的 worker 不得打开 SQLite，因此不会触发 schema adopt 或迁移写入。
#[tokio::test]
async fn cancelled_worker_has_no_database_side_effect_before_returning() {
    let temp = tempfile::tempdir().expect("isolated app-data is available");
    let cancellation = ScanCancellation::new();
    cancellation.cancel();

    let error = run::run_scan::<CodexClient>(
        temp.path().to_path_buf(),
        temp.path().to_path_buf(),
        None,
        ScanKind::Quick,
        ScanStartOrigin::ExplicitUser,
        cancellation,
        Box::new(|_| {}),
    )
    .await
    .expect_err("cancelled worker returns before opening the index");

    assert_eq!(error, loki_metis_core::scan_cancelled_status_message());
    assert!(
        !loki_metis_core::source_client_usage_index_path(
            temp.path(),
            loki_metis_core::SourceClientKind::Codex,
        )
        .exists()
    );
}

/// 验证合并覆盖时不会用少量已发现根覆盖卷级主动搜索范围。
#[test]
fn combined_coverage_preserves_larger_discovery_scope() {
    let discovery = CoverageReport {
        state: CoverageState::Complete,
        roots_scanned: 5,
        roots_discovered: 1,
        permission_denied_count: 1,
        skipped_count: 2,
        warning_count: 0,
    };
    let scan = CoverageReport {
        state: CoverageState::Partial,
        roots_scanned: 1,
        roots_discovered: 1,
        permission_denied_count: 2,
        skipped_count: 3,
        warning_count: 4,
    };

    let combined = merge_coverage_reports(discovery, &scan);

    assert_eq!(combined.state, CoverageState::Partial);
    assert_eq!(combined.roots_scanned, 5);
    assert_eq!(combined.roots_discovered, 1);
    assert_eq!(combined.permission_denied_count, 3);
    assert_eq!(combined.skipped_count, 5);
    assert_eq!(combined.warning_count, 4);
}

/// 验证全设备发现合并时保留已记忆根的精确 ID，并传播其不确定覆盖与失效集合。
#[test]
fn full_device_merge_preserves_validated_known_root_identity() {
    let known_root = crate::backend::local_index::DiscoveredRoot {
        path: PathBuf::from("/synthetic/known"),
        root_id: "root-known-exact".to_owned(),
        alias: "已记忆根".to_owned(),
        discovery_method: crate::backend::local_index::DiscoveryMethod::Registered,
        has_sessions: true,
        sessions_inspection_complete: true,
        has_archived_sessions: false,
        archived_sessions_inspection_complete: true,
    };
    let full_duplicate = crate::backend::local_index::DiscoveredRoot {
        root_id: "root-recomputed".to_owned(),
        alias: "全设备候选".to_owned(),
        discovery_method: crate::backend::local_index::DiscoveryMethod::FullDevice,
        ..known_root.clone()
    };
    let known_validation = DiscoveryResult {
        roots: vec![known_root],
        confirmed_invalid_root_ids: vec!["root-invalid".to_owned()],
        unconfirmed_root_ids: vec!["root-uncertain".to_owned()],
        coverage: CoverageReport {
            state: CoverageState::Partial,
            roots_scanned: 2,
            roots_discovered: 1,
            permission_denied_count: 0,
            skipped_count: 1,
            warning_count: 0,
        },
        directories_scanned: 0,
        symlink_skipped_count: 0,
        network_skipped_count: 0,
    };
    let full_discovery = DiscoveryResult {
        roots: vec![full_duplicate],
        confirmed_invalid_root_ids: Vec::new(),
        unconfirmed_root_ids: Vec::new(),
        coverage: CoverageReport {
            state: CoverageState::Complete,
            roots_scanned: 10,
            roots_discovered: 1,
            permission_denied_count: 0,
            skipped_count: 0,
            warning_count: 0,
        },
        directories_scanned: 10,
        symlink_skipped_count: 0,
        network_skipped_count: 0,
    };

    let merged = merge_discovery_results(known_validation, full_discovery);

    assert_eq!(merged.roots.len(), 1);
    assert_eq!(merged.roots[0].root_id, "root-known-exact");
    assert_eq!(merged.roots[0].alias, "已记忆根");
    assert_eq!(
        merged.confirmed_invalid_root_ids,
        ["root-invalid".to_owned()]
    );
    assert_eq!(merged.unconfirmed_root_ids, ["root-uncertain".to_owned()]);
    assert_eq!(merged.coverage.state, CoverageState::Partial);
    assert_eq!(merged.coverage.roots_scanned, 10);
    assert_eq!(merged.directories_scanned, 10);
}

/// 验证主动发现复用特殊路径的已停用旧 ID，不会插入新启用根绕过用户选择。
#[cfg(unix)]
#[test]
fn full_discovery_reuses_disabled_legacy_identity() {
    let temp = tempfile::tempdir().expect("temporary app data is available");
    let special_path = temp.path().join("codex\\legacy");
    let legacy_root = crate::backend::local_index::DiscoveredRoot {
        path: special_path.clone(),
        root_id: "root-legacy-disabled".to_owned(),
        alias: "已停用特殊根".to_owned(),
        discovery_method: crate::backend::local_index::DiscoveryMethod::Registered,
        has_sessions: true,
        sessions_inspection_complete: true,
        has_archived_sessions: false,
        archived_sessions_inspection_complete: true,
    };
    let mut index = block_on(LocalIndex::open_in_app_data(
        temp.path(),
        SourceClientKind::Codex.parser_version(),
    ))
    .expect("isolated local index opens");
    block_on(index.register_root(&legacy_root)).expect("legacy root is registered");
    block_on(index.set_root_enabled(&legacy_root.root_id, false)).expect("legacy root is disabled");

    let mut discovery = DiscoveryResult {
        roots: vec![crate::backend::local_index::DiscoveredRoot {
            root_id: "root-new-lossless-key".to_owned(),
            alias: "重新发现候选".to_owned(),
            discovery_method: crate::backend::local_index::DiscoveryMethod::FullDevice,
            ..legacy_root.clone()
        }],
        confirmed_invalid_root_ids: Vec::new(),
        unconfirmed_root_ids: Vec::new(),
        coverage: CoverageReport {
            state: CoverageState::Complete,
            roots_scanned: 1,
            roots_discovered: 1,
            permission_denied_count: 0,
            skipped_count: 0,
            warning_count: 0,
        },
        directories_scanned: 1,
        symlink_skipped_count: 0,
        network_skipped_count: 0,
    };

    let all_roots = block_on(index.all_roots()).expect("disabled root identity loads");
    rebind_discovered_root_identity_for_local_scan_tests(&mut discovery.roots, &all_roots);
    block_on(index.register_root(&discovery.roots[0]))
        .expect("rediscovered root reuses existing row");
    let records = block_on(index.list_sources()).expect("source records load");

    assert_eq!(discovery.roots[0].root_id, "root-legacy-disabled");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].root_id, "root-legacy-disabled");
    assert!(!records[0].enabled);
}

/// 验证完全扫描以同一物理目录的词法别名返回时仍复用历史 ID 与用户别名。
#[test]
fn full_discovery_reuses_physical_alias_identity() {
    let temp = tempfile::tempdir().expect("temporary app data is available");
    let root_path = temp.path().join("physical-root");
    std::fs::create_dir_all(&root_path).expect("physical root is created");
    let registered = RegisteredRoot {
        root_id: Some("root-existing-physical".to_owned()),
        path: root_path.clone(),
        alias: "历史物理根".to_owned(),
        enabled: false,
    };
    let mut roots = vec![crate::backend::local_index::DiscoveredRoot {
        path: root_path.join("..").join("physical-root"),
        root_id: "root-new-path-key".to_owned(),
        alias: "重新发现别名".to_owned(),
        discovery_method: crate::backend::local_index::DiscoveryMethod::FullDevice,
        has_sessions: true,
        sessions_inspection_complete: true,
        has_archived_sessions: false,
        archived_sessions_inspection_complete: true,
    }];

    rebind_discovered_root_identity_for_local_scan_tests(&mut roots, &[registered]);

    assert_eq!(roots[0].root_id, "root-existing-physical");
    assert_eq!(roots[0].alias, "历史物理根");
}

/// 验证已有历史物理重复时精确词法路径优先，不会被排序更早的 alias 抢占身份。
#[test]
fn exact_registered_path_wins_before_physical_aliases() {
    let temp = tempfile::tempdir().expect("temporary root is available");
    let root_path = temp.path().join("identity-root");
    std::fs::create_dir_all(&root_path).expect("physical root is created");
    let physical_alias = RegisteredRoot {
        root_id: Some("root-0000-alias".to_owned()),
        path: root_path.join("..").join("identity-root"),
        alias: "历史物理别名".to_owned(),
        enabled: false,
    };
    let exact = RegisteredRoot {
        root_id: Some("root-ffff-exact".to_owned()),
        path: root_path.clone(),
        alias: "精确路径身份".to_owned(),
        enabled: true,
    };
    let mut roots = vec![crate::backend::local_index::DiscoveredRoot {
        path: root_path,
        root_id: "root-new".to_owned(),
        alias: "重新发现".to_owned(),
        discovery_method: crate::backend::local_index::DiscoveryMethod::FullDevice,
        has_sessions: true,
        sessions_inspection_complete: true,
        has_archived_sessions: false,
        archived_sessions_inspection_complete: true,
    }];

    rebind_discovered_root_identity_for_local_scan_tests(&mut roots, &[physical_alias, exact]);

    assert_eq!(roots[0].root_id, "root-ffff-exact");
    assert_eq!(roots[0].alias, "精确路径身份");
}

/// 验证没有精确路径且存在多个历史物理 alias 时，以最小稳定 ID 确定性复用一行。
#[test]
fn multiple_physical_aliases_use_a_deterministic_identity() {
    let temp = tempfile::tempdir().expect("temporary root is available");
    let root_path = temp.path().join("identity-root");
    std::fs::create_dir_all(&root_path).expect("physical root is created");
    let first_alias = RegisteredRoot {
        root_id: Some("root-z-legacy".to_owned()),
        path: root_path.join("..").join("identity-root"),
        alias: "较大 ID".to_owned(),
        enabled: false,
    };
    let second_alias = RegisteredRoot {
        root_id: Some("root-a-legacy".to_owned()),
        path: root_path
            .join("..")
            .join("identity-root")
            .join("..")
            .join("identity-root"),
        alias: "较小 ID".to_owned(),
        enabled: true,
    };
    let discovered_path = std::fs::canonicalize(&root_path).expect("root canonicalizes");
    let mut roots = vec![crate::backend::local_index::DiscoveredRoot {
        path: discovered_path,
        root_id: "root-new".to_owned(),
        alias: "重新发现".to_owned(),
        discovery_method: crate::backend::local_index::DiscoveryMethod::FullDevice,
        has_sessions: true,
        sessions_inspection_complete: true,
        has_archived_sessions: false,
        archived_sessions_inspection_complete: true,
    }];

    rebind_discovered_root_identity_for_local_scan_tests(&mut roots, &[first_alias, second_alias]);

    assert_eq!(roots[0].root_id, "root-a-legacy");
    assert_eq!(roots[0].alias, "较小 ID");
}

/// 验证历史 registry 路径变成链接后不再有资格向安全主动发现候选传播旧身份。
#[cfg(unix)]
#[test]
fn unsafe_historical_link_is_not_reused_by_discovery() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("temporary root is available");
    let candidate = temp.path().join("safe-candidate");
    std::fs::create_dir_all(&candidate).expect("safe candidate is created");
    let historical_link = temp.path().join("historical-link");
    symlink(&candidate, &historical_link).expect("historical link is created");
    let registered = RegisteredRoot {
        root_id: Some("root-unsafe-history".to_owned()),
        path: historical_link,
        alias: "不应复用".to_owned(),
        enabled: false,
    };
    let mut roots = vec![crate::backend::local_index::DiscoveredRoot {
        path: std::fs::canonicalize(candidate).expect("candidate canonicalizes"),
        root_id: "root-safe-new".to_owned(),
        alias: "安全候选".to_owned(),
        discovery_method: crate::backend::local_index::DiscoveryMethod::FullDevice,
        has_sessions: true,
        sessions_inspection_complete: true,
        has_archived_sessions: false,
        archived_sessions_inspection_complete: true,
    }];

    rebind_discovered_root_identity_for_local_scan_tests(&mut roots, &[registered]);

    assert_eq!(roots[0].root_id, "root-safe-new");
    assert_eq!(roots[0].alias, "安全候选");
}
