//! 审计 clear_local_index 运行时收尾：coverage / last_cleared 与不得清的跨客户端状态。

use super::AppRuntimeState;
use crate::backend::local_index::{LocalIndex, PARSER_VERSION};
use crate::dto::{AgentClientKindDto, ScanKindDto, ScanScopeCodeDto, UsageClientKindDto};
use crate::privacy_store::load_settings;
use loki_metis_core::{
    Completeness, Confidence, CoverageReport, CoverageState, Freshness, MetricFact, MetricScope,
    ProviderKind, SourceClientKind, empty_coverage, source_client_app_data_dir,
};
use tempfile::tempdir;

/// 写入一条只含规范化本机字段的合成快照，证明另一客户端索引不被波及。
async fn seed_provider_snapshot(index: &mut LocalIndex) {
    let fact = MetricFact::new(
        0_u64,
        ProviderKind::ClaudeTranscriptJsonl,
        MetricScope::DeviceObserved,
        1,
        Freshness::Fresh,
        Completeness::Complete,
        Confidence::Exact,
        None,
    );
    let normalized_json = serde_json::to_string(&fact).expect("local fact serializes");
    index
        .save_normalized_snapshot(1, 2, &normalized_json)
        .await
        .expect("provider snapshot is seeded");
}

/// 构造一条可与 empty_coverage 区分的非空覆盖报告。
fn seeded_coverage(roots_scanned: u64) -> CoverageReport {
    CoverageReport {
        state: CoverageState::Complete,
        roots_scanned,
        roots_discovered: roots_scanned,
        permission_denied_count: 0,
        skipped_count: 0,
        warning_count: 0,
    }
}

/// 走与 clear_local_index 相同的收尾：clear_index + mark_index_cleared + empty_coverage。
#[tokio::test]
async fn clear_local_index_runtime_resets_only_target_client_coverage() {
    let temp = tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new_with_username_candidate(temp.path().to_path_buf(), || {
        Some("审计会话用户".to_owned())
    });
    state
        .set_initialization_completed(true)
        .await
        .expect("initialization completes");
    state
        .set_device_username("审计设备标签".to_owned())
        .await
        .expect("device username is saved");
    let collect = state
        .add_collect_provider("https://example.com/".to_owned(), 1, None)
        .await
        .expect("collect provider is saved");

    let claude_dir = source_client_app_data_dir(temp.path(), SourceClientKind::ClaudeCode);
    let mut claude_index = LocalIndex::open_in_app_data(&claude_dir, PARSER_VERSION)
        .await
        .expect("claude index opens");
    seed_provider_snapshot(&mut claude_index).await;
    assert_eq!(
        claude_index
            .provider_snapshot_count()
            .await
            .expect("claude snapshot count loads"),
        1
    );
    drop(claude_index);

    *state.coverages.get(SourceClientKind::Codex).write().await = seeded_coverage(3);
    *state
        .coverages
        .get(SourceClientKind::ClaudeCode)
        .write()
        .await = seeded_coverage(7);
    state
        .mark_index_cleared(AgentClientKindDto::ClaudeCode)
        .await;
    let claude_cleared_before = state
        .privacy_settings(UsageClientKindDto::ClaudeCode)
        .await
        .last_cleared_at_epoch_ms;
    assert!(claude_cleared_before.is_some());

    let scan = state.scans.get(SourceClientKind::Codex);
    scan.start(ScanKindDto::Quick, 1_000)
        .await
        .expect("scan snapshot can start");
    scan.update_progress(
        3,
        12,
        10_000,
        ScanScopeCodeDto::RegisteredRoots,
        None,
        "已登记数据目录".to_owned(),
    )
    .await;
    scan.finish_completed(2_000).await;
    assert_eq!(scan.snapshot().await.calls_indexed, 12);

    state
        .agent_clients
        .get(SourceClientKind::Codex)
        .local_analysis
        .clear_index()
        .await
        .expect("codex derived index is cleared");
    state.mark_index_cleared(AgentClientKindDto::Codex).await;
    *state.coverages.get(SourceClientKind::Codex).write().await = empty_coverage();

    assert_eq!(
        *state.coverages.get(SourceClientKind::Codex).read().await,
        empty_coverage()
    );
    let codex_settings = state.privacy_settings(UsageClientKindDto::Codex).await;
    assert!(codex_settings.last_cleared_at_epoch_ms.is_some());
    assert_eq!(
        codex_settings.device_username.as_deref(),
        Some("审计设备标签")
    );

    assert_eq!(
        *state
            .coverages
            .get(SourceClientKind::ClaudeCode)
            .read()
            .await,
        seeded_coverage(7)
    );
    assert_eq!(
        state
            .privacy_settings(UsageClientKindDto::ClaudeCode)
            .await
            .last_cleared_at_epoch_ms,
        claude_cleared_before
    );

    let persisted = load_settings(temp.path()).expect("settings remain");
    assert!(persisted.initialization_completed);
    assert_eq!(
        persisted
            .device_username
            .as_ref()
            .map(|value| value.as_str()),
        Some("审计设备标签")
    );
    assert_eq!(persisted.collect_providers.as_slice().len(), 1);
    assert_eq!(persisted.collect_providers.as_slice()[0].id, collect.id);

    let claude_index = LocalIndex::open_in_app_data(&claude_dir, PARSER_VERSION)
        .await
        .expect("claude index reopens");
    assert_eq!(
        claude_index
            .provider_snapshot_count()
            .await
            .expect("claude snapshot remains"),
        1
    );

    // 当前清空不重置 ScanCoordinator；数据源页可能仍显示上次 callsIndexed。
    assert_eq!(
        state
            .scans
            .get(SourceClientKind::Codex)
            .snapshot()
            .await
            .calls_indexed,
        12
    );
}
