use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use loki_metis_core::{CoverageState, LocalUsageWindow, TimeStandard, UsageDimension};
use serde_json::{Value, json};
use tempfile::tempdir;

use super::*;

/// 把 RFC 3339 测试时刻转换为 Unix 毫秒，避免依赖测试宿主时区。
fn epoch_ms(value: &str) -> i64 {
    value
        .parse::<jiff::Timestamp>()
        .expect("fixture timestamp is valid")
        .as_millisecond()
}

/// 创建 WorkBuddy 夹具根及其固定 `projects` 目录。
fn fixture_home(root: &Path) -> PathBuf {
    let home = root.join(".workbuddy");
    fs::create_dir_all(home.join("projects")).expect("projects fixture root exists");
    home
}

/// 生成一条直接携带 `providerData.usage` 的真实形状事件。
#[allow(clippy::too_many_arguments)]
fn usage_event(
    timestamp: i64,
    session_id: &str,
    message_id: &str,
    model: &str,
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    requests: i64,
    credit: f64,
) -> String {
    json!({
        "timestamp": timestamp,
        "sessionId": session_id,
        "cwd": "/Users/example/project-a",
        "providerData": {
            "messageId": message_id,
            "model": model,
            "usage": {
                "inputTokens": input_tokens,
                "inputTokensDetails": [{"cached_tokens": cached_input_tokens}],
                "outputTokens": output_tokens,
                "outputTokensDetails": [{"reasoning_tokens": 987654321}],
                "requests": requests,
                "totalTokens": input_tokens + output_tokens
            },
            "rawUsage": {"credit": credit},
            "message": {
                "usage": {
                    "inputTokens": 9000000,
                    "inputTokensDetails": [{"cached_tokens": 8000000}],
                    "outputTokens": 7000000,
                    "requests": 1,
                    "totalTokens": 16000000
                }
            }
        }
    })
    .to_string()
}

/// 生成只有嵌套 `providerData.message.usage`、没有直接 usage 的噪声事件。
fn nested_usage_only_event(timestamp: i64) -> String {
    json!({
        "timestamp": timestamp,
        "sessionId": "nested-session",
        "providerData": {
            "messageId": "nested-message",
            "model": "must-not-appear",
            "message": {
                "usage": {
                    "inputTokens": 5000000,
                    "inputTokensDetails": [{"cached_tokens": 4000000}],
                    "outputTokens": 3000000,
                    "requests": 1,
                    "totalTokens": 8000000
                }
            }
        }
    })
    .to_string()
}

/// 写入以换行结尾的 JSONL 记录。
fn write_lines(path: &Path, lines: &[String]) {
    fs::create_dir_all(path.parent().expect("fixture file has parent"))
        .expect("fixture parent exists");
    let mut file = File::create(path).expect("fixture file opens");
    for line in lines {
        writeln!(file, "{line}").expect("fixture line writes");
    }
}

/// 按生产路径读取指定窗口的模型表，失败时给出明确测试诊断。
async fn model_window(
    home: &Path,
    window: LocalUsageWindow,
    now_epoch_ms: i64,
) -> loki_metis_core::WorkbuddyModelUsageWindow {
    read_workbuddy_usage_details(
        home,
        window,
        UsageDimension::Model,
        now_epoch_ms,
        TimeStandard::utc(),
    )
    .await
    .expect("requested model window exists")
    .model_usage
}

/// 顶层与 subagent 事件必须按事件时刻、实际模型和精确 Token 分量统一统计。
#[tokio::test]
async fn reads_top_level_and_subagent_usage_with_exact_model_breakdown() {
    let temp = tempdir().expect("isolated dir exists");
    let home = fixture_home(temp.path());
    let day_one = epoch_ms("2026-09-03T12:00:00Z");
    let day_two = epoch_ms("2026-09-04T12:00:00Z");
    let top_file = home.join("projects/project-a/session-a.jsonl");
    write_lines(
        &top_file,
        &[
            usage_event(
                day_one,
                "shared-session",
                "m1",
                "alpha",
                100,
                80,
                10,
                1,
                1.25,
            ),
            nested_usage_only_event(day_two),
            usage_event(day_two, "shared-session", "m2", "beta", 50, 0, 5, 1, 0.50),
        ],
    );
    let subagent_file = home.join("projects/project-a/session-a/subagents/agent-a.jsonl");
    write_lines(
        &subagent_file,
        &[usage_event(
            day_two,
            "shared-session",
            "m3",
            "alpha",
            40,
            30,
            4,
            1,
            0.25,
        )],
    );
    fs::create_dir_all(
        subagent_file
            .parent()
            .expect("subagent parent")
            .join("metadata"),
    )
    .expect("ordinary non-source directory is allowed");

    let traces_dir = home.join(WORKBUDDY_TRACES_DIR_NAME).join("1234");
    fs::create_dir_all(&traces_dir).expect("trace fixture dir exists");
    fs::write(
        traces_dir.join("trace.json"),
        r#"{"trace":{"startedAt":"2026-09-04T10:00:00Z","duration":1500,
        "status":"error","totalTokens":999999999,"modelInfo":{"models":["ghost-model"],
        "callCount":99,"totalInputTokens":888888888,"totalCachedTokens":777777777,
        "totalOutputTokens":111111111}}}"#,
    )
    .expect("trace fixture writes");

    let evidence = inspect_workbuddy_project_source(&home).expect("source evidence available");
    assert_eq!(evidence.file_count, 2);
    assert_eq!(evidence.top_level_file_count, 1);
    assert_eq!(evidence.subagent_file_count, 1);
    assert_eq!(evidence.permission_denied_count, 0);
    assert_eq!(evidence.skipped_count, 0);
    assert!(!evidence.budget_exhausted);

    let snapshot = read_workbuddy_statistics(&home, day_two, TimeStandard::utc())
        .await
        .expect("statistics compute succeeds");
    assert_eq!(snapshot.coverage.state, CoverageState::Complete);
    assert_eq!(snapshot.total_sessions, 1);
    assert_eq!(snapshot.total_requests, 3);
    assert_eq!(snapshot.top_level_requests, 2);
    assert_eq!(snapshot.subagent_requests, 1);
    assert_eq!(snapshot.total_input_tokens, 190);
    assert_eq!(snapshot.total_cached_input_tokens, 110);
    assert_eq!(snapshot.total_uncached_input_tokens, 80);
    assert_eq!(snapshot.total_output_tokens, 19);
    assert_eq!(snapshot.total_tokens, 209);
    assert_eq!(snapshot.total_credits, Some(2.0));
    assert_eq!(snapshot.daily_buckets.len(), 2);
    assert_eq!(snapshot.daily_buckets[0].date, "2026-09-03");
    assert_eq!(snapshot.daily_buckets[0].request_count, 1);
    assert_eq!(snapshot.daily_buckets[1].date, "2026-09-04");
    assert_eq!(snapshot.daily_buckets[1].request_count, 2);

    // Trace 只贡献状态和耗时；其中伪造的 Token 与模型绝不能进入用量。
    assert_eq!(snapshot.trace_total_count, 1);
    assert_eq!(snapshot.trace_error_count, 1);
    assert_eq!(snapshot.trace_cancelled_count, 0);
    assert_eq!(snapshot.trace_average_duration_ms, 1_500.0);
    let month = model_window(&home, LocalUsageWindow::ThisMonth, day_two).await;
    assert_eq!(month.groups.len(), 2);
    let alpha = month
        .groups
        .iter()
        .find(|group| group.model.as_deref() == Some("alpha"))
        .expect("alpha model row exists");
    assert_eq!(alpha.call_count, 2);
    assert_eq!(alpha.input_tokens, 140);
    assert_eq!(alpha.cached_input_tokens, 110);
    assert_eq!(alpha.uncached_input_tokens, 30);
    assert_eq!(alpha.output_tokens, 14);
    assert_eq!(alpha.total_tokens, 154);
    assert_eq!(alpha.credits, Some(1.5));
    assert_eq!(alpha.top_level_call_count, 1);
    assert_eq!(alpha.subagent_call_count, 1);
    assert!(
        month
            .groups
            .iter()
            .all(|group| group.model.as_deref() != Some("ghost-model"))
    );
}

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
        &home,
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

    let snapshot = read_workbuddy_statistics(&home, now, TimeStandard::utc())
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

    let snapshot = read_workbuddy_statistics(&home, now, TimeStandard::utc())
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

    let snapshot = read_workbuddy_statistics(&home, now, TimeStandard::utc())
        .await
        .expect("large file statistics compute succeeds");
    assert_eq!(snapshot.coverage.state, CoverageState::Complete);
    assert_eq!(snapshot.total_requests, 1);
    assert_eq!(snapshot.total_tokens, 30);
}

/// 获批 `projects` 根缺失时必须失败关闭为来源不可用。
#[tokio::test]
async fn missing_projects_root_is_source_unavailable() {
    let temp = tempdir().expect("isolated dir exists");
    let home = temp.path().join(".workbuddy");
    fs::create_dir_all(&home).expect("workbuddy fixture home exists");

    let result =
        read_workbuddy_statistics(&home, epoch_ms("2026-09-04T12:00:00Z"), TimeStandard::utc())
            .await;
    assert_eq!(result.unwrap_err(), WorkbuddyReadError::SourceUnavailable);
    assert_eq!(
        inspect_workbuddy_project_source(&home).unwrap_err(),
        WorkbuddyReadError::SourceUnavailable
    );
}

/// 已存在但链接型的 `projects` 根是安全读取失败，不能伪装成未安装来源。
#[cfg(unix)]
#[tokio::test]
async fn linked_projects_root_is_an_explicit_read_failure() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().expect("isolated dir exists");
    let home = temp.path().join(".workbuddy");
    let outside = temp.path().join("outside-projects");
    fs::create_dir_all(&home).expect("workbuddy fixture home exists");
    fs::create_dir_all(&outside).expect("outside fixture exists");
    symlink(&outside, home.join("projects")).expect("projects symlink creates");

    let result =
        read_workbuddy_statistics(&home, epoch_ms("2026-09-04T12:00:00Z"), TimeStandard::utc())
            .await;
    assert_eq!(result.unwrap_err(), WorkbuddyReadError::Read);
    assert_eq!(
        inspect_workbuddy_project_source(&home).unwrap_err(),
        WorkbuddyReadError::Read
    );
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

    let evidence = inspect_workbuddy_project_source(&home).expect("source evidence available");
    assert_eq!(evidence.file_count, 1);
    assert_eq!(evidence.top_level_file_count, 1);
    assert_eq!(evidence.subagent_file_count, 0);
    assert_eq!(evidence.skipped_count, 1);

    let snapshot = read_workbuddy_statistics(&home, now, TimeStandard::utc())
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
