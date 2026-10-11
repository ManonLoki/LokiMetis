//! 本文件的测试覆盖：把 RFC 3339 测试时刻转换为 Unix 毫秒，避免依赖测试宿主时区；在夹具根下创建指定 WorkBuddy 版本的固定目录及其 `projects` 子目录；创建 WorkBuddy 国内版夹具根及其固定 `projects` 目录。

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use loki_metris_core::{
    CoverageState, LocalUsageWindow, TimeStandard, UsageDimension, WORKBUDDY_HOME_DIR_NAME,
    WORKBUDDY_INTERNATIONAL_HOME_DIR_NAME,
};
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

/// 在夹具根下创建指定 WorkBuddy 版本的固定目录及其 `projects` 子目录。
fn fixture_edition_home(root: &Path, dir_name: &str) -> PathBuf {
    let home = root.join(dir_name);
    fs::create_dir_all(home.join("projects")).expect("projects fixture root exists");
    home
}

/// 创建 WorkBuddy 国内版夹具根及其固定 `projects` 目录。
fn fixture_home(root: &Path) -> PathBuf {
    fixture_edition_home(root, WORKBUDDY_HOME_DIR_NAME)
}

/// 创建 WorkBuddy 国际版夹具根及其固定 `projects` 目录。
fn fixture_international_home(root: &Path) -> PathBuf {
    fixture_edition_home(root, WORKBUDDY_INTERNATIONAL_HOME_DIR_NAME)
}

/// 把夹具路径包装成只读扫描源；测试只关心探测证据，身份字段只求稳定。
fn fixture_source(home: &Path) -> WorkbuddyScanSource {
    WorkbuddyScanSource {
        root_id: "workbuddy-root-fixture".to_owned(),
        alias: "fixture".to_owned(),
        path: home.to_path_buf(),
    }
}

/// 读取单个夹具根的项目结构证据，保留读取失败以便断言失败关闭语义。
fn single_source_evidence(
    home: &Path,
) -> Result<WorkbuddyProjectSourceEvidence, WorkbuddyReadError> {
    inspect_workbuddy_project_sources(&[fixture_source(home)])
        .into_iter()
        .next()
        .expect("single fixture root yields one inspection")
        .evidence
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

/// 写入一个固定根下形状最小的 Trace 诊断文件。
fn write_trace(home: &Path, status: &str, duration: i64) {
    let traces_dir = home.join(WORKBUDDY_TRACES_DIR_NAME).join("1234");
    fs::create_dir_all(&traces_dir).expect("trace fixture dir exists");
    fs::write(
        traces_dir.join("trace.json"),
        format!(
            r#"{{"trace":{{"startedAt":"2026-09-04T10:00:00Z","duration":{duration},"status":"{status}"}}}}"#
        ),
    )
    .expect("trace fixture writes");
}

/// 按生产路径读取单个夹具根在指定窗口的模型表，失败时给出明确测试诊断。
async fn model_window(
    home: &Path,
    window: LocalUsageWindow,
    now_epoch_ms: i64,
) -> loki_metris_core::WorkbuddyModelUsageWindow {
    read_workbuddy_usage_details(
        &[home.to_path_buf()],
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

    let evidence = single_source_evidence(&home).expect("source evidence available");
    assert_eq!(evidence.file_count, 2);
    assert_eq!(evidence.top_level_file_count, 1);
    assert_eq!(evidence.subagent_file_count, 1);
    assert_eq!(evidence.permission_denied_count, 0);
    assert_eq!(evidence.skipped_count, 0);
    assert!(!evidence.budget_exhausted);

    let snapshot = read_workbuddy_statistics(&[home.clone()], day_two, TimeStandard::utc())
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

/// 同一 `messageId` 的后续直接 usage 会修正初值，最终 Token 只计一次。
#[tokio::test]
async fn later_provider_usage_update_keeps_final_workbuddy_ai_totals() {
    let temp = tempdir().expect("isolated dir exists");
    let home = fixture_international_home(temp.path());
    let first = epoch_ms("2026-09-04T12:00:00Z");
    write_lines(
        &home.join("projects/project-a/session-a.jsonl"),
        &[
            usage_event(
                first,
                "session-a",
                "message-a",
                "model-a",
                88_551,
                88_320,
                5_061,
                1,
                0.0,
            ),
            usage_event(
                first + 9_625,
                "session-a",
                "message-a",
                "model-a",
                93_805,
                93_568,
                2_525,
                1,
                0.0,
            ),
        ],
    );

    let snapshot = read_workbuddy_statistics(&[home], first + 10_000, TimeStandard::utc())
        .await
        .expect("statistics compute succeeds");
    assert_eq!(snapshot.coverage.state, CoverageState::Complete);
    assert_eq!(snapshot.total_requests, 1);
    assert_eq!(snapshot.total_input_tokens, 93_805);
    assert_eq!(snapshot.total_cached_input_tokens, 93_568);
    assert_eq!(snapshot.total_output_tokens, 2_525);
    assert_eq!(snapshot.total_tokens, 96_330);
}

/// 国内版与国际版两个固定根必须合并成同一份快照，并保留逐根会话身份。
#[tokio::test]
async fn merges_domestic_and_international_roots_into_one_snapshot() {
    let temp = tempdir().expect("isolated dir exists");
    let domestic = fixture_home(temp.path());
    let international = fixture_international_home(temp.path());
    let now = epoch_ms("2026-09-04T12:00:00Z");
    write_lines(
        &domestic.join("projects/project-a/session-a.jsonl"),
        &[usage_event(
            now,
            "shared-session",
            "domestic-call",
            "alpha",
            100,
            80,
            10,
            1,
            1.25,
        )],
    );
    write_lines(
        &international.join("projects/project-a/session-a.jsonl"),
        &[usage_event(
            now,
            "shared-session",
            "international-call",
            "alpha",
            40,
            30,
            4,
            1,
            0.25,
        )],
    );
    let homes = vec![domestic, international];

    let snapshot = read_workbuddy_statistics(&homes, now, TimeStandard::utc())
        .await
        .expect("merged editions compute succeeds");
    assert_eq!(snapshot.coverage.state, CoverageState::Complete);
    assert_eq!(snapshot.coverage.roots_scanned, 2);
    assert_eq!(snapshot.total_requests, 2);
    assert_eq!(snapshot.total_input_tokens, 140);
    assert_eq!(snapshot.total_cached_input_tokens, 110);
    assert_eq!(snapshot.total_uncached_input_tokens, 30);
    assert_eq!(snapshot.total_output_tokens, 14);
    assert_eq!(snapshot.total_tokens, 154);
    assert_eq!(snapshot.total_credits, Some(1.5));
    assert_eq!(snapshot.daily_buckets.len(), 1);
    assert_eq!(snapshot.daily_buckets[0].request_count, 2);
    // 两个版本是互不相同的安装，同名 sessionId 属于各自根下的不同会话。
    assert_eq!(snapshot.total_sessions, 2);

    let details = read_workbuddy_usage_details(
        &homes,
        LocalUsageWindow::ThisMonth,
        UsageDimension::Model,
        now,
        TimeStandard::utc(),
    )
    .await
    .expect("merged model window builds");
    assert_eq!(details.model_usage.groups.len(), 1);
    let alpha = &details.model_usage.groups[0];
    assert_eq!(alpha.model.as_deref(), Some("alpha"));
    assert_eq!(alpha.call_count, 2);
    assert_eq!(alpha.total_tokens, 154);
    assert_eq!(details.statistics.fact.value.call_count, 2);
    assert_eq!(details.statistics.fact.value.tokens.total_tokens, 154);
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - tests_usage_reconcile.rs：统计页和模型表必须由同一批 project JSONL 事件构造并相互对账。；活跃文件末尾没有换行的半条记录必须留待下次读取，不产生错误或虚假用量。；损坏行与超长完整行要降级覆盖，但不得阻断其后的正常记录。
include!("tests_usage_reconcile.rs");
