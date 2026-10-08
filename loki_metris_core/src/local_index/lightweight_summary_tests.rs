use std::path::Path;

use sea_orm::ConnectionTrait;

use crate::{
    Confidence, SessionTokenSnapshot, SourceDiscoveryMethod, SourceRootInput, TokenUsage,
    UsageCall, build_indexed_source_roots, build_source_roots,
};

use super::{
    DiscoveryMethod, LocalErrorKind, LocalIndex, RootUsageSummaryRecord, SourceParseCheckpoint,
};

const PARSER_VERSION: u32 = 9;

/// 登记一个隔离测试根；路径只用于满足 registry 的真实持久化约束。
async fn register_root(index: &mut LocalIndex, base: &Path, root_id: &str) {
    let path = base.join(root_id);
    std::fs::create_dir_all(&path).expect("fixture root exists");
    index
        .register_root_fields(root_id, &path, root_id, DiscoveryMethod::Registered)
        .await
        .expect("fixture root registers");
}

/// 提交一个 ready 来源，并把其当前 generation 固定为测试指定值。
async fn commit_source(index: &mut LocalIndex, source_id: &str, root_id: &str, generation: u64) {
    index
        .commit_source_file_checkpoint(
            source_id,
            root_id,
            &format!("sessions/{source_id}.jsonl"),
            "fixture-file",
            false,
            100,
            1_000,
            100,
            0,
            false,
            PARSER_VERSION,
            generation,
            &SourceParseCheckpoint {
                thread_key: format!("thread-{source_id}"),
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
        .expect("fixture source commits");
}

/// 构造不含正文或真实路径的规范化调用。
fn usage_call(logical_call_id: &str, thread_key: &str) -> UsageCall {
    usage_call_with_input(logical_call_id, thread_key, 10)
}

/// 构造可用于制造冲突重复的指定输入 Token 调用。
fn usage_call_with_input(logical_call_id: &str, thread_key: &str, input_tokens: u64) -> UsageCall {
    UsageCall {
        logical_call_id: logical_call_id.to_owned(),
        occurred_at_epoch_ms: 1_000,
        model: Some("fixture-model".to_owned()),
        reasoning_effort: None,
        project_key: None,
        thread_key: thread_key.to_owned(),
        project_label: None,
        thread_label: None,
        usage: TokenUsage::new_with_availability(
            input_tokens,
            Some(2),
            None,
            3,
            Some(1),
            Some(input_tokens + 3),
        )
        .expect("fixture usage is valid"),
        adapter_consistency_key: None,
        confidence: Confidence::Exact,
        provenance: Vec::new(),
    }
}

/// 建立一个包含单一 ready 来源与合法调用的最小索引。
async fn index_with_one_call() -> (tempfile::TempDir, LocalIndex) {
    let temp = tempfile::tempdir().expect("isolated fixture exists");
    let mut index = LocalIndex::open_in_app_data(&temp.path().join("app-data"), PARSER_VERSION)
        .await
        .expect("index opens");
    register_root(&mut index, temp.path(), "root-a").await;
    commit_source(&mut index, "source-a", "root-a", 1).await;
    insert_call(&mut index, "source-a", 1, "call-a").await;
    (temp, index)
}

/// 向指定来源 generation 写入一条调用。
async fn insert_call(
    index: &mut LocalIndex,
    source_id: &str,
    generation: u64,
    logical_call_id: &str,
) {
    let mut batch = vec![usage_call(logical_call_id, &format!("thread-{source_id}"))];
    index
        .insert_usage_batch(source_id, generation, &mut batch)
        .await
        .expect("fixture call inserts");
}

/// 对照旧的全量 canonical 路径，验证轻量总计数与按根摘要在覆盖场景中一致。
async fn assert_lightweight_matches_full(index: &LocalIndex) -> Vec<RootUsageSummaryRecord> {
    let canonical = index
        .canonical_calls()
        .await
        .expect("full canonical fixture loads");
    assert_eq!(
        index
            .canonical_call_count()
            .await
            .expect("lightweight canonical count loads"),
        u64::try_from(canonical.calls.len()).expect("fixture call count fits u64")
    );

    let roots = index
        .root_usage_summary_records()
        .await
        .expect("lightweight roots load");
    let legacy_inputs = roots
        .iter()
        .map(|root| SourceRootInput {
            id: root.root.root_id.clone(),
            alias: root.root.alias.clone(),
            enabled: root.root.enabled,
            activation_state: root.root.activation_state,
            is_primary: root.root.is_primary,
            discovery_method: SourceDiscoveryMethod::Registered,
            source_file_count: root.root.source_file_count,
            call_observation_count: root.root.call_observation_count,
        })
        .collect::<Vec<_>>();
    let legacy = build_source_roots(&legacy_inputs, &canonical, "测试环境数据根");
    let lightweight = build_indexed_source_roots(&roots, "测试环境数据根");
    assert_eq!(lightweight, legacy);
    roots
}

/// 同一根内多个来源观察到同一逻辑调用时，轻量摘要仍只计一个 canonical 调用。
#[tokio::test]
async fn lightweight_counts_match_same_root_duplicates() {
    let temp = tempfile::tempdir().expect("isolated fixture exists");
    let mut index = LocalIndex::open_in_app_data(&temp.path().join("app-data"), PARSER_VERSION)
        .await
        .expect("index opens");
    register_root(&mut index, temp.path(), "root-a").await;
    commit_source(&mut index, "source-a1", "root-a", 1).await;
    commit_source(&mut index, "source-a2", "root-a", 1).await;
    insert_call(&mut index, "source-a1", 1, "shared").await;
    insert_call(&mut index, "source-a2", 1, "shared").await;

    let roots = assert_lightweight_matches_full(&index).await;
    assert_eq!(roots[0].root.call_observation_count, 2);
    assert_eq!(roots[0].canonical_call_count, 1);
    assert_eq!(
        build_indexed_source_roots(&roots, "测试环境数据根")[0].duplicate_count,
        1
    );
}

/// 跨根副本会在总量中去重，但每个参与根各保留一次 canonical 归属。
#[tokio::test]
async fn lightweight_counts_match_cross_root_provenance() {
    let temp = tempfile::tempdir().expect("isolated fixture exists");
    let mut index = LocalIndex::open_in_app_data(&temp.path().join("app-data"), PARSER_VERSION)
        .await
        .expect("index opens");
    for root_id in ["root-a", "root-b"] {
        register_root(&mut index, temp.path(), root_id).await;
        let source_id = format!("source-{root_id}");
        commit_source(&mut index, &source_id, root_id, 1).await;
        insert_call(&mut index, &source_id, 1, "shared").await;
    }

    let roots = assert_lightweight_matches_full(&index).await;
    assert_eq!(index.canonical_call_count().await.unwrap(), 1);
    assert!(roots.iter().all(|root| root.canonical_call_count == 1));
    assert!(
        build_indexed_source_roots(&roots, "测试环境数据根")
            .iter()
            .all(|root| root.duplicate_count == 0)
    );
}

/// 停用根的调用不进入 canonical 总量，根摘要也保持既有的零重复语义。
#[tokio::test]
async fn lightweight_counts_exclude_disabled_roots() {
    let temp = tempfile::tempdir().expect("isolated fixture exists");
    let mut index = LocalIndex::open_in_app_data(&temp.path().join("app-data"), PARSER_VERSION)
        .await
        .expect("index opens");
    for root_id in ["root-enabled", "root-disabled"] {
        register_root(&mut index, temp.path(), root_id).await;
        let source_id = format!("source-{root_id}");
        commit_source(&mut index, &source_id, root_id, 1).await;
        insert_call(&mut index, &source_id, 1, root_id).await;
    }
    index
        .set_root_enabled("root-disabled", false)
        .await
        .expect("fixture root is disabled");

    let roots = assert_lightweight_matches_full(&index).await;
    assert_eq!(index.canonical_call_count().await.unwrap(), 1);
    let disabled = roots
        .iter()
        .find(|record| record.root.root_id == "root-disabled")
        .expect("disabled root remains listed");
    assert_eq!(disabled.root.call_observation_count, 1);
    assert_eq!(disabled.canonical_call_count, 0);
    let disabled_summary = build_indexed_source_roots(&roots, "测试环境数据根")
        .into_iter()
        .find(|root| root.id == "root-disabled")
        .expect("disabled root summary remains listed");
    assert_eq!(disabled_summary.duplicate_count, 0);
}

/// 同一来源残留的旧 generation 观察不得进入轻量总量或根重复计数。
#[tokio::test]
async fn lightweight_counts_only_current_generation() {
    let temp = tempfile::tempdir().expect("isolated fixture exists");
    let mut index = LocalIndex::open_in_app_data(&temp.path().join("app-data"), PARSER_VERSION)
        .await
        .expect("index opens");
    register_root(&mut index, temp.path(), "root-a").await;
    commit_source(&mut index, "source-a", "root-a", 2).await;
    insert_call(&mut index, "source-a", 1, "stale-generation").await;
    insert_call(&mut index, "source-a", 2, "current-generation").await;

    let roots = assert_lightweight_matches_full(&index).await;
    assert_eq!(index.canonical_call_count().await.unwrap(), 1);
    assert_eq!(roots[0].root.call_observation_count, 1);
    assert_eq!(roots[0].canonical_call_count, 1);
}

/// Token 冲突只降低 canonical 置信度，不改变总调用数或按根归属。
#[tokio::test]
async fn lightweight_counts_match_conflicting_logical_call_ids() {
    let temp = tempfile::tempdir().expect("isolated fixture exists");
    let mut index = LocalIndex::open_in_app_data(&temp.path().join("app-data"), PARSER_VERSION)
        .await
        .expect("index opens");
    for (root_id, input_tokens) in [("root-a", 10), ("root-b", 20)] {
        register_root(&mut index, temp.path(), root_id).await;
        let source_id = format!("source-{root_id}");
        commit_source(&mut index, &source_id, root_id, 1).await;
        let mut batch = vec![usage_call_with_input(
            "conflicting-shared",
            &format!("thread-{source_id}"),
            input_tokens,
        )];
        index
            .insert_usage_batch(&source_id, 1, &mut batch)
            .await
            .expect("conflicting fixture inserts");
    }

    let canonical = index
        .canonical_calls()
        .await
        .expect("conflicting canonical fixture loads");
    assert_eq!(canonical.calls.len(), 1);
    assert_eq!(canonical.warnings.len(), 1);
    let roots = assert_lightweight_matches_full(&index).await;
    assert!(roots.iter().all(|root| root.canonical_call_count == 1));
}

/// 只有 cumulative 快照时，调用数与根重复数仍为零且轻量路径不会忽略完整性。
#[tokio::test]
async fn lightweight_counts_match_snapshot_only_sources() {
    let temp = tempfile::tempdir().expect("isolated fixture exists");
    let mut index = LocalIndex::open_in_app_data(&temp.path().join("app-data"), PARSER_VERSION)
        .await
        .expect("index opens");
    register_root(&mut index, temp.path(), "root-a").await;
    commit_source(&mut index, "source-a", "root-a", 1).await;
    let mut calls = Vec::new();
    let mut snapshots = vec![SessionTokenSnapshot {
        thread_key: "thread-source-a".to_owned(),
        occurred_at_epoch_ms: 1_000,
        cumulative_total_tokens: 13,
        logical_call_id: "snapshot-only".to_owned(),
        model: Some("fixture-model".to_owned()),
        reasoning_effort: None,
        project_key: None,
        provenance: Vec::new(),
    }];
    index
        .insert_usage_and_snapshots("source-a", 1, &mut calls, &mut snapshots)
        .await
        .expect("snapshot-only fixture inserts");

    let canonical = index
        .canonical_calls()
        .await
        .expect("snapshot-only canonical fixture loads");
    assert!(canonical.calls.is_empty());
    assert_eq!(canonical.snapshots.len(), 1);
    let roots = assert_lightweight_matches_full(&index).await;
    assert_eq!(index.canonical_call_count().await.unwrap(), 0);
    assert_eq!(roots[0].root.call_observation_count, 0);
    assert_eq!(roots[0].canonical_call_count, 0);
}

/// 负 Token 持久行必须像旧全量解码一样在轻量计数阶段 fail-closed。
#[tokio::test]
async fn lightweight_count_rejects_negative_persisted_tokens() {
    let (_temp, index) = index_with_one_call().await;
    index
        .connection
        .execute_unprepared("UPDATE usage_calls SET input_tokens = -1")
        .await
        .expect("fixture row is corrupted");
    assert_eq!(
        index.canonical_calls().await.unwrap_err().kind(),
        LocalErrorKind::Database
    );
    assert_eq!(
        index.canonical_call_count().await.unwrap_err().kind(),
        LocalErrorKind::Database
    );
    assert_eq!(
        index.root_usage_summary_records().await.unwrap_err().kind(),
        LocalErrorKind::Database
    );
}

/// 非法 confidence 标签必须像旧全量解码一样在轻量计数阶段 fail-closed。
#[tokio::test]
async fn lightweight_count_rejects_invalid_confidence() {
    let (_temp, index) = index_with_one_call().await;
    index
        .connection
        .execute_unprepared("UPDATE usage_calls SET confidence = 'invalid'")
        .await
        .expect("fixture confidence is corrupted");
    assert_eq!(
        index.canonical_calls().await.unwrap_err().kind(),
        LocalErrorKind::Database
    );
    assert_eq!(
        index.canonical_call_count().await.unwrap_err().kind(),
        LocalErrorKind::Database
    );
    assert_eq!(
        index.root_usage_summary_records().await.unwrap_err().kind(),
        LocalErrorKind::Database
    );
}

/// Token 子集或显式总量违反业务不变量时，轻量路径保持 InvalidUsage 分类。
#[tokio::test]
async fn lightweight_count_rejects_invalid_token_relationships() {
    let (_temp, index) = index_with_one_call().await;
    index
        .connection
        .execute_unprepared("UPDATE usage_calls SET cached_input_tokens = input_tokens + 1")
        .await
        .expect("fixture token relationship is corrupted");
    assert_eq!(
        index.canonical_calls().await.unwrap_err().kind(),
        LocalErrorKind::InvalidUsage
    );
    assert_eq!(
        index.canonical_call_count().await.unwrap_err().kind(),
        LocalErrorKind::InvalidUsage
    );
    assert_eq!(
        index.root_usage_summary_records().await.unwrap_err().kind(),
        LocalErrorKind::InvalidUsage
    );
}

/// 损坏的 snapshot 即使不贡献 call_count，也必须保持旧读取路径的拒绝语义。
#[tokio::test]
async fn lightweight_count_rejects_corrupt_snapshots() {
    let (_temp, mut index) = index_with_one_call().await;
    let mut calls = Vec::new();
    let mut snapshots = vec![SessionTokenSnapshot {
        thread_key: "thread-source-a".to_owned(),
        occurred_at_epoch_ms: 1_000,
        cumulative_total_tokens: 13,
        logical_call_id: "snapshot-a".to_owned(),
        model: None,
        reasoning_effort: None,
        project_key: None,
        provenance: Vec::new(),
    }];
    index
        .insert_usage_and_snapshots("source-a", 1, &mut calls, &mut snapshots)
        .await
        .expect("snapshot fixture inserts");
    index
        .connection
        .execute_unprepared("UPDATE usage_token_snapshots SET total_tokens = -1")
        .await
        .expect("snapshot fixture is corrupted");
    assert_eq!(
        index.canonical_calls().await.unwrap_err().kind(),
        LocalErrorKind::Database
    );
    assert_eq!(
        index.canonical_call_count().await.unwrap_err().kind(),
        LocalErrorKind::Database
    );
    assert_eq!(
        index.root_usage_summary_records().await.unwrap_err().kind(),
        LocalErrorKind::Database
    );
}
