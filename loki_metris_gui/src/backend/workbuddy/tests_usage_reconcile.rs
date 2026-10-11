// 本文件是 tests.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：统计页和模型表必须由同一批 project JSONL 事件构造并相互对账。；活跃文件末尾没有换行的半条记录必须留待下次读取，不产生错误或虚假用量。；损坏行与超长完整行要降级覆盖，但不得阻断其后的正常记录。

/// 统计页和模型表必须由同一批 project JSONL 事件构造并相互对账。
#[tokio::test]
async fn usage_details_reconciles_exact_components_and_models() {
    let temp = tempdir().expect("isolated dir exists");
    let home = fixture_home(temp.path());
    let now = epoch_ms("2026-09-04T12:00:00Z");
    write_lines(
        &home.join("projects/project-a/session-a.jsonl"),
        &[
            usage_event(now - 1_000, "s1", "m1", "alpha", 30, 20, 3, 1, 0.3),
            usage_event(now, "s1", "m2", "beta", 70, 40, 7, 2, 0.7),
        ],
    );

    let details = read_workbuddy_usage_details(
        &[home.clone()],
        LocalUsageWindow::ThisMonth,
        UsageDimension::Model,
        now,
        TimeStandard::utc(),
    )
    .await
    .expect("usage details page builds");

    assert_eq!(details.statistics.fact.value.call_count, 3);
    assert_eq!(details.statistics.fact.value.tokens.input_tokens, 100);
    assert_eq!(
        details.statistics.fact.value.tokens.cached_input_tokens,
        Some(60)
    );
    assert_eq!(details.statistics.fact.value.tokens.output_tokens, 10);
    assert_eq!(details.statistics.fact.value.tokens.total_tokens, 110);
    assert_eq!(details.statistics.groups.len(), 2);
    assert_eq!(details.model_usage.window, LocalUsageWindow::ThisMonth);
    assert_eq!(details.model_usage.groups.len(), 2);
    assert_eq!(
        details
            .model_usage
            .groups
            .iter()
            .map(|group| group.call_count)
            .sum::<u64>(),
        details.statistics.fact.value.call_count
    );
    assert_eq!(
        details
            .model_usage
            .groups
            .iter()
            .map(|group| group.total_tokens)
            .sum::<u64>(),
        details.statistics.fact.value.tokens.total_tokens
    );
}

/// 活跃文件末尾没有换行的半条记录必须留待下次读取，不产生错误或虚假用量。
#[tokio::test]
async fn ignores_unterminated_tail_without_degrading_coverage() {
    let temp = tempdir().expect("isolated dir exists");
    let home = fixture_home(temp.path());
    let now = epoch_ms("2026-09-04T12:00:00Z");
    let path = home.join("projects/project-a/session-a.jsonl");
    fs::create_dir_all(path.parent().expect("fixture file has parent"))
        .expect("fixture parent exists");
    let mut file = File::create(&path).expect("fixture file opens");
    writeln!(
        file,
        "{}",
        usage_event(now, "s1", "m1", "alpha", 10, 4, 2, 1, 0.1)
    )
    .expect("complete fixture line writes");
    write!(
        file,
        "{}",
        usage_event(now, "s1", "m2", "beta", 900, 800, 90, 1, 9.0)
    )
    .expect("unterminated fixture tail writes");
    drop(file);

    let snapshot = read_workbuddy_statistics(&[home.clone()], now, TimeStandard::utc())
        .await
        .expect("statistics compute succeeds");
    assert_eq!(snapshot.coverage.state, CoverageState::Complete);
    assert_eq!(snapshot.coverage.warning_count, 0);
    assert_eq!(snapshot.total_requests, 1);
    assert_eq!(snapshot.total_tokens, 12);
    assert_eq!(
        model_window(&home, LocalUsageWindow::Today, now)
            .await
            .groups
            .len(),
        1
    );
}

/// 损坏行与超长完整行要降级覆盖，但不得阻断其后的正常记录。
#[tokio::test]
async fn malformed_and_oversized_lines_are_partial_and_recovery_continues() {
    let temp = tempdir().expect("isolated dir exists");
    let home = fixture_home(temp.path());
    let now = epoch_ms("2026-09-04T12:00:00Z");
    let path = home.join("projects/project-a/session-a.jsonl");
    fs::create_dir_all(path.parent().expect("fixture file has parent"))
        .expect("fixture parent exists");
    let mut file = File::create(&path).expect("fixture file opens");
    writeln!(file, "{{malicious-not-json}}").expect("malformed line writes");
    let oversized = json!({"padding": "x".repeat(1024 * 1024)}).to_string();
    writeln!(file, "{oversized}").expect("oversized line writes");
    writeln!(
        file,
        "{}",
        usage_event(now, "s1", "valid-after-errors", "alpha", 20, 15, 3, 1, 0.2)
    )
    .expect("valid recovery line writes");
    drop(file);

    let snapshot = read_workbuddy_statistics(&[home.clone()], now, TimeStandard::utc())
        .await
        .expect("partial statistics remain usable");
    assert_eq!(snapshot.coverage.state, CoverageState::Partial);
    assert_eq!(snapshot.coverage.warning_count, 2);
    assert_eq!(snapshot.total_requests, 1);
    assert_eq!(snapshot.total_input_tokens, 20);
    assert_eq!(snapshot.total_cached_input_tokens, 15);
    assert_eq!(snapshot.total_output_tokens, 3);
    assert_eq!(snapshot.total_tokens, 23);
}

/// 单个 JSONL 超过旧 2 MiB 整文件上限时，末端正常记录仍必须被流式读取。
#[tokio::test]
async fn reads_usage_after_two_megabytes_in_one_file() {
    let temp = tempdir().expect("isolated dir exists");
    let home = fixture_home(temp.path());
    let now = epoch_ms("2026-09-04T12:00:00Z");
    let path = home.join("projects/project-a/session-a.jsonl");
    fs::create_dir_all(path.parent().expect("fixture file has parent"))
        .expect("fixture parent exists");
    let mut file = File::create(&path).expect("fixture file opens");
    let ignored = json!({"padding": "x".repeat(750_000)}).to_string();
    for _ in 0..3 {
        writeln!(file, "{ignored}").expect("large ignored line writes");
    }
    writeln!(
        file,
        "{}",
        usage_event(now, "s1", "after-two-mib", "alpha", 25, 20, 5, 1, 0.25)
    )
    .expect("trailing usage line writes");
    drop(file);
    assert!(fs::metadata(&path).expect("fixture metadata").len() > 2 * 1024 * 1024);

    let snapshot = read_workbuddy_statistics(&[home.clone()], now, TimeStandard::utc())
        .await
        .expect("large file statistics compute succeeds");
    assert_eq!(snapshot.coverage.state, CoverageState::Complete);
    assert_eq!(snapshot.total_requests, 1);
    assert_eq!(snapshot.total_tokens, 30);
}

/// 尚未产生 `projects` 布局的固定根只被跳过；全部根都如此时按来源不可用处理。
#[tokio::test]
async fn roots_without_projects_layout_are_skipped_until_none_remain() {
    let temp = tempdir().expect("isolated dir exists");
    let now = epoch_ms("2026-09-04T12:00:00Z");
    let domestic = temp.path().join(WORKBUDDY_HOME_DIR_NAME);
    let international = temp.path().join(WORKBUDDY_INTERNATIONAL_HOME_DIR_NAME);
    fs::create_dir_all(&domestic).expect("domestic fixture home exists");
    fs::create_dir_all(&international).expect("international fixture home exists");

    // 两个根都还没有用量布局：整体按未安装来源处理，逐根探测也同为不可用。
    let result = read_workbuddy_statistics(
        &[domestic.clone(), international.clone()],
        now,
        TimeStandard::utc(),
    )
    .await;
    assert_eq!(result.unwrap_err(), WorkbuddyReadError::SourceUnavailable);
    assert_eq!(
        single_source_evidence(&domestic).unwrap_err(),
        WorkbuddyReadError::SourceUnavailable
    );
    assert_eq!(
        single_source_evidence(&international).unwrap_err(),
        WorkbuddyReadError::SourceUnavailable
    );

    // 只有国际版产生布局后，该根独立贡献用量，缺失的国内版不影响结果。
    write_lines(
        &international.join("projects/project-a/session-a.jsonl"),
        &[usage_event(now, "s1", "m1", "alpha", 10, 5, 1, 1, 0.1)],
    );
    let snapshot = read_workbuddy_statistics(&[domestic, international], now, TimeStandard::utc())
        .await
        .expect("international-only usage remains readable");
    assert_eq!(snapshot.coverage.state, CoverageState::Complete);
    assert_eq!(snapshot.coverage.roots_scanned, 1);
    assert_eq!(snapshot.total_requests, 1);
    assert_eq!(snapshot.total_tokens, 11);
}

/// 已存在但链接型的 `projects` 根是安全读取失败，并使整个来源失败关闭。
#[cfg(unix)]
#[tokio::test]
async fn linked_projects_root_fails_the_whole_source_closed() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().expect("isolated dir exists");
    let now = epoch_ms("2026-09-04T12:00:00Z");
    let domestic = temp.path().join(WORKBUDDY_HOME_DIR_NAME);
    let outside = temp.path().join("outside-projects");
    fs::create_dir_all(&domestic).expect("workbuddy fixture home exists");
    fs::create_dir_all(&outside).expect("outside fixture exists");
    symlink(&outside, domestic.join("projects")).expect("projects symlink creates");

    let result =
        read_workbuddy_statistics(std::slice::from_ref(&domestic), now, TimeStandard::utc()).await;
    assert_eq!(result.unwrap_err(), WorkbuddyReadError::Read);
    assert_eq!(
        single_source_evidence(&domestic).unwrap_err(),
        WorkbuddyReadError::Read
    );

    // 另一个根即使完全健康，也不能把被链接根的部分结果伪装成完整统计。
    let international = fixture_international_home(temp.path());
    write_lines(
        &international.join("projects/project-a/session-a.jsonl"),
        &[usage_event(now, "s1", "m1", "alpha", 10, 5, 1, 1, 0.1)],
    );
    let result =
        read_workbuddy_statistics(&[domestic, international], now, TimeStandard::utc()).await;
    assert_eq!(result.unwrap_err(), WorkbuddyReadError::Read);
}

/// 符号链接 transcript 必须被拒绝，链接目标不得贡献任何用量。
#[cfg(unix)]
#[tokio::test]
async fn rejects_symlinked_project_transcripts() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().expect("isolated dir exists");
    let home = fixture_home(temp.path());
    let now = epoch_ms("2026-09-04T12:00:00Z");
    let project_dir = home.join("projects/project-a");
    let kept = usage_event(now, "s1", "kept", "alpha", 10, 5, 1, 1, 0.1);
    write_lines(&project_dir.join("session-a.jsonl"), &[kept]);
    let outside = temp.path().join("outside.jsonl");
    write_lines(
        &outside,
        &[usage_event(
            now,
            "s2",
            "must-not-read",
            "beta",
            900,
            800,
            90,
            1,
            9.0,
        )],
    );
    symlink(&outside, project_dir.join("linked.jsonl")).expect("fixture symlink creates");

    let evidence = single_source_evidence(&home).expect("source evidence available");
    assert_eq!(evidence.file_count, 1);
    assert_eq!(evidence.top_level_file_count, 1);
    assert_eq!(evidence.subagent_file_count, 0);
    assert_eq!(evidence.skipped_count, 1);

    let snapshot = read_workbuddy_statistics(&[home.clone()], now, TimeStandard::utc())
        .await
        .expect("safe records remain usable");
    assert_eq!(snapshot.coverage.state, CoverageState::Partial);
    assert_eq!(snapshot.total_requests, 1);
    assert_eq!(snapshot.total_tokens, 11);
    assert_eq!(
        model_window(&home, LocalUsageWindow::Today, now)
            .await
            .groups
            .len(),
        1
    );
}

/// 两个固定根的 Trace 诊断必须合并成一份快照，而不是只读国内版。
#[tokio::test]
async fn trace_diagnostics_merge_across_both_roots() {
    let temp = tempdir().expect("isolated dir exists");
    let now = epoch_ms("2026-09-04T12:00:00Z");
    let domestic = fixture_home(temp.path());
    let international = fixture_international_home(temp.path());
    write_lines(
        &domestic.join("projects/project-a/session-a.jsonl"),
        &[usage_event(now, "s1", "d1", "alpha", 10, 5, 1, 1, 0.1)],
    );
    write_lines(
        &international.join("projects/project-a/session-a.jsonl"),
        &[usage_event(now, "s1", "i1", "alpha", 10, 5, 1, 1, 0.1)],
    );
    write_trace(&domestic, "error", 1_500);
    write_trace(&international, "ok", 500);

    let snapshot = read_workbuddy_statistics(&[domestic, international], now, TimeStandard::utc())
        .await
        .expect("statistics compute succeeds");
    assert_eq!(snapshot.trace_total_count, 2);
    assert_eq!(snapshot.trace_error_count, 1);
    assert_eq!(snapshot.trace_cancelled_count, 0);
    assert_eq!(snapshot.trace_average_duration_ms, 1_000.0);
}

/// 未知 Trace 状态必须被忽略，且缺失 traces 目录本身不是用量错误。
#[test]
fn trace_diagnostics_ignore_unknown_status_and_missing_directory() {
    let temp = tempdir().expect("isolated dir exists");
    let home = temp.path();
    assert!(read_traces(home).is_empty());

    let trace_path = home.join("unknown.json");
    fs::write(
        &trace_path,
        r#"{"trace":{"startedAt":"2026-09-04T10:00:00Z","duration":1,"status":"weird"}}"#,
    )
    .expect("trace fixture writes");
    assert!(read_trace_file(&trace_path, u64::MAX).is_none());

    // 确保测试 JSON 自身有效，防止因为夹具拼写错误造成假阳性。
    let value: Value =
        serde_json::from_str(&fs::read_to_string(&trace_path).expect("trace fixture reads"))
            .expect("trace fixture is valid JSON");
    assert_eq!(value["trace"]["status"], "weird");
}

/// Trace 诊断即使面对大量无效目录项，也必须在全局枚举预算处停止。
#[test]
fn trace_directory_enumeration_obeys_the_hard_entry_budget() {
    let temp = tempdir().expect("isolated dir exists");
    for name in ["one.json", "two.json", "three.json"] {
        fs::write(temp.path().join(name), b"{}").expect("trace entry fixture writes");
    }

    let mut remaining_entries = 2;
    let entries = sorted_entries(temp.path(), &mut remaining_entries)
        .expect("bounded trace directory enumeration succeeds");

    assert_eq!(entries.len(), 2);
    assert_eq!(remaining_entries, 0);
}

/// 通知摘要使用捕获的时区；同一JSONL在UTC与上海的今日归属不同。
#[tokio::test]
async fn notification_workbuddy_snapshot_uses_captured_day_zone() {
    let temp=tempdir().unwrap();let home=fixture_home(temp.path());
    let event=epoch_ms("2026-09-03T20:00:00Z");let now=epoch_ms("2026-09-04T01:00:00Z");
    write_lines(&home.join("projects/project-a/session-a.jsonl"),&[usage_event(event,"s","m","model",100,0,10,1,1.0)]);
    let zone=jiff::tz::TimeZone::get("Asia/Shanghai").unwrap();
    let local=read_workbuddy_usage_snapshot_in_zone(&[home.clone()],now,TimeStandard::Local,&zone).await.unwrap();
    let utc=read_workbuddy_usage_snapshot_in_zone(&[home],now,TimeStandard::Local,&jiff::tz::TimeZone::UTC).await.unwrap();
    let tokens=|s:&WorkbuddyStatisticsSnapshot|s.windows.iter().find(|w|w.window==LocalUsageWindow::Today).unwrap().tokens;
    assert_eq!(tokens(&local),110);assert_eq!(tokens(&utc),0);
}
