//! 针对统一 `run_scan` 骨架的跨层回归：未登记 Quick 根必须在发现当轮完成
//! 登记、认领、索引和激活，同时不能因后续自动重发现破坏用户维护状态。

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use loki_metis_core::{
    CoverageReport, CoverageState, DiscoveryMethod, ProviderKind, RootActivationState,
    ScanCancellation, ScanKind, SourceClientKind, SourceParseCheckpoint, empty_coverage,
};

use super::client::ScanClient;
use super::run::run_scan;
use crate::backend::local_index::{
    CancellationToken, ClaudeDiscoveredRoot, ClaudeDiscoveryResult, DiscoveredRoot,
    DiscoveryInputs, DiscoveryProgress, DiscoveryResult, LocalError, LocalIndex,
    RegisterDiscoveredRoot, RegisteredRoot, ScanConfig, ScanProgress, ScanSummary, discover_quick,
    scan_discovered_roots,
};

/// 测试用固定合成根路径，供 `CurrentScanTestClient::discover_quick` 使用。
static SYNTHETIC_DISCOVERY_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// 固定返回测试准备的未登记 Codex 根，其余登记与扫描均复用真实实现。
struct CurrentScanTestClient;

impl ScanClient for CurrentScanTestClient {
    /// 复用真实 Codex 发现根类型。
    type DiscoveredRoot = DiscoveredRoot;
    /// 复用真实 Codex 发现结果类型。
    type Discovery = DiscoveryResult;

    /// 返回真实 Codex parser generation。
    fn parser_version() -> u32 {
        SourceClientKind::Codex.parser_version()
    }

    /// 返回真实 Codex provider 类型。
    fn provider_kind() -> ProviderKind {
        ProviderKind::RolloutJsonl
    }

    /// 测试固定预算，不需要真实的全设备进度换算。
    fn full_device_max_directories() -> u64 {
        1
    }

    /// 直接透传错误的 `Display` 文本，不做额外折叠。
    fn map_local_error(error: LocalError) -> String {
        error.to_string()
    }

    /// 忽略传入的已登记根，固定发现测试准备好的合成未登记根。
    fn discover_quick(
        _registered_roots: Vec<RegisteredRoot>,
        cancellation: &CancellationToken,
    ) -> Self::Discovery {
        discover_quick(
            &DiscoveryInputs {
                codex_home: Some(
                    SYNTHETIC_DISCOVERY_ROOT
                        .get()
                        .expect("test root is initialized")
                        .clone(),
                ),
                ..DiscoveryInputs::default()
            },
            cancellation,
        )
    }

    /// 测试单根重验证只消费显式登记输入，不读取进程环境。
    fn discover_registered(
        registered_roots: Vec<RegisteredRoot>,
        cancellation: &CancellationToken,
    ) -> Self::Discovery {
        discover_quick(
            &DiscoveryInputs {
                registered_roots,
                ..DiscoveryInputs::default()
            },
            cancellation,
        )
    }

    /// 测试不需要全设备发现，固定返回空结果。
    fn discover_full_device_with_progress<F>(
        _cancellation: &CancellationToken,
        _on_progress: F,
    ) -> Self::Discovery
    where
        F: FnMut(DiscoveryProgress),
    {
        DiscoveryResult {
            roots: Vec::new(),
            confirmed_invalid_root_ids: Vec::new(),
            unconfirmed_root_ids: Vec::new(),
            coverage: CoverageReport {
                state: CoverageState::Complete,
                roots_scanned: 0,
                roots_discovered: 0,
                permission_denied_count: 0,
                skipped_count: 0,
                warning_count: 0,
            },
            directories_scanned: 0,
            symlink_skipped_count: 0,
            network_skipped_count: 0,
        }
    }

    /// 复用真实的登记实现。
    async fn register_root(
        index: &mut LocalIndex,
        root: &Self::DiscoveredRoot,
    ) -> Result<(), LocalError> {
        index.register_root(root).await
    }

    /// 复用真实的增量扫描实现。
    async fn scan_discovered_roots<F>(
        index: &mut LocalIndex,
        roots: &[Self::DiscoveredRoot],
        config: ScanConfig,
        cancellation: &CancellationToken,
        on_progress: F,
    ) -> Result<ScanSummary, LocalError>
    where
        F: FnMut(ScanProgress) + Send,
    {
        scan_discovered_roots(index, roots, config, cancellation, on_progress).await
    }
}

/// 固定把全部已登记 Claude 根判为失效，用于复现首次候选被错误删除的生命周期。
struct InvalidClaudeCandidateTestClient;

impl ScanClient for InvalidClaudeCandidateTestClient {
    /// 复用 Claude 严格发现根类型。
    type DiscoveredRoot = ClaudeDiscoveredRoot;
    /// 复用 Claude 发现结果载荷。
    type Discovery = ClaudeDiscoveryResult;

    /// 返回当前 Claude parser generation。
    fn parser_version() -> u32 {
        SourceClientKind::ClaudeCode.parser_version()
    }

    /// 返回 Claude transcript 来源类型。
    fn provider_kind() -> ProviderKind {
        ProviderKind::ClaudeTranscriptJsonl
    }

    /// 本测试不执行全盘发现，进度上限保持最小非零值。
    fn full_device_max_directories() -> u64 {
        1
    }

    /// 测试直接保留底层错误文本。
    fn map_local_error(error: LocalError) -> String {
        error.to_string()
    }

    /// 把传入的已登记根全部模拟为严格签名失效。
    fn discover_quick(
        registered_roots: Vec<RegisteredRoot>,
        _cancellation: &CancellationToken,
    ) -> Self::Discovery {
        let confirmed_invalid_root_ids = registered_roots
            .into_iter()
            .filter_map(|root| root.root_id)
            .collect::<Vec<_>>();
        let root_count = u64::try_from(confirmed_invalid_root_ids.len()).unwrap_or(u64::MAX);
        ClaudeDiscoveryResult {
            roots: Vec::new(),
            confirmed_invalid_root_ids,
            unconfirmed_root_ids: Vec::new(),
            coverage: CoverageReport {
                state: CoverageState::Partial,
                roots_scanned: root_count,
                roots_discovered: 0,
                permission_denied_count: 0,
                skipped_count: root_count,
                warning_count: 0,
            },
            directories_scanned: 0,
            symlink_skipped_count: 0,
            network_skipped_count: 0,
        }
    }

    /// 单根重验证沿用相同的严格失效模拟。
    fn discover_registered(
        registered_roots: Vec<RegisteredRoot>,
        cancellation: &CancellationToken,
    ) -> Self::Discovery {
        Self::discover_quick(registered_roots, cancellation)
    }

    /// 本测试不执行全盘发现，固定返回空的完整覆盖。
    fn discover_full_device_with_progress<F>(
        _cancellation: &CancellationToken,
        _on_progress: F,
    ) -> Self::Discovery
    where
        F: FnMut(DiscoveryProgress),
    {
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

    /// 失效发现没有可登记根；若意外调用仍保持无副作用。
    async fn register_root(
        _index: &mut LocalIndex,
        _root: &Self::DiscoveredRoot,
    ) -> Result<(), LocalError> {
        Ok(())
    }

    /// 失效发现没有可扫描根，返回空 Claude 汇总。
    async fn scan_discovered_roots<F>(
        _index: &mut LocalIndex,
        _roots: &[Self::DiscoveredRoot],
        _config: ScanConfig,
        _cancellation: &CancellationToken,
        _on_progress: F,
    ) -> Result<ScanSummary, LocalError>
    where
        F: FnMut(ScanProgress) + Send,
    {
        Ok(ScanSummary {
            scan_id: "invalid-claude-candidate".to_owned(),
            coverage: empty_coverage(),
            files_scanned: 0,
            unchanged_files: 0,
            rebuilt_files: 0,
            calls_added: 0,
            call_count: 0,
        })
    }
}

/// 验证本轮新发现的未登记根会在同一次扫描内完成登记、认领、索引与激活，
/// 且不会因后续自动重发现破坏用户已维护的状态。
#[tokio::test]
async fn newly_discovered_root_indexes_in_same_scan_and_preserves_user_state() {
    let source_temp = tempfile::tempdir().expect("isolated source root is available");
    let app_temp = tempfile::tempdir().expect("isolated app data is available");
    let root = source_temp.path().join("environment-codex");
    let rollout = root.join("sessions/rollout-current-scan.jsonl");
    fs::create_dir_all(rollout.parent().expect("rollout has a parent"))
        .expect("session directory is created");
    fs::write(
        rollout,
        concat!(
            "{\"timestamp\":\"2026-08-08T00:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"session-current-scan\",\"cwd\":\"/synthetic/project\"}}\n",
            "{\"timestamp\":\"2026-08-08T00:00:01Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"call_id\":\"call-current-scan\",\"model\":\"gpt-5\",\"reasoning_effort\":\"high\",\"info\":{\"last_token_usage\":{\"input_tokens\":10,\"cached_input_tokens\":2,\"cache_write_input_tokens\":1,\"output_tokens\":5,\"reasoning_output_tokens\":1,\"total_tokens\":15}}}}\n"
        ),
    )
    .expect("synthetic rollout is written");
    SYNTHETIC_DISCOVERY_ROOT
        .set(root)
        .expect("the current-scan fixture is initialized once");

    let first = run_scan::<CurrentScanTestClient>(
        app_temp.path().to_path_buf(),
        app_temp.path().to_path_buf(),
        None,
        ScanKind::Quick,
        loki_metis_core::ScanStartOrigin::ExplicitUser,
        ScanCancellation::new(),
        Box::new(|_| {}),
    )
    .await
    .expect("first quick scan succeeds");

    assert_eq!(first.call_count, 1);
    assert_eq!(first.coverage.state, CoverageState::Complete);
    assert_eq!(first.roots.len(), 1);
    assert_eq!(first.roots[0].activation_state, RootActivationState::Ready);
    let root_id = first.roots[0].id.clone();

    let mut index =
        LocalIndex::open_in_app_data(app_temp.path(), SourceClientKind::Codex.parser_version())
            .await
            .expect("isolated index reopens");
    assert!(
        index
            .set_root_alias(&root_id, "用户保留别名")
            .await
            .expect("alias changes")
    );
    assert!(
        index
            .set_primary_root(Some(&root_id))
            .await
            .expect("primary root changes")
    );
    drop(index);

    let second = run_scan::<CurrentScanTestClient>(
        app_temp.path().to_path_buf(),
        app_temp.path().to_path_buf(),
        None,
        ScanKind::Quick,
        loki_metis_core::ScanStartOrigin::ExplicitUser,
        ScanCancellation::new(),
        Box::new(|_| {}),
    )
    .await
    .expect("second quick scan succeeds");
    assert_eq!(second.roots[0].alias, "用户保留别名");
    assert!(second.roots[0].is_primary);
    assert_eq!(second.roots[0].activation_state, RootActivationState::Ready);

    let mut index =
        LocalIndex::open_in_app_data(app_temp.path(), SourceClientKind::Codex.parser_version())
            .await
            .expect("isolated index reopens for disable");
    assert!(
        index
            .set_root_enabled(&root_id, false)
            .await
            .expect("root disables")
    );
    drop(index);

    let third = run_scan::<CurrentScanTestClient>(
        app_temp.path().to_path_buf(),
        app_temp.path().to_path_buf(),
        None,
        ScanKind::Quick,
        loki_metis_core::ScanStartOrigin::ExplicitUser,
        ScanCancellation::new(),
        Box::new(|_| {}),
    )
    .await
    .expect("disabled-root quick scan succeeds");
    assert_eq!(third.roots.len(), 1);
    assert!(!third.roots[0].enabled);
    assert_eq!(third.roots[0].alias, "用户保留别名");
    assert!(!third.roots[0].is_primary);
}

/// 验证 Claude 首次待验证候选失去签名时保留失败状态，而历史就绪根仍按规则移除。
#[tokio::test]
async fn invalid_claude_candidate_stays_visible_as_validation_failed() {
    let source_temp = tempfile::tempdir().expect("isolated Claude roots are available");
    let app_temp = tempfile::tempdir().expect("isolated app data is available");
    let ready_path = source_temp.path().join("ready-history");
    let pending_path = source_temp.path().join("pending-candidate");
    fs::create_dir_all(&ready_path).expect("ready root exists");
    fs::create_dir_all(&pending_path).expect("pending root exists");

    let mut index = LocalIndex::open_in_app_data(
        app_temp.path(),
        SourceClientKind::ClaudeCode.parser_version(),
    )
    .await
    .expect("Claude index opens");
    index
        .register_confirmed_root_fields(
            "claude-ready-history",
            &ready_path,
            "ready history",
            DiscoveryMethod::MetadataDiscovery,
        )
        .await
        .expect("ready history registers");
    let ready_claim = index
        .claim_background_index_roots()
        .await
        .expect("ready history is claimed");
    index
        .commit_source_file_checkpoint(
            "source-ready-history",
            "claude-ready-history",
            "projects/synthetic/session.jsonl",
            "fixture-ready-history",
            false,
            0,
            0,
            0,
            0,
            false,
            SourceClientKind::ClaudeCode.parser_version(),
            1,
            &SourceParseCheckpoint {
                thread_key: "thread-ready-history".to_owned(),
                project_key: None,
                project_label: None,
                thread_label: None,
                model: None,
                reasoning_effort: None,
                call_sequence: 0,
                adapter_state: None,
            },
        )
        .await
        .expect("ready source checkpoint commits");
    index
        .finish_background_index_roots(&ready_claim)
        .await
        .expect("history becomes ready");
    index
        .register_confirmed_root_fields(
            "claude-pending-candidate",
            &pending_path,
            "pending candidate",
            DiscoveryMethod::MetadataDiscovery,
        )
        .await
        .expect("pending candidate registers");
    drop(index);

    let output = run_scan::<InvalidClaudeCandidateTestClient>(
        app_temp.path().to_path_buf(),
        app_temp.path().to_path_buf(),
        None,
        ScanKind::Quick,
        loki_metis_core::ScanStartOrigin::ExplicitUser,
        ScanCancellation::new(),
        Box::new(|_| {}),
    )
    .await
    .expect("Claude validation scan completes");

    assert_eq!(output.roots.len(), 1);
    assert_eq!(output.roots[0].id, "claude-pending-candidate");
    assert_eq!(
        output.roots[0].activation_state,
        RootActivationState::ValidationFailed
    );
}
