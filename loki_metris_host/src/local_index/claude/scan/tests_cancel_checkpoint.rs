// 本文件是 tests.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：验证扫描被取消后保存的检查点，下次能从断点续扫而不是被判定为无变化。；测试辅助：把单个路径作为已登记根跑一次快速发现，返回确认结果。；测试辅助：写入一条（或追加一条）合成 assistant 用量观察记录。

/// 验证扫描被取消后保存的检查点，下次能从断点续扫而不是被判定为无变化。
#[test]
fn cancelled_claude_checkpoint_resumes_instead_of_becoming_unchanged() {
    let temp = tempdir().expect("isolated fixture is available");
    let root_path = temp.path().join(".claude");
    let transcript = root_path.join(format!("projects/project-a/{SESSION}.jsonl"));
    write_observation(&transcript, "msg-first", 1, 0, 0, 1, false);
    let root = discover_roots(root_path)
        .into_iter()
        .next()
        .expect("Claude root is discovered");
    let mut index = open_index(&temp.path().join("app-data"), CLAUDE_PARSER_VERSION);
    block_on(index.register_claude_root(&root)).expect("root registers");

    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let first = block_on(index.index_claude_transcript(
        &root,
        &transcript,
        1024 * 1024,
        i64::MIN,
        false,
        &cancelled,
    ))
    .expect("pre-cancelled first scan keeps a resumable checkpoint");
    assert!(first.cancelled);
    assert!(!first.unchanged);
    let first_checkpoint = block_on(index.stored_source_file(&first.source_id))
        .expect("checkpoint loads")
        .expect("checkpoint exists");
    assert_eq!(first_checkpoint.observed_size, 0);

    let resumed = block_on(index.index_claude_transcript(
        &root,
        &transcript,
        1024 * 1024,
        i64::MIN,
        false,
        &CancellationToken::new(),
    ))
    .expect("first scan resumes");
    assert!(!resumed.unchanged);
    assert_eq!(
        block_on(index.aggregate())
            .expect("first call is indexed")
            .call_count,
        1
    );

    write_observation(&transcript, "msg-second", 2, 0, 0, 1, true);
    let append_cancelled = CancellationToken::new();
    append_cancelled.cancel();
    let partial_append = block_on(index.index_claude_transcript(
        &root,
        &transcript,
        1024 * 1024,
        i64::MIN,
        false,
        &append_cancelled,
    ))
    .expect("cancelled append preserves its prior ready generation");
    assert!(partial_append.cancelled);
    assert_eq!(
        block_on(index.aggregate())
            .expect("old call remains")
            .call_count,
        1
    );

    let append_resumed = block_on(index.index_claude_transcript(
        &root,
        &transcript,
        1024 * 1024,
        i64::MIN,
        false,
        &CancellationToken::new(),
    ))
    .expect("append resumes");
    assert!(!append_resumed.unchanged);
    assert_eq!(
        block_on(index.aggregate())
            .expect("both calls are indexed")
            .call_count,
        2
    );
}

/// 测试辅助：把单个路径作为已登记根跑一次快速发现，返回确认结果。
fn discover_roots(path: std::path::PathBuf) -> Vec<ClaudeDiscoveredRoot> {
    discover_claude_quick(
        &ClaudeDiscoveryInputs {
            registered_roots: vec![RegisteredRoot {
                root_id: None,
                path,
                alias: "Claude 测试根".to_owned(),
                enabled: true,
            }],
            ..ClaudeDiscoveryInputs::default()
        },
        &CancellationToken::new(),
    )
    .roots
}

/// 测试辅助：写入一条（或追加一条）合成 assistant 用量观察记录。
fn write_observation(
    path: &std::path::Path,
    message_id: &str,
    input: u64,
    cached: u64,
    cache_write: u64,
    output: u64,
    append: bool,
) {
    fs::create_dir_all(path.parent().expect("transcript has parent"))
        .expect("transcript parent is created");
    let line = format!(
        "{}\n",
        serde_json::json!({
            "type": "assistant",
            "sessionId": SESSION,
            "timestamp": "2026-07-31T08:00:00Z",
            "message": {
                "id": message_id,
                "model": "claude-test",
                "content": "privacy bait",
                "usage": {
                    "input_tokens": input,
                    "cache_read_input_tokens": cached,
                    "cache_creation_input_tokens": cache_write,
                    "output_tokens": output,
                }
            }
        })
    );
    if append {
        OpenOptions::new()
            .append(true)
            .open(path)
            .expect("transcript opens for append")
            .write_all(line.as_bytes())
            .expect("observation appends");
    } else {
        fs::write(path, line).expect("transcript is written");
    }
}
