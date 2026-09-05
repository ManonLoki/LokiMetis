use super::*;
use crate::workbuddy_stats::{WorkbuddyUsageEventRecord, WorkbuddyUsageOrigin};

/// 固定观测时刻 2026-07-27T12:00:00Z。
const OBSERVED_EPOCH_MS: i64 = 1_785_153_600_000;
/// 观测日内的请求时刻。
const TODAY_EPOCH_MS: i64 = 1_785_148_280_179;

/// 构造一条实际模型明确的 usage 事件。
#[allow(clippy::too_many_arguments)]
fn usage(
    id: &str,
    timestamp: i64,
    model: Option<&str>,
    input: i64,
    cached: i64,
    output: i64,
    credit: Option<f64>,
    origin: WorkbuddyUsageOrigin,
) -> WorkbuddyUsageEventRecord {
    WorkbuddyUsageEventRecord {
        logical_call_id: id.to_owned(),
        session_key: "same-session".to_owned(),
        source_id: format!("source-{id}"),
        occurred_at_epoch_ms: timestamp,
        model: model.map(ToOwned::to_owned),
        project_key: None,
        project_label: None,
        request_count: 1,
        input_tokens: input,
        cached_input_tokens: cached,
        output_tokens: output,
        total_tokens: input + output,
        credit,
        origin,
    }
}

/// 按固定观测时刻聚合指定模型窗口。
fn window(
    records: &[WorkbuddyUsageEventRecord],
    target: LocalUsageWindow,
) -> WorkbuddyModelUsageWindow {
    let (prepared, _) = crate::workbuddy_stats::record::prepare_workbuddy_usage_records(records);
    let today = crate::timeline::civil_date_for_timestamp(
        OBSERVED_EPOCH_MS,
        &TimeStandard::utc(),
        &TimeZone::UTC,
    )
    .expect("observed date");
    super::build_model_usage_window(
        &prepared,
        target,
        today,
        &TimeStandard::utc(),
        &TimeZone::UTC,
    )
    .expect("model window exists")
}

/// 同一会话切换模型时必须按每个事件的实际模型分开，并精确展示所有 Token 分项。
#[test]
fn groups_each_event_by_actual_model_and_origin() {
    let records = vec![
        usage(
            "a",
            TODAY_EPOCH_MS,
            Some("minimax-m3"),
            100,
            80,
            20,
            Some(1.5),
            WorkbuddyUsageOrigin::TopLevel,
        ),
        usage(
            "b",
            TODAY_EPOCH_MS + 1,
            Some("minimax-m3"),
            50,
            10,
            5,
            Some(0.5),
            WorkbuddyUsageOrigin::Subagent,
        ),
        usage(
            "c",
            TODAY_EPOCH_MS + 2,
            Some("deepseek-v4-flash"),
            30,
            20,
            10,
            Some(0.25),
            WorkbuddyUsageOrigin::TopLevel,
        ),
    ];
    let today = window(&records, LocalUsageWindow::Today);
    assert_eq!(today.groups.len(), 2);
    let minimax = &today.groups[0];
    assert_eq!(minimax.model.as_deref(), Some("minimax-m3"));
    assert_eq!(minimax.call_count, 2);
    assert_eq!(minimax.input_tokens, 150);
    assert_eq!(minimax.cached_input_tokens, 90);
    assert_eq!(minimax.uncached_input_tokens, 60);
    assert_eq!(minimax.output_tokens, 25);
    assert_eq!(minimax.total_tokens, 175);
    assert_eq!(minimax.credits, Some(2.0));
    assert_eq!(minimax.top_level_call_count, 1);
    assert_eq!(minimax.subagent_call_count, 1);
}

/// 任一请求缺失积分时对应模型积分保持不可用，但 Token 和请求仍完整累计。
#[test]
fn missing_credit_does_not_discard_model_tokens() {
    let records = vec![
        usage(
            "a",
            TODAY_EPOCH_MS,
            Some("model-a"),
            10,
            5,
            2,
            Some(0.1),
            WorkbuddyUsageOrigin::TopLevel,
        ),
        usage(
            "b",
            TODAY_EPOCH_MS + 1,
            Some("model-a"),
            20,
            10,
            3,
            None,
            WorkbuddyUsageOrigin::TopLevel,
        ),
    ];
    let group = &window(&records, LocalUsageWindow::Today).groups[0];
    assert_eq!(group.call_count, 2);
    assert_eq!(group.total_tokens, 35);
    assert_eq!(group.credits, None);
}

/// 合法单值相加若溢出为无穷，积分合计必须退回未提供而不是暴露非法浮点数。
#[test]
fn credit_sum_overflow_stays_unavailable() {
    assert_eq!(sum_optional_credit(Some(f64::MAX), Some(f64::MAX)), None);
}

/// 窗口成员必须按事件时间决定，并保持总量降序、同量模型名字典序。
#[test]
fn calendar_membership_and_stable_sort_follow_events() {
    let yesterday = TODAY_EPOCH_MS - 24 * 60 * 60 * 1_000;
    let records = vec![
        usage(
            "z",
            TODAY_EPOCH_MS,
            Some("zeta"),
            8,
            2,
            2,
            Some(0.1),
            WorkbuddyUsageOrigin::TopLevel,
        ),
        usage(
            "a",
            TODAY_EPOCH_MS + 1,
            Some("alpha"),
            8,
            2,
            2,
            Some(0.1),
            WorkbuddyUsageOrigin::TopLevel,
        ),
        usage(
            "old",
            yesterday,
            Some("yesterday"),
            20,
            5,
            5,
            Some(0.2),
            WorkbuddyUsageOrigin::TopLevel,
        ),
    ];
    let today = window(&records, LocalUsageWindow::Today);
    let today_models: Vec<_> = today
        .groups
        .iter()
        .map(|group| group.model.as_deref())
        .collect();
    assert_eq!(today_models, vec![Some("alpha"), Some("zeta")]);
    assert_eq!(
        window(&records, LocalUsageWindow::Yesterday).groups[0]
            .model
            .as_deref(),
        Some("yesterday")
    );
}

/// 缺失或不安全模型名进入统一未归属行，不能泄露路径形态标签。
#[test]
fn missing_or_unsafe_model_is_grouped_as_unassigned() {
    let records = vec![
        usage(
            "missing",
            TODAY_EPOCH_MS,
            None,
            10,
            0,
            1,
            Some(0.1),
            WorkbuddyUsageOrigin::TopLevel,
        ),
        usage(
            "unsafe",
            TODAY_EPOCH_MS + 1,
            Some("/private/model"),
            20,
            0,
            2,
            Some(0.2),
            WorkbuddyUsageOrigin::TopLevel,
        ),
    ];
    let today = window(&records, LocalUsageWindow::Today);
    assert_eq!(today.groups.len(), 1);
    assert_eq!(today.groups[0].model, None);
    assert_eq!(today.groups[0].call_count, 2);
    assert_eq!(today.groups[0].total_tokens, 33);
}
