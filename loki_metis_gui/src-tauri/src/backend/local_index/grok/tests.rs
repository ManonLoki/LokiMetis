//! 驱动真实 Grok 解析 → 本机索引 → 统计快照，总量必须等于夹具自身字段求和。

use std::fs;

use loki_metis_core::{
    CoverageReport, CoverageState, LocalIndexState, ProviderKind, SourceClientKind, TokenUsage,
    UsageDimension, build_usage_statistics,
};
use tempfile::tempdir;

use super::jsonl::{
    GROK_PARSER_VERSION, PRODUCTION_GROK_USAGE_MODEL_USAGE_JSONL, SYNTHETIC_GROK_UPDATES_JSONL,
    sum_completed_usage_from_fixture,
};
use super::{GrokDiscoveryInputs, discover_grok_quick, scan_grok_discovered_roots};
use crate::backend::local_index::{CancellationToken, LocalIndex, ScanConfig};

/// 返回 Grok 夹具的上游总量，缓存读取保持为输入分析子集。
fn expected_grok_tokens(usage: TokenUsage) -> TokenUsage {
    usage
}

/// writer 写入的 generation 必须等于 core 打开索引、概览和 Collect 使用的值。
#[test]
fn grok_writer_generation_matches_core_reader() {
    assert_eq!(
        GROK_PARSER_VERSION,
        SourceClientKind::GrokBuildCli.parser_version()
    );
}

/// 把合成 Grok home 走完发现、扫描和统计，进行中轮次不得进入总量。
#[tokio::test]
async fn grok_scan_statistics_match_fixture_completed_usage() {
    let home = tempdir().expect("isolated grok home");
    let session = home
        .path()
        .join("sessions")
        .join("%2Ftmp%2Fapp")
        .join("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    fs::create_dir_all(&session).expect("session directory");
    fs::write(session.join("updates.jsonl"), SYNTHETIC_GROK_UPDATES_JSONL)
        .expect("fixture written");
    let app_data = tempdir().expect("isolated app-data");
    let mut index = LocalIndex::open_in_app_data(app_data.path(), GROK_PARSER_VERSION)
        .await
        .expect("index opens");
    let discovered = discover_grok_quick(
        &GrokDiscoveryInputs {
            home_dir: None,
            grok_home: Some(home.path().to_path_buf()),
            registered_roots: Vec::new(),
        },
        &CancellationToken::new(),
    );
    assert_eq!(
        discovered.roots.len(),
        1,
        "synthetic Grok home must be accepted"
    );
    let mut scan_progress = Vec::new();
    let summary = scan_grok_discovered_roots(
        &mut index,
        &discovered.roots,
        ScanConfig::default(),
        &CancellationToken::new(),
        |progress| scan_progress.push(progress),
    )
    .await
    .expect("scan completes");
    drop(index);
    assert_eq!(summary.calls_added, 3);
    assert!(
        scan_progress
            .iter()
            .all(|progress| progress.current_root_id == discovered.roots[0].root_id)
    );
    assert!(!scan_progress.is_empty());

    let snapshot = LocalIndex::open_in_app_data(app_data.path(), GROK_PARSER_VERSION)
        .await
        .expect("reopen")
        .usage_snapshot()
        .await
        .expect("snapshot");
    let observed_at = 1_786_795_200_000_i64; // 2026-08-15T12:00:00Z
    let coverage = CoverageReport {
        state: CoverageState::Complete,
        roots_scanned: 1,
        roots_discovered: 1,
        permission_denied_count: 0,
        skipped_count: 0,
        warning_count: 0,
    };
    let aliases = snapshot
        .roots
        .into_iter()
        .map(|root| (root.root_id, root.alias))
        .collect();
    let statistics = build_usage_statistics(
        &snapshot.canonical,
        &aliases,
        snapshot.index_state,
        &coverage,
        loki_metis_core::LocalUsageWindow::Today,
        UsageDimension::Model,
        observed_at,
        ProviderKind::GrokSessionJsonl,
        Some(
            ProviderKind::GrokSessionJsonl
                .parser_source_label(SourceClientKind::GrokBuildCli.parser_version())
                .as_str(),
        ),
    )
    .expect("statistics build");
    let expected = expected_grok_tokens(sum_completed_usage_from_fixture(
        SYNTHETIC_GROK_UPDATES_JSONL,
    ));
    assert_eq!(statistics.fact.value.tokens, expected);
    assert_eq!(statistics.fact.value.call_count, 3);
    assert_ne!(statistics.fact.value.tokens, TokenUsage::zero());
}

/// 用 core 的 parser generation 打开索引、强制重建后，概览必须仍能读到完成轮次。
/// 禁止 writer 写更高一代、读取仍按旧代，把已入库的近 30 日记录显示成空扫描。
#[tokio::test]
async fn force_rebuild_keeps_completed_turns_visible_through_core_parser_version() {
    let home = tempdir().expect("isolated grok home");
    let session = home
        .path()
        .join("sessions")
        .join("%2Ftmp%2Fapp")
        .join("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    fs::create_dir_all(&session).expect("session directory");
    fs::write(
        session.join("updates.jsonl"),
        PRODUCTION_GROK_USAGE_MODEL_USAGE_JSONL,
    )
    .expect("fixture written");
    let app_data = tempdir().expect("isolated app-data");
    let parser = SourceClientKind::GrokBuildCli.parser_version();
    let mut index = LocalIndex::open_in_app_data(app_data.path(), parser)
        .await
        .expect("index opens");
    let discovered = discover_grok_quick(
        &GrokDiscoveryInputs {
            home_dir: None,
            grok_home: Some(home.path().to_path_buf()),
            registered_roots: Vec::new(),
        },
        &CancellationToken::new(),
    );
    assert_eq!(discovered.roots.len(), 1);
    scan_grok_discovered_roots(
        &mut index,
        &discovered.roots,
        ScanConfig::default(),
        &CancellationToken::new(),
        |_| {},
    )
    .await
    .expect("first scan completes");
    let rebuilt = scan_grok_discovered_roots(
        &mut index,
        &discovered.roots,
        ScanConfig {
            scan_since_epoch_ms: 1_786_891_792_000 - 30 * 86_400_000,
            force_rebuild: true,
            ..ScanConfig::default()
        },
        &CancellationToken::new(),
        |_| {},
    )
    .await
    .expect("force rebuild completes");
    drop(index);

    let snapshot = LocalIndex::open_in_app_data(app_data.path(), parser)
        .await
        .expect("reopen with core parser generation")
        .usage_snapshot()
        .await
        .expect("snapshot");
    assert_eq!(rebuilt.aggregate.call_count, 1);
    assert_eq!(snapshot.index_state, LocalIndexState::Ready);
    assert_eq!(snapshot.canonical.calls.len(), 1);
    assert_eq!(
        snapshot.canonical.calls[0].model.as_deref(),
        Some("grok-4.5-build")
    );
    assert_eq!(
        snapshot.canonical.calls[0].usage,
        sum_completed_usage_from_fixture(PRODUCTION_GROK_USAGE_MODEL_USAGE_JSONL)
    );
}

const APPENDED_GROK_TURN: &str = concat!(
    r#"{"sessionUpdate":"turn_completed","timestamp":"2026-08-15T12:00:00Z","model":"grok-4.5-build","usage":{"inputTokens":12,"outputTokens":8,"totalTokens":20}}"#,
    "\n",
);

/// 首扫后再追加一条无 turnId 的完成轮次，统计必须按该行自身字段增加，不能覆盖旧行。
#[tokio::test]
async fn grok_incremental_append_adds_new_turn_instead_of_overwriting() {
    let home = tempdir().expect("isolated grok home");
    let updates = home
        .path()
        .join("sessions")
        .join("%2Ftmp%2Fapp")
        .join("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
        .join("updates.jsonl");
    fs::create_dir_all(updates.parent().expect("session directory")).expect("session directory");
    fs::write(&updates, SYNTHETIC_GROK_UPDATES_JSONL).expect("fixture written");
    let app_data = tempdir().expect("isolated app-data");
    let discovered = discover_grok_quick(
        &GrokDiscoveryInputs {
            home_dir: None,
            grok_home: Some(home.path().to_path_buf()),
            registered_roots: Vec::new(),
        },
        &CancellationToken::new(),
    );
    let mut index = LocalIndex::open_in_app_data(app_data.path(), GROK_PARSER_VERSION)
        .await
        .expect("index opens");
    scan_grok_discovered_roots(
        &mut index,
        &discovered.roots,
        ScanConfig::default(),
        &CancellationToken::new(),
        |_| {},
    )
    .await
    .expect("first scan completes");
    drop(index);

    let before = load_today_statistics(app_data.path()).await;
    let baseline_raw = sum_completed_usage_from_fixture(SYNTHETIC_GROK_UPDATES_JSONL);
    let baseline = expected_grok_tokens(baseline_raw.clone());
    assert_eq!(before.tokens, baseline);
    assert_eq!(before.call_count, 3);

    let mut body = fs::read_to_string(&updates).expect("read fixture");
    body.push_str(APPENDED_GROK_TURN);
    fs::write(&updates, &body).expect("append completed turn");

    let mut index = LocalIndex::open_in_app_data(app_data.path(), GROK_PARSER_VERSION)
        .await
        .expect("index reopens");
    scan_grok_discovered_roots(
        &mut index,
        &discovered.roots,
        ScanConfig::default(),
        &CancellationToken::new(),
        |_| {},
    )
    .await
    .expect("append scan completes");
    drop(index);

    let after = load_today_statistics(app_data.path()).await;
    let appended = sum_completed_usage_from_fixture(APPENDED_GROK_TURN);
    let expected = expected_grok_tokens(
        baseline_raw
            .checked_add(&appended)
            .expect("combined sum fits"),
    );
    assert_eq!(after.tokens, expected);
    assert_eq!(after.call_count, before.call_count.saturating_add(1));
}

/// 已索引 `updates.jsonl` 在扫描窗口外被追加时必须重新解析，追加轮次不得因窗口丢失；
/// 追加内容入库后，同一窗口的下一次扫描不再打开该文件。
#[tokio::test]
async fn appended_grok_updates_outside_the_scan_window_are_still_indexed() {
    let home = tempdir().expect("isolated grok home");
    let updates = home
        .path()
        .join("sessions")
        .join("%2Ftmp%2Fapp")
        .join("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
        .join("updates.jsonl");
    fs::create_dir_all(updates.parent().expect("session directory")).expect("session directory");
    fs::write(&updates, SYNTHETIC_GROK_UPDATES_JSONL).expect("fixture written");
    let app_data = tempdir().expect("isolated app-data");
    let discovered = discover_grok_quick(
        &GrokDiscoveryInputs {
            home_dir: None,
            grok_home: Some(home.path().to_path_buf()),
            registered_roots: Vec::new(),
        },
        &CancellationToken::new(),
    );
    let mut index = LocalIndex::open_in_app_data(app_data.path(), GROK_PARSER_VERSION)
        .await
        .expect("index opens");
    let first = scan_grok_discovered_roots(
        &mut index,
        &discovered.roots,
        ScanConfig::default(),
        &CancellationToken::new(),
        |_| {},
    )
    .await
    .expect("first scan completes");
    assert_eq!(first.aggregate.call_count, 3);

    let mut body = fs::read_to_string(&updates).expect("read fixture");
    body.push_str(APPENDED_GROK_TURN);
    fs::write(&updates, &body).expect("append completed turn");
    let stale_mtime = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
    fs::File::options()
        .write(true)
        .open(&updates)
        .expect("updates reopens")
        .set_modified(stale_mtime)
        .expect("updates mtime is backdated");
    let windowed = ScanConfig {
        scan_since_epoch_ms: 3_000_000_000_000,
        ..ScanConfig::default()
    };

    let appended = scan_grok_discovered_roots(
        &mut index,
        &discovered.roots,
        windowed,
        &CancellationToken::new(),
        |_| {},
    )
    .await
    .expect("windowed scan completes");
    assert_eq!(appended.files_scanned, 1, "changed source must be re-read");
    assert_eq!(appended.calls_added, 1);
    assert_eq!(appended.aggregate.call_count, 4);

    let settled = scan_grok_discovered_roots(
        &mut index,
        &discovered.roots,
        windowed,
        &CancellationToken::new(),
        |_| {},
    )
    .await
    .expect("settled scan completes");
    assert_eq!(
        settled.files_scanned, 0,
        "unchanged source outside the window stays closed"
    );
    assert_eq!(
        settled.aggregate.call_count, 4,
        "retained source keeps its calls"
    );
}

/// 从当前 Grok 索引读取当天窗口的调用数与 Token 合计。
async fn load_today_statistics(app_data: &std::path::Path) -> TokenAndCount {
    let snapshot = LocalIndex::open_in_app_data(app_data, GROK_PARSER_VERSION)
        .await
        .expect("reopen")
        .usage_snapshot()
        .await
        .expect("snapshot");
    let observed_at = 1_786_795_200_000_i64;
    let coverage = CoverageReport {
        state: CoverageState::Complete,
        roots_scanned: 1,
        roots_discovered: 1,
        permission_denied_count: 0,
        skipped_count: 0,
        warning_count: 0,
    };
    let aliases = snapshot
        .roots
        .into_iter()
        .map(|root| (root.root_id, root.alias))
        .collect();
    let statistics = build_usage_statistics(
        &snapshot.canonical,
        &aliases,
        snapshot.index_state,
        &coverage,
        loki_metis_core::LocalUsageWindow::Today,
        UsageDimension::Model,
        observed_at,
        ProviderKind::GrokSessionJsonl,
        Some(
            ProviderKind::GrokSessionJsonl
                .parser_source_label(SourceClientKind::GrokBuildCli.parser_version())
                .as_str(),
        ),
    )
    .expect("statistics build");
    TokenAndCount {
        tokens: statistics.fact.value.tokens,
        call_count: statistics.fact.value.call_count,
    }
}

/// 测试对账用的合计与调用数。
struct TokenAndCount {
    tokens: TokenUsage,
    call_count: u64,
}
