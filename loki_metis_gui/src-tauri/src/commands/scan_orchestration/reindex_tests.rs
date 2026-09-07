//! 单根重新索引的跨层回归：目标根生成新 generation，其他根保持不变。

use std::fs;
use std::path::{Path, PathBuf};

use loki_metis_core::{ScanCancellation, SourceClientKind, path_key, stable_id};

use super::client::CodexClient;
use super::run::run_reindex;
use crate::backend::local_index::{
    CancellationToken, DiscoveryInputs, LocalIndex, PARSER_VERSION, RegisteredRoot, ScanConfig,
    discover_quick, scan_discovered_roots,
};
use crate::dto::AgentClientKindDto;
use crate::runtime::AppRuntimeState;
use crate::source_commands::validated_source_root_reindex_request;

/// 创建一个只含单份合法 rollout 的隔离 Codex 根，并返回对应来源 ID。
fn create_registered_root(parent: &Path, name: &str, root_id: &str) -> (RegisteredRoot, String) {
    let root = parent.join(name);
    fs::create_dir_all(root.join("sessions")).expect("sessions directory is created");
    let relative = PathBuf::from("sessions/rollout-test.jsonl");
    let observed_at = jiff::Timestamp::now();
    let content = format!(
        concat!(
            "{{\"timestamp\":\"{observed_at}\",\"type\":\"session_meta\",",
            "\"payload\":{{\"id\":\"session-{name}\",\"cwd\":\"/private/project\"}}}}\n",
            "{{\"timestamp\":\"{observed_at}\",\"type\":\"event_msg\",",
            "\"payload\":{{\"type\":\"token_count\",\"call_id\":\"call-{name}\",",
            "\"info\":{{\"last_token_usage\":{{\"input_tokens\":10,",
            "\"cached_input_tokens\":2,\"output_tokens\":4,\"total_tokens\":14}}}}}}}}\n"
        ),
        name = name,
        observed_at = observed_at,
    );
    fs::write(root.join(&relative), content).expect("rollout fixture is written");
    let source_id = stable_id("source", &format!("{root_id}\u{0}{}", path_key(&relative)));
    (
        RegisteredRoot {
            root_id: Some(root_id.to_owned()),
            path: root,
            alias: name.to_owned(),
            enabled: true,
        },
        source_id,
    )
}

/// 重新索引只扫描目标根并切换其 generation，不重复累计或改写其他根。
#[tokio::test]
async fn reindex_targets_one_root_and_preserves_other_generations() {
    let source_temp = tempfile::tempdir().expect("isolated source area is available");
    let app_temp = tempfile::tempdir().expect("isolated app-data is available");
    let target_id = "root-1111111111111111";
    let other_id = "root-2222222222222222";
    let (target, target_source_id) =
        create_registered_root(source_temp.path(), "target", target_id);
    let (other, other_source_id) = create_registered_root(source_temp.path(), "other", other_id);
    let discovered = discover_quick(
        &DiscoveryInputs {
            registered_roots: vec![target, other],
            ..DiscoveryInputs::default()
        },
        &CancellationToken::new(),
    );
    assert_eq!(discovered.roots.len(), 2);

    let mut index = LocalIndex::open_in_app_data(app_temp.path(), PARSER_VERSION)
        .await
        .expect("index opens");
    let initial = scan_discovered_roots(
        &mut index,
        &discovered.roots,
        ScanConfig::default(),
        &CancellationToken::new(),
        |_| {},
    )
    .await
    .expect("initial scan succeeds");
    assert_eq!(initial.call_count, 2);
    let target_generation = index
        .stored_source_file(&target_source_id)
        .await
        .expect("target checkpoint loads")
        .expect("target checkpoint exists")
        .generation;
    let other_generation = index
        .stored_source_file(&other_source_id)
        .await
        .expect("other checkpoint loads")
        .expect("other checkpoint exists")
        .generation;
    drop(index);

    let state = AppRuntimeState::new(app_temp.path().to_path_buf());
    let request =
        validated_source_root_reindex_request(&state, AgentClientKindDto::Codex, target_id)
            .await
            .expect("enabled target produces a validated request");
    let output = run_reindex::<CodexClient>(
        app_temp.path().to_path_buf(),
        app_temp.path().to_path_buf(),
        None,
        request,
        ScanCancellation::new(),
        Box::new(|_| {}),
    )
    .await
    .expect("targeted reindex succeeds");
    assert_eq!(output.files_scanned, 1);
    assert_eq!(output.call_count, 2);

    let reopened =
        LocalIndex::open_in_app_data(app_temp.path(), SourceClientKind::Codex.parser_version())
            .await
            .expect("index reopens");
    assert_eq!(
        reopened
            .stored_source_file(&target_source_id)
            .await
            .expect("target checkpoint reloads")
            .expect("target checkpoint remains")
            .generation,
        target_generation + 1
    );
    assert_eq!(
        reopened
            .stored_source_file(&other_source_id)
            .await
            .expect("other checkpoint reloads")
            .expect("other checkpoint remains")
            .generation,
        other_generation
    );
}
