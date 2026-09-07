//! 扫描保留下界必须读产品根上的已保存天数，不能读客户端子目录后回退成 90。

use loki_metis_core::{
    LocalIndex, ProviderKind, RetentionDays, ScanCancellation, ScanKind, ScanStartOrigin,
    SourceClientKind, TimeStandard, empty_coverage, retention_cutoff_epoch_ms,
    source_client_app_data_dir,
};
use sea_orm::{ConnectOptions, ConnectionTrait, Database};
use tempfile::tempdir;

use super::client::ScanClient;
use super::run::{run_scan, scan_retention_policy};
use crate::backend::local_index::{
    CancellationToken, ClaudeDiscoveredRoot, ClaudeDiscoveryResult, DiscoveryProgress,
    GrokDiscoveredRoot, GrokDiscoveryResult, LocalError, RegisteredRoot, ScanConfig, ScanProgress,
    ScanSummary,
};
use crate::privacy_store::save_settings;
use crate::runtime::now_epoch_ms;

/// 产品根保存 180 天时，Claude/Grok 子目录里第 91–179 日的派生调用扫描后必须仍在。
#[tokio::test]
async fn run_scan_keeps_in_window_calls_when_settings_live_at_product_root() {
    let product = tempdir().expect("product app-data exists");
    let settings = crate::privacy_store::LocalPrivacySettings {
        retention_days: RetentionDays::new(180).expect("180 days is approved"),
        ..Default::default()
    };
    save_settings(product.path(), &settings).expect("retention days persist at product root");

    let observed = now_epoch_ms();
    let kept_cutoff = retention_cutoff_epoch_ms(
        settings.retention_days,
        observed,
        &TimeStandard::Local,
        &jiff::tz::TimeZone::system(),
    )
    .expect("180-day cutoff exists");
    let default_cutoff = retention_cutoff_epoch_ms(
        RetentionDays::default(),
        observed,
        &TimeStandard::Local,
        &jiff::tz::TimeZone::system(),
    )
    .expect("90-day cutoff exists");
    let inside_user_window = default_cutoff.saturating_sub(1);
    let outside_user_window = kept_cutoff.saturating_sub(1);
    assert!(inside_user_window < default_cutoff);
    assert!(inside_user_window >= kept_cutoff);

    let claude_dir = source_client_app_data_dir(product.path(), SourceClientKind::ClaudeCode);
    assert_eq!(
        scan_retention_policy(
            product.path(),
            ScanStartOrigin::ExplicitUser,
            true,
            observed
        )
        .retention_since_epoch_ms,
        kept_cutoff
    );
    assert_eq!(
        scan_retention_policy(&claude_dir, ScanStartOrigin::ExplicitUser, true, observed)
            .retention_since_epoch_ms,
        default_cutoff,
        "client subdirectory must not be treated as the settings root"
    );

    prepare_client_index(
        product.path(),
        SourceClientKind::ClaudeCode,
        outside_user_window,
        inside_user_window,
    )
    .await;
    run_scan::<PruneOnlyClaudeClient>(
        claude_dir,
        product.path().to_path_buf(),
        None,
        ScanKind::Quick,
        ScanStartOrigin::ExplicitUser,
        ScanCancellation::new(),
        Box::new(|_| {}),
    )
    .await
    .expect("claude scan prune runs");
    assert_client_keeps_user_window(product.path(), SourceClientKind::ClaudeCode).await;

    prepare_client_index(
        product.path(),
        SourceClientKind::GrokBuildCli,
        outside_user_window,
        inside_user_window,
    )
    .await;
    run_scan::<PruneOnlyGrokClient>(
        source_client_app_data_dir(product.path(), SourceClientKind::GrokBuildCli),
        product.path().to_path_buf(),
        None,
        ScanKind::Quick,
        ScanStartOrigin::ExplicitUser,
        ScanCancellation::new(),
        Box::new(|_| {}),
    )
    .await
    .expect("grok scan prune runs");
    assert_client_keeps_user_window(product.path(), SourceClientKind::GrokBuildCli).await;
}

/// 发现与增量扫描都为空，只让 `run_scan` 走设置读取和派生 prune。
struct PruneOnlyClaudeClient;

impl ScanClient for PruneOnlyClaudeClient {
    /// 使用 Claude 已发现根作为空扫描客户端的来源类型。
    type DiscoveredRoot = ClaudeDiscoveredRoot;
    /// 使用 Claude 发现结果承载空候选集合。
    type Discovery = ClaudeDiscoveryResult;

    /// 返回当前 Claude parser generation，确保清理打开正确物理库。
    fn parser_version() -> u32 {
        SourceClientKind::ClaudeCode.parser_version()
    }

    /// 返回 Claude transcript provider，供空扫描摘要保持客户端身份。
    fn provider_kind() -> ProviderKind {
        ProviderKind::ClaudeTranscriptJsonl
    }

    /// 将完全扫描目录预算压到最小，测试不会实际遍历目录。
    fn full_device_max_directories() -> u64 {
        1
    }

    /// 把本机错误映射为测试编排使用的安全字符串。
    fn map_local_error(error: LocalError) -> String {
        error.to_string()
    }

    /// 返回空的 Claude 快速发现结果，隔离真实 transcript。
    fn discover_quick(_: Vec<RegisteredRoot>, _: &CancellationToken) -> Self::Discovery {
        empty_claude_discovery()
    }

    /// 单根重验证在本测试客户端中同样保持空发现结果。
    fn discover_registered(_: Vec<RegisteredRoot>, _: &CancellationToken) -> Self::Discovery {
        empty_claude_discovery()
    }

    /// 返回空的 Claude 完全发现结果且不报告虚假进度。
    fn discover_full_device_with_progress<F>(_: &CancellationToken, _: F) -> Self::Discovery
    where
        F: FnMut(DiscoveryProgress),
    {
        empty_claude_discovery()
    }

    /// 接受空测试根登记调用而不写入真实来源。
    async fn register_root(
        _: &mut crate::backend::local_index::LocalIndex,
        _: &Self::DiscoveredRoot,
    ) -> Result<(), LocalError> {
        Ok(())
    }

    /// 返回空 Claude 扫描摘要，只让测试观察清理窗口。
    async fn scan_discovered_roots<F>(
        _: &mut crate::backend::local_index::LocalIndex,
        _: &[Self::DiscoveredRoot],
        _: ScanConfig,
        _: &CancellationToken,
        _: F,
    ) -> Result<ScanSummary, LocalError>
    where
        F: FnMut(ScanProgress) + Send,
    {
        Ok(empty_scan_summary())
    }
}

/// 发现与增量扫描都为空，只让 `run_scan` 走设置读取和派生 prune。
struct PruneOnlyGrokClient;

impl ScanClient for PruneOnlyGrokClient {
    /// 使用 Grok 已发现根作为空扫描客户端的来源类型。
    type DiscoveredRoot = GrokDiscoveredRoot;
    /// 使用 Grok 发现结果承载空候选集合。
    type Discovery = GrokDiscoveryResult;

    /// 返回当前 Grok parser generation，确保清理打开正确物理库。
    fn parser_version() -> u32 {
        SourceClientKind::GrokBuildCli.parser_version()
    }

    /// 返回 Grok session provider，供空扫描摘要保持客户端身份。
    fn provider_kind() -> ProviderKind {
        ProviderKind::GrokSessionJsonl
    }

    /// 将完全扫描目录预算压到最小，测试不会实际遍历目录。
    fn full_device_max_directories() -> u64 {
        1
    }

    /// 把本机错误映射为测试编排使用的安全字符串。
    fn map_local_error(error: LocalError) -> String {
        error.to_string()
    }

    /// 返回空的 Grok 快速发现结果，隔离真实 updates 文件。
    fn discover_quick(_: Vec<RegisteredRoot>, _: &CancellationToken) -> Self::Discovery {
        empty_grok_discovery()
    }

    /// 单根重验证在本测试客户端中同样保持空发现结果。
    fn discover_registered(_: Vec<RegisteredRoot>, _: &CancellationToken) -> Self::Discovery {
        empty_grok_discovery()
    }

    /// 返回空的 Grok 完全发现结果且不报告虚假进度。
    fn discover_full_device_with_progress<F>(_: &CancellationToken, _: F) -> Self::Discovery
    where
        F: FnMut(DiscoveryProgress),
    {
        empty_grok_discovery()
    }

    /// 接受空测试根登记调用而不写入真实来源。
    async fn register_root(
        _: &mut crate::backend::local_index::LocalIndex,
        _: &Self::DiscoveredRoot,
    ) -> Result<(), LocalError> {
        Ok(())
    }

    /// 返回空 Grok 扫描摘要，只让测试观察清理窗口。
    async fn scan_discovered_roots<F>(
        _: &mut crate::backend::local_index::LocalIndex,
        _: &[Self::DiscoveredRoot],
        _: ScanConfig,
        _: &CancellationToken,
        _: F,
    ) -> Result<ScanSummary, LocalError>
    where
        F: FnMut(ScanProgress) + Send,
    {
        Ok(empty_scan_summary())
    }
}

/// 空 Claude 发现结果，避免测试触碰本机真实 transcript。
fn empty_claude_discovery() -> ClaudeDiscoveryResult {
    ClaudeDiscoveryResult {
        roots: Vec::new(),
        confirmed_invalid_root_ids: Vec::new(),
        unconfirmed_root_ids: Vec::new(),
        coverage: empty_coverage(),
        directories_scanned: 0,
        symlink_skipped_count: 0,
        network_skipped_count: 0,
    }
}

/// 空 Grok 发现结果，避免测试触碰本机真实 sessions。
fn empty_grok_discovery() -> GrokDiscoveryResult {
    GrokDiscoveryResult {
        roots: Vec::new(),
        confirmed_invalid_root_ids: Vec::new(),
        unconfirmed_root_ids: Vec::new(),
        coverage: empty_coverage(),
        directories_scanned: 0,
        symlink_skipped_count: 0,
        network_skipped_count: 0,
    }
}

/// 空增量扫描摘要。
fn empty_scan_summary() -> ScanSummary {
    ScanSummary {
        scan_id: "prune-only".to_owned(),
        coverage: empty_coverage(),
        files_scanned: 0,
        unchanged_files: 0,
        rebuilt_files: 0,
        calls_added: 0,
        call_count: 0,
    }
}

/// 在客户端子目录建库并写入窗口内外各一条派生调用。
async fn prepare_client_index(
    product_dir: &std::path::Path,
    client: SourceClientKind,
    outside_epoch_ms: i64,
    inside_epoch_ms: i64,
) {
    let client_dir = source_client_app_data_dir(product_dir, client);
    let index = LocalIndex::open_in_app_data(&client_dir, client.parser_version())
        .await
        .expect("client index opens");
    let database_path = index.database_path().to_path_buf();
    drop(index);
    insert_window_fixture(
        &database_path,
        client.parser_version(),
        outside_epoch_ms,
        inside_epoch_ms,
    )
    .await;
}

/// 断言用户 180 天窗口内的调用仍在，更早的已被裁掉，根与 checkpoint 仍在。
async fn assert_client_keeps_user_window(product_dir: &std::path::Path, client: SourceClientKind) {
    let index = LocalIndex::open_in_app_data(
        &source_client_app_data_dir(product_dir, client),
        client.parser_version(),
    )
    .await
    .expect("client index reopens");
    let remaining = index.canonical_calls().await.expect("calls remain");
    let ids: Vec<_> = remaining
        .calls
        .iter()
        .map(|call| call.logical_call_id.as_str())
        .collect();
    assert!(
        ids.contains(&"inside-user-window"),
        "{client:?} must keep day-91-to-179 calls when product root saved 180 days: {ids:?}"
    );
    assert!(
        !ids.contains(&"outside-user-window"),
        "{client:?} must still drop calls older than 180 days: {ids:?}"
    );
    assert!(
        index
            .has_current_parser_usage()
            .await
            .expect("checkpoint remains")
    );
    assert_eq!(index.all_roots().await.expect("roots remain").len(), 1);
}

/// 写入一根、一条用户窗口内调用和一条窗口外调用。
async fn insert_window_fixture(
    database_path: &std::path::Path,
    parser_version: u32,
    outside_epoch_ms: i64,
    inside_epoch_ms: i64,
) {
    let app_data_dir = database_path
        .parent()
        .expect("database has an app-data parent");
    let mut index = LocalIndex::open_in_app_data(app_data_dir, parser_version)
        .await
        .expect("fixture index opens");
    index
        .register_confirmed_root_fields(
            "root-keep",
            &app_data_dir.join("retention-root"),
            "Keep",
            loki_metis_core::DiscoveryMethod::Registered,
        )
        .await
        .expect("fixture root registers");
    drop(index);
    let mut options = ConnectOptions::new("sqlite://placeholder.sqlite3");
    let database_path = database_path.to_path_buf();
    options.map_sqlx_sqlite_opts(move |sqlite_options| {
        sqlite_options
            .filename(&database_path)
            .create_if_missing(true)
    });
    let connection = Database::connect(options)
        .await
        .expect("migrated database reopens");
    connection
        .execute_unprepared(&format!(
            "UPDATE source_roots
                SET last_coverage_state = 'complete', activation_state = 'ready'
              WHERE root_id = 'root-keep';
             INSERT INTO source_files
               (source_id, root_id, relative_label, file_identity, archived,
                observed_size, modified_at_epoch_ms, parsed_offset, trailing_bytes,
                oversized_tail, parser_version, generation, ready, thread_key,
                project_key, model, reasoning_effort, call_sequence, token_snapshots_ready)
             VALUES
               ('source-keep', 'root-keep', 'sessions/test.jsonl', 'file', 0,
                10, 200, 10, 0, 0, {parser_version}, 1, 1, 'thread', NULL, NULL, NULL, 2, 1);
             INSERT INTO usage_calls
               (source_id, generation, logical_call_id, occurred_at_epoch_ms,
                model, reasoning_effort, project_key, thread_key, input_tokens,
                cached_input_tokens, cache_write_input_tokens, output_tokens,
                reasoning_output_tokens, total_tokens, total_is_derived, confidence)
             VALUES
               ('source-keep', 1, 'outside-user-window', {outside_epoch_ms}, NULL, NULL, NULL, 'thread', 1, 0, NULL, 0, 0, 1, 0, 'exact'),
               ('source-keep', 1, 'inside-user-window', {inside_epoch_ms}, NULL, NULL, NULL, 'thread', 2, 0, NULL, 0, 0, 2, 0, 'exact');
             INSERT INTO usage_token_snapshots
               (source_id, generation, thread_key, occurred_at_epoch_ms,
                logical_call_id, total_tokens)
             VALUES
               ('source-keep', 1, 'thread', {outside_epoch_ms}, 'outside-user-window', 1),
               ('source-keep', 1, 'thread', {inside_epoch_ms}, 'inside-user-window', 2);"
        ))
        .await
        .expect("fixture is inserted");
}
