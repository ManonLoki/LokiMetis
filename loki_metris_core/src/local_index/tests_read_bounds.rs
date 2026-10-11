// 本文件是 tests.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：验证 30 日读取下界与物理裁剪同时覆盖调用和累计快照，并保留来源 checkpoint。

/// 验证 30 日读取下界与物理裁剪同时覆盖调用和累计快照，并保留来源 checkpoint。
#[tokio::test]
async fn bounds_snapshot_reads_and_prunes_derived_history_without_losing_sources() {
    let temp = tempfile::tempdir().expect("isolated app-data exists");
    let index = LocalIndex::open_in_app_data(temp.path(), 8)
        .await
        .expect("index opens");
    assert!(
        !index
            .has_current_parser_usage()
            .await
            .expect("empty usage state")
    );
    let database_path = index.database_path().to_path_buf();
    drop(index);

    let fixture = reopen_for_inspection(&database_path).await;
    fixture
        .execute_unprepared(
            "INSERT INTO source_roots
               (root_id, access_path, alias, enabled, discovery_method,
                last_coverage_state, activation_state)
             VALUES ('root-retention', X'01', 'Retention', 1, 'registered', 'complete', 'ready');
             INSERT INTO source_files
               (source_id, root_id, relative_label, file_identity, archived,
                observed_size, modified_at_epoch_ms, parsed_offset, trailing_bytes,
                oversized_tail, parser_version, generation, ready, thread_key,
                project_key, model, reasoning_effort, call_sequence, token_snapshots_ready)
             VALUES
               ('source-retention', 'root-retention', 'sessions/test.jsonl', 'file', 0,
                10, 200, 10, 0, 0, 8, 1, 1, 'thread', NULL, NULL, NULL, 2, 1);
             INSERT INTO usage_calls
               (source_id, generation, logical_call_id, occurred_at_epoch_ms,
                model, reasoning_effort, project_key, thread_key, input_tokens,
                cached_input_tokens, cache_write_input_tokens, output_tokens,
                reasoning_output_tokens, total_tokens, total_is_derived, confidence)
             VALUES
               ('source-retention', 1, 'old', 99, NULL, NULL, NULL, 'thread', 1, 0, NULL, 0, 0, 1, 0, 'exact'),
               ('source-retention', 1, 'recent', 100, NULL, NULL, NULL, 'thread', 2, 0, NULL, 0, 0, 2, 0, 'exact');
             INSERT INTO usage_token_snapshots
               (source_id, generation, thread_key, occurred_at_epoch_ms,
                logical_call_id, total_tokens)
             VALUES
               ('source-retention', 1, 'thread', 99, 'old', 1),
               ('source-retention', 1, 'thread', 100, 'recent', 2);",
        )
        .await
        .expect("retention fixture is inserted");
    drop(fixture);

    let mut index = LocalIndex::open_in_app_data(temp.path(), 8)
        .await
        .expect("index reopens");
    assert!(
        index
            .has_current_parser_usage()
            .await
            .expect("source state")
    );
    let bounded = index
        .usage_snapshot_since(100)
        .await
        .expect("bounded snapshot loads");
    assert_eq!(bounded.canonical.calls.len(), 1);
    assert_eq!(bounded.canonical.calls[0].logical_call_id, "recent");
    assert_eq!(bounded.canonical.snapshots.len(), 1);
    assert_eq!(bounded.canonical.snapshots[0].logical_call_id, "recent");
    let view = index
        .usage_view_snapshot_since(100)
        .await
        .expect("view snapshot loads");
    assert_eq!(view.canonical.calls, bounded.canonical.calls);
    assert!(view.canonical.snapshots.is_empty());
    assert_eq!(view.index_state, bounded.index_state);
    assert_eq!(view.roots, bounded.roots);
    index
        .prune_usage_before(100)
        .await
        .expect("derived history is pruned");
    assert_eq!(
        index
            .canonical_calls()
            .await
            .expect("calls remain")
            .calls
            .len(),
        1
    );
    assert!(
        index
            .has_current_parser_usage()
            .await
            .expect("source remains")
    );
}
