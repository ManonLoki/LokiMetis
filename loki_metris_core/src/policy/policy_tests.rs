use super::*;

/// 锁定各客户端当前 parser generation，避免 GUI writer 单独加一代而读取仍用旧值。
#[test]
fn parser_generation_matches_shipped_clients() {
    assert_eq!(SourceClientKind::Codex.parser_version(), 8);
    assert_eq!(SourceClientKind::ClaudeCode.parser_version(), 4);
    assert_eq!(SourceClientKind::GrokBuildCli.parser_version(), 3);
}

/// 验证首次初始化与后续启动都不会声称完整覆盖。
#[test]
fn provides_initial_empty_coverage() {
    assert_eq!(
        initial_coverage(),
        CoverageReport {
            state: CoverageState::Partial,
            roots_scanned: 0,
            roots_discovered: 0,
            permission_denied_count: 0,
            skipped_count: 0,
            warning_count: 0,
        }
    );
    assert_eq!(initial_coverage(), empty_coverage());
}

/// 验证扫描门禁只允许显式用户触发绕过初始化。
#[test]
fn denies_automatic_scan_before_initialization() {
    assert_eq!(
        ensure_scan_start_allowed(ScanStartOrigin::ExplicitUser, false, ScanKind::FullDevice),
        Ok(())
    );
    assert_eq!(
        ensure_scan_start_allowed(ScanStartOrigin::InitialAutomatic, false, ScanKind::Quick),
        Err(ScanStartAccessError::InitializationRequired)
    );
    assert_eq!(
        ensure_scan_start_allowed(ScanStartOrigin::PeriodicAutomatic, false, ScanKind::Quick),
        Err(ScanStartAccessError::InitializationRequired)
    );
    assert_eq!(
        ensure_scan_start_allowed(
            ScanStartOrigin::PeriodicAutomatic,
            true,
            ScanKind::FullDevice
        ),
        Err(ScanStartAccessError::AutomaticScansMustBeQuick)
    );
    assert_eq!(
        ensure_scan_start_allowed(ScanStartOrigin::InitialAutomatic, true, ScanKind::Quick),
        Ok(())
    );
}

#[test]
/// 验证首次扫描覆盖近 30 日，而周期扫描只处理当天增量。
fn scan_policy_bootstraps_thirty_days_then_limits_periodic_work_to_today() {
    let observed_at_epoch_ms = 1_775_000_000_000;
    let boundaries = WindowBoundaries::for_local_today(observed_at_epoch_ms);
    let empty_periodic = local_index_scan_policy(
        ScanStartOrigin::PeriodicAutomatic,
        false,
        observed_at_epoch_ms,
    );
    assert_eq!(empty_periodic.window, LocalIndexScanWindow::ThirtyDays);
    assert_eq!(empty_periodic.scan_since_epoch_ms, boundaries.thirty_days);

    let incremental = local_index_scan_policy(
        ScanStartOrigin::PeriodicAutomatic,
        true,
        observed_at_epoch_ms,
    );
    assert_eq!(incremental.window, LocalIndexScanWindow::Today);
    assert_eq!(incremental.scan_since_epoch_ms, boundaries.today);
    assert_eq!(incremental.retention_since_epoch_ms, boundaries.thirty_days);
}

#[test]
/// 验证显式与初始化扫描都保留近 30 日恢复窗口。
fn explicit_and_initial_scans_keep_the_thirty_day_recovery_window() {
    let observed_at_epoch_ms = 1_775_000_000_000;
    let boundaries = WindowBoundaries::for_local_today(observed_at_epoch_ms);
    for origin in [
        ScanStartOrigin::ExplicitUser,
        ScanStartOrigin::InitialAutomatic,
    ] {
        let policy = local_index_scan_policy(origin, true, observed_at_epoch_ms);
        assert_eq!(policy.window, LocalIndexScanWindow::ThirtyDays);
        assert_eq!(policy.scan_since_epoch_ms, boundaries.thirty_days);
    }
    assert_eq!(LOCAL_DISCOVERY_WORKER_LIMIT, 2);
    assert_eq!(LOCAL_INDEX_WORKER_LIMIT, 1);
}

/// 数据库或 parser 升级只在明确需要重建时触发启动后立即索引。
#[test]
fn only_needs_rescan_requires_immediate_upgrade_reindex() {
    use crate::{LocalIndexState, immediate_reindex_required};

    assert!(immediate_reindex_required(LocalIndexState::NeedsRescan));
    for state in [
        LocalIndexState::NotScanned,
        LocalIndexState::ReadyNoCalls,
        LocalIndexState::Ready,
    ] {
        assert!(!immediate_reindex_required(state));
    }
}

/// 扫描回看仍可是 30 日，派生保留下界必须跟用户天数，不能裁回 30 日。
#[test]
fn scan_policy_retention_follows_saved_days_not_the_thirty_day_ingest_window() {
    let observed_at_epoch_ms = 1_775_000_000_000;
    let device_tz = jiff::tz::TimeZone::UTC;
    let days = crate::RetentionDays::new(90).expect("90 days is approved");
    let policy = crate::local_index_scan_policy_with_retention(
        ScanStartOrigin::ExplicitUser,
        true,
        observed_at_epoch_ms,
        days,
        crate::TimeStandard::utc(),
        &device_tz,
    );
    let expected_retention = crate::retention_cutoff_epoch_ms(
        days,
        observed_at_epoch_ms,
        &crate::TimeStandard::utc(),
        &device_tz,
    )
    .expect("90-day cutoff exists");
    let thirty_same_standard = crate::retention_cutoff_epoch_ms(
        crate::RetentionDays::new(30).expect("30 days is approved"),
        observed_at_epoch_ms,
        &crate::TimeStandard::utc(),
        &device_tz,
    )
    .expect("30-day cutoff exists");
    let ingest_thirty = WindowBoundaries::for_local_today(observed_at_epoch_ms).thirty_days;
    assert_eq!(policy.window, LocalIndexScanWindow::ThirtyDays);
    assert_eq!(policy.scan_since_epoch_ms, ingest_thirty);
    assert_eq!(policy.retention_since_epoch_ms, expected_retention);
    assert!(
        policy.retention_since_epoch_ms < thirty_same_standard,
        "90-day retention must start earlier than a 30-day window under the same standard"
    );
}

/// 验证覆盖报告合并遵循失败 > 取消 > 部分 > 完整的优先级。
#[test]
fn merges_coverage_with_latest_scope_and_sums_metadata() {
    let discovery = CoverageReport {
        state: CoverageState::Complete,
        roots_scanned: 5,
        roots_discovered: 2,
        permission_denied_count: 1,
        skipped_count: 2,
        warning_count: 0,
    };
    let scan = CoverageReport {
        state: CoverageState::Partial,
        roots_scanned: 1,
        roots_discovered: 0,
        permission_denied_count: 2,
        skipped_count: 3,
        warning_count: 4,
    };
    let merged = merge_coverage_reports(discovery, &scan);

    assert_eq!(merged.state, CoverageState::Partial);
    assert_eq!(merged.roots_scanned, 5);
    assert_eq!(merged.roots_discovered, 2);
    assert_eq!(merged.permission_denied_count, 3);
    assert_eq!(merged.skipped_count, 5);
    assert_eq!(merged.warning_count, 4);
}

/// 验证稳定客户端前缀与别名规范化规则可复用并保持一致。
#[test]
fn validates_root_id_and_alias_rules() {
    assert!(validate_source_root_id(SourceClientKind::Codex, "root-0123456789abcdef").is_ok());
    assert!(
        validate_source_root_id(SourceClientKind::ClaudeCode, "claude-root-0123456789abcdef")
            .is_ok()
    );
    assert!(validate_source_root_id(SourceClientKind::Codex, "/Users/example/.codex").is_err());
    assert!(validate_source_root_id(SourceClientKind::Codex, "root-not-a-hash").is_err());
    assert!(
        validate_source_root_id(SourceClientKind::ClaudeCode, "root-0123456789abcdef").is_err()
    );
    assert_eq!(
        normalize_source_root_alias("  工作数据根  ").expect("alias should normalize"),
        "工作数据根".to_owned()
    );
    assert!(normalize_source_root_alias("/Users/example/.codex").is_err());
    assert!(normalize_source_root_alias(" ").is_err());
    assert!(normalize_source_root_alias(&"x".repeat(65)).is_err());
    assert_eq!(
        source_root_alias_from_path(std::path::Path::new("/"), SourceClientKind::Codex),
        SourceClientKind::Codex.default_source_root_alias()
    );
    assert_eq!(
        source_root_alias_from_path(
            std::path::Path::new("/tmp/cli-demo"),
            SourceClientKind::ClaudeCode
        ),
        "cli-demo".to_owned()
    );
}

#[derive(Debug)]
/// 表示数据根策略测试中的最小已发现来源。
struct TestSourceRoot {
    id: &'static str,
}

#[test]
/// 验证单个已核对来源可以进入登记决策。
fn validates_single_verified_source_root() {
    let roots = [TestSourceRoot {
        id: "root-0123456789abcdef",
    }];
    assert_eq!(
        ensure_single_verified_source_root(
            &roots,
            |root| root.id,
            &[] as &[String],
            &[] as &[String],
            Some("root-0123456789abcdef"),
        ),
        Ok(())
    );

    let duplicate = [
        TestSourceRoot {
            id: "root-0123456789abcdef",
        },
        TestSourceRoot {
            id: "root-0123456789abceef",
        },
    ];
    assert_eq!(
        ensure_single_verified_source_root(
            &duplicate,
            |root| root.id,
            &[] as &[String],
            &[] as &[String],
            None,
        ),
        Err(SourceRootDiscoveryVerificationError::NotSingleCandidate)
    );
}

#[test]
/// 验证未核对来源会被保守标记并阻止直接登记。
fn detects_unverified_source_root_discovery() {
    let roots = [TestSourceRoot {
        id: "root-0123456789abcdef",
    }];
    let invalid = ["bad-root".to_owned()];
    assert_eq!(
        ensure_single_verified_source_root(
            &roots,
            |root| root.id,
            &invalid,
            &[] as &[String],
            None,
        ),
        Err(SourceRootDiscoveryVerificationError::DiscoveryNotVerifiable)
    );
    assert_eq!(
        ensure_single_verified_source_root(
            &roots,
            |root| root.id,
            &[] as &[String],
            &invalid,
            None,
        ),
        Err(SourceRootDiscoveryVerificationError::DiscoveryNotVerifiable)
    );
}

#[test]
/// 验证接受来源前必须匹配用户请求的稳定数据根标识。
fn checks_requested_root_id_before_accepting_source_root() {
    let roots = [TestSourceRoot {
        id: "root-0123456789abcdef",
    }];
    assert_eq!(
        ensure_single_verified_source_root(
            &roots,
            |root| root.id,
            &[] as &[String],
            &[] as &[String],
            Some("root-0000000000000001"),
        ),
        Err(SourceRootDiscoveryVerificationError::RootIdMismatch)
    );
}

/// 验证筛选 ID 仅允许安全字符、长度与非空约束。
#[test]
fn validates_usage_filter_ids() {
    assert!(is_safe_usage_filter_id("model-gpt-4o"));
    assert_eq!(validate_usage_filter_id("thread_abc.123"), Ok(()));
    assert_eq!(
        validate_usage_filter_id("not valid"),
        Err(UsageFilterIdValidationError::Invalid)
    );
    assert_eq!(
        validate_usage_filter_id(&"x".repeat(161)),
        Err(UsageFilterIdValidationError::Invalid)
    );
}

/// 验证来源客户端的展示文本映射保持稳定。
#[test]
fn exposes_source_client_display_labels() {
    assert_eq!(
        SourceClientKind::Codex.default_source_root_alias(),
        "Codex 数据根"
    );
    assert_eq!(
        SourceClientKind::ClaudeCode.default_source_root_alias(),
        "Claude Code 数据根"
    );
    assert_eq!(
        SourceClientKind::Codex.source_environment_label(),
        "当前 CODEX_HOME"
    );
    assert_eq!(
        SourceClientKind::ClaudeCode.source_environment_label(),
        "当前 CLAUDE_CONFIG_DIR"
    );
    assert_eq!(
        SourceClientKind::GrokBuildCli.default_source_root_alias(),
        "Grok 数据根"
    );
    assert_eq!(
        SourceClientKind::GrokBuildCli.source_environment_label(),
        "当前 GROK_HOME"
    );
    assert_eq!(
        SourceClientKind::GrokBuildCli.root_id_namespace(),
        "grok-root"
    );
}

#[test]
/// 验证周期快速扫描只调度当前没有活动扫描的客户端。
fn allows_only_idle_clients_for_periodic_quick_scan() {
    assert_eq!(
        ensure_periodic_quick_scan_allowed(false, false),
        Err(PeriodicQuickScanError::InitializationRequired)
    );
    assert_eq!(
        ensure_periodic_quick_scan_allowed(true, true),
        Err(PeriodicQuickScanError::ScanAlreadyRunning)
    );
    assert_eq!(ensure_periodic_quick_scan_allowed(true, false), Ok(()));
}

/// 无已开放客户端时本拍必须跳过，不得虚构扫描对象。
#[test]
fn periodic_tick_skips_when_no_agents_are_enabled() {
    assert_eq!(
        decide_periodic_scan_tick(&[], |_| false, false),
        PeriodicScanTick::Skip
    );
}

/// 共享 writer 忙碌时本拍等待，不新增任务；空闲后同一拍覆盖全部已开放客户端。
#[test]
fn busy_writer_waits_and_idle_writer_scans_every_enabled_client() {
    let enabled = [
        SourceClientKind::Codex,
        SourceClientKind::ClaudeCode,
        SourceClientKind::GrokBuildCli,
    ];
    assert_eq!(
        decide_periodic_scan_tick(&enabled, |_| false, true),
        PeriodicScanTick::Wait
    );
    assert_eq!(
        decide_periodic_scan_tick(&enabled, |_| false, false),
        PeriodicScanTick::Start {
            clients: enabled.to_vec(),
        }
    );
}

/// 已在扫描的客户端从本拍剔除，其余空闲客户端仍按固定顺序全部执行。
#[test]
fn running_client_is_excluded_while_idle_agents_still_scan_this_tick() {
    let enabled = [
        SourceClientKind::Codex,
        SourceClientKind::ClaudeCode,
        SourceClientKind::GrokBuildCli,
    ];
    let running = |client| client == SourceClientKind::Codex;
    assert_eq!(
        decide_periodic_scan_tick(&enabled, running, false),
        PeriodicScanTick::Start {
            clients: vec![SourceClientKind::ClaudeCode, SourceClientKind::GrokBuildCli,],
        }
    );
    assert_eq!(
        decide_periodic_scan_tick(&enabled, |_| true, false),
        PeriodicScanTick::Wait
    );
}

#[test]
/// 验证来源新增与主目录变更返回一致的摘要状态。
fn summarizes_source_root_mutation_results_for_add_and_primary_path() {
    assert_eq!(
        source_root_add_outcome(true),
        SourceRootMutationOutcome {
            changed: true,
            kind: SourceRootMutationKind::SourceRootRegistered,
        }
    );
    assert_eq!(
        source_root_add_outcome(false),
        SourceRootMutationOutcome {
            changed: false,
            kind: SourceRootMutationKind::SourceRootAlreadyRegistered,
        }
    );
    assert_eq!(
        source_root_primary_outcome(true, true),
        SourceRootMutationOutcome {
            changed: true,
            kind: SourceRootMutationKind::PrimarySourceRootChanged,
        }
    );
    assert_eq!(
        source_root_primary_outcome(true, false),
        SourceRootMutationOutcome {
            changed: false,
            kind: SourceRootMutationKind::PrimarySourceRootAlreadySelected,
        }
    );
    assert_eq!(
        source_root_primary_outcome(false, false),
        SourceRootMutationOutcome {
            changed: false,
            kind: SourceRootMutationKind::PrimarySourceRootNotSet,
        }
    );
}

/// 验证展示标签净化规则可复用。
#[test]
fn validates_safe_display_labels() {
    assert_eq!(
        safe_technical_label("gpt-4o-mini"),
        Some("gpt-4o-mini".to_owned())
    );
    assert_eq!(safe_technical_label("/Users/root"), None);
    assert_eq!(
        safe_root_label(" 用户根目录 "),
        Some("用户根目录".to_owned())
    );
    assert_eq!(safe_root_label("/Users/root"), None);
    assert_eq!(safe_short_value("0123456789ABCDEF"), "89ABCDEF");
}

/// 验证主目录设置能力仅对 Codex 开放。
#[test]
fn supports_primary_source_root_only_for_codex() {
    assert_eq!(
        ensure_primary_source_root_supported(SourceClientKind::Codex),
        Ok(())
    );
    assert_eq!(
        ensure_primary_source_root_supported(SourceClientKind::ClaudeCode),
        Err(PrimarySourceRootSupportError::NotSupported)
    );
}
