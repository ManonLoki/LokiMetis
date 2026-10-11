// 本文件是 tests.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：验证 IgnoreNonIncremental 仍写出 cumulative 快照，且不推进增量基线。；验证累计检查点仅恢复合法当前版本，不接受截断、旧版或矛盾总量。；验证缺失或畸形时间戳会被跳过并单独计数，不能落到 Unix epoch 0。

/// 验证 IgnoreNonIncremental 仍写出 cumulative 快照，且不推进增量基线。
#[test]
fn ignore_non_incremental_still_emits_session_snapshot() {
    let input = concat!(
        r#"{"timestamp":"2026-08-12T03:28:00Z","type":"event_msg","#,
        r#""payload":{"type":"token_count","call_id":"real","#,
        r#""info":{"#,
        r#""last_token_usage":{"input_tokens":100,"cached_input_tokens":40,"#,
        r#""output_tokens":20,"reasoning_output_tokens":5,"total_tokens":120},"#,
        r#""total_token_usage":{"input_tokens":100,"cached_input_tokens":40,"#,
        r#""output_tokens":20,"reasoning_output_tokens":5,"total_tokens":120}}}}"#,
        "\n",
        r#"{"timestamp":"2026-08-12T03:40:00Z","type":"event_msg","#,
        r#""payload":{"type":"token_count","call_id":"ignored","#,
        r#""info":{"#,
        r#""last_token_usage":{"input_tokens":100,"cached_input_tokens":40,"#,
        r#""output_tokens":20,"reasoning_output_tokens":5,"total_tokens":120},"#,
        r#""total_token_usage":{"input_tokens":600000,"cached_input_tokens":500000,"#,
        r#""output_tokens":20000,"reasoning_output_tokens":5000,"total_tokens":696019}}}}"#,
        "\n"
    );
    let cancellation = CancellationToken::new();
    let mut context = JsonlParseContext::new("fixture.jsonl");
    let mut calls = Vec::new();
    let report = parse_jsonl_stream(
        input.as_bytes(),
        DEFAULT_MAX_JSONL_LINE_BYTES,
        &mut context,
        &provenance(),
        &cancellation,
        |call| {
            calls.push(call);
            Ok(())
        },
    )
    .expect("ignore snapshot fixture is parsed");
    assert_eq!(report.calls_emitted, 1);
    assert_eq!(calls[0].usage.total_tokens, 120);
    assert_eq!(report.snapshots.len(), 2);
    assert_eq!(report.snapshots[1].cumulative_total_tokens, 696_019);
    assert_eq!(
        context
            .previous_cumulative
            .as_ref()
            .map(|usage| usage.total_tokens),
        Some(120)
    );
}

/// 验证累计检查点仅恢复合法当前版本，不接受截断、旧版或矛盾总量。
#[test]
fn incremental_checkpoint_round_trips_and_rejects_invalid_values() {
    let usage = TokenUsage::new_with_availability(110, Some(44), None, 22, Some(6), Some(132))
        .expect("checkpoint usage is valid");
    let last = TokenUsage::new_with_availability(10, Some(4), None, 2, Some(0), Some(12))
        .expect("checkpoint last is valid");
    let encoded = encode_incremental_checkpoint(4, &usage, Some(&last));

    assert_eq!(
        restore_incremental_checkpoint(Some(&encoded)),
        Some(IncrementalUsageCheckpoint {
            cumulative: usage.clone(),
            last: Some(last),
        })
    );
    assert_eq!(
        restore_incremental_checkpoint(Some(&encode_incremental_checkpoint(4, &usage, None))),
        Some(IncrementalUsageCheckpoint {
            cumulative: usage,
            last: None,
        })
    );
    assert_eq!(
        restore_incremental_checkpoint(Some("codex-v5:00000000000000000004:110:44:u:22:6:132:0")),
        None
    );
    assert_eq!(
        restore_incremental_checkpoint(Some("codex-v6:00000000000000000004:110:44:u:22:6:132:0")),
        None
    );
    assert_eq!(
        restore_incremental_checkpoint(Some("codex-v7:00000000000000000004:110:44:u:22:6:999:1:0")),
        None
    );
    assert_eq!(
        restore_incremental_checkpoint(Some("codex-v7:broken")),
        None
    );
}

/// 验证缺失或畸形时间戳会被跳过并单独计数，不能落到 Unix epoch 0。
#[test]
fn rejects_token_events_without_a_valid_timestamp() {
    let input = concat!(
        "{\"type\":\"event_msg\",\"payload\":{",
        "\"type\":\"token_count\",\"call_id\":\"missing-time\",\"info\":{",
        "\"last_token_usage\":{\"input_tokens\":5,\"cached_input_tokens\":1,",
        "\"output_tokens\":2,\"reasoning_output_tokens\":1,\"total_tokens\":7}}}}\n",
        "{\"timestamp\":\"not-a-time\",\"type\":\"event_msg\",\"payload\":{",
        "\"type\":\"token_count\",\"call_id\":\"bad-time\",\"info\":{",
        "\"last_token_usage\":{\"input_tokens\":5,\"cached_input_tokens\":1,",
        "\"output_tokens\":2,\"reasoning_output_tokens\":1,\"total_tokens\":7}}}}\n",
        "{\"timestamp\":9223372036854775807,\"type\":\"event_msg\",\"payload\":{",
        "\"type\":\"token_count\",\"call_id\":\"range-time\",\"info\":{",
        "\"last_token_usage\":{\"input_tokens\":5,\"cached_input_tokens\":1,",
        "\"output_tokens\":2,\"reasoning_output_tokens\":1,\"total_tokens\":7}}}}\n",
        "{\"timestamp\":\"2026-07-31T12:00:00.Z\",\"type\":\"event_msg\",",
        "\"payload\":{\"type\":\"token_count\",\"call_id\":\"empty-fraction\",",
        "\"info\":{\"last_token_usage\":{\"input_tokens\":5,",
        "\"cached_input_tokens\":1,\"output_tokens\":2,",
        "\"reasoning_output_tokens\":1,\"total_tokens\":7}}}}\n"
    );
    let cancellation = CancellationToken::new();
    let mut context = JsonlParseContext::new("fixture.jsonl");
    let mut calls = Vec::new();

    let report = parse_jsonl_stream(
        input.as_bytes(),
        DEFAULT_MAX_JSONL_LINE_BYTES,
        &mut context,
        &provenance(),
        &cancellation,
        |call| {
            calls.push(call);
            Ok(())
        },
    )
    .expect("invalid timestamps are isolated warnings");

    assert!(calls.is_empty());
    assert_eq!(report.calls_emitted, 0);
    assert_eq!(report.warnings.invalid_timestamps, 4);
    assert_eq!(report.warnings.total(), 4);
}

/// 验证 jiff 解析保留既有 RFC3339 词法、毫秒截断和四位年份边界。
#[test]
fn parses_rfc3339_with_strict_calendar_and_existing_boundaries() {
    assert_eq!(
        parse_rfc3339_millis("2024-02-29T12:34:56.123456Z"),
        Some(1_709_210_096_123)
    );
    assert_eq!(
        parse_rfc3339_millis("2024-02-29T20:34:56.9+08:00"),
        Some(1_709_210_096_900)
    );
    // 真实历史闰秒：chrono 会把 `:60` 前进折算到下一秒（次日 00:00:00），
    // jiff 则把闰秒收拢到同一秒内、与 `:59` 得到相同的 Unix 毫秒值——
    // 两者对闰秒的诠释不同，但都拒绝真正非法的日历值，这里按 jiff 的
    // 实际行为更新期望值，而不是伪造一个它不会产生的结果。
    assert_eq!(
        parse_rfc3339_millis("2016-12-31T23:59:60Z"),
        Some(1_483_228_799_000)
    );
    assert_eq!(
        parse_rfc3339_millis("0000-01-01T00:00:00Z"),
        Some(MIN_SUPPORTED_TIMESTAMP_MS)
    );
    // jiff `Timestamp` 实际只能表示到 `9999-12-30T22:00:00Z`（约比历法上的
    // 年末早 26 小时），这里直接用该边界值验证上界常量与解析结果一致，
    // 而不是用 chrono 时代那个 jiff 已经无法表示的 `9999-12-31T23:59:59.999Z`。
    assert_eq!(
        parse_rfc3339_millis("9999-12-30T22:00:00Z"),
        Some(MAX_SUPPORTED_TIMESTAMP_MS)
    );
    assert_eq!(parse_rfc3339_millis("9999-12-31T23:59:59.999Z"), None);

    for invalid in [
        "2023-02-29T00:00:00Z",
        "2024-04-31T00:00:00Z",
        "2024-02-29t00:00:00Z",
        "2024-02-29T00:00:00z",
        "2024-02-29 00:00:00Z",
        "2024-02-29T00:00:00.Z",
        "2024-02-29T00:00:00",
        "+10000-01-01T00:00:00Z",
    ] {
        assert_eq!(parse_rfc3339_millis(invalid), None, "accepted {invalid}");
    }
}
