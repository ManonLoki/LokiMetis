//! 用量解释的成功路径与最高风险误读：月度周期不得变成周窗口，Claude 不在目录中。

use crate::AiTool;

use super::{
    QuotaFailure, QuotaProvider, QuotaWindowKind, classify_window_seconds, parse_provider_quota,
    supported_quota_providers,
};

#[test]
fn providers_follow_requested_order_and_omit_unavailable_tools() {
    let providers = supported_quota_providers();
    assert_eq!(providers.map(QuotaProvider::slug), ["codex", "cursor", "grok"]);
    assert!(providers.into_iter().all(|provider| !matches!(provider.ai_tool(), AiTool::ClaudeCode | AiTool::WorkBuddy)));
}

/// 缺失、仅周窗口与错放的窗口均不能获得展示资格，零余额仍是有效短窗。
#[test]
fn quota_display_requires_a_real_five_hour_window() {
    let mut quota = parse_provider_quota(QuotaProvider::Codex, "{}").unwrap();
    assert!(!quota.has_five_hour_window());
    let weekly = super::QuotaWindow { kind: QuotaWindowKind::Weekly, used_tenths: 1000, reset_at_unix: None };
    quota.weekly = Some(weekly);
    assert!(!quota.has_five_hour_window());
    quota.five_hour = Some(weekly);
    assert!(!quota.has_five_hour_window());
    quota.five_hour = Some(super::QuotaWindow { kind: QuotaWindowKind::FiveHour, ..weekly });
    assert!(quota.has_five_hour_window());
}

#[test]
fn window_seconds_only_match_five_hours_or_seven_days() {
    assert_eq!(classify_window_seconds(14_399), None);
    assert_eq!(classify_window_seconds(14_400), Some(QuotaWindowKind::FiveHour));
    assert_eq!(classify_window_seconds(18_000), Some(QuotaWindowKind::FiveHour));
    assert_eq!(classify_window_seconds(21_600), Some(QuotaWindowKind::FiveHour));
    assert_eq!(classify_window_seconds(21_601), None);
    assert_eq!(classify_window_seconds(518_399), None);
    assert_eq!(classify_window_seconds(518_400), Some(QuotaWindowKind::Weekly));
    assert_eq!(classify_window_seconds(604_800), Some(QuotaWindowKind::Weekly));
    assert_eq!(classify_window_seconds(691_200), Some(QuotaWindowKind::Weekly));
    assert_eq!(classify_window_seconds(691_201), None);
    assert_eq!(classify_window_seconds(2_592_000), None);
}

#[test]
fn codex_keeps_explicit_five_hour_and_weekly_windows() {
    let body = r#"{
        "plan_type": "plus",
        "rate_limit": {
            "primary_window": {"used_percent": 27, "limit_window_seconds": 18000, "reset_at": 1700000000},
            "secondary_window": {"used_percent": 4, "limit_window_seconds": 604800, "reset_at": 1700604800}
        }
    }"#;
    let quota = parse_provider_quota(QuotaProvider::Codex, body).unwrap();
    assert_eq!(quota.plan.as_deref(), Some("plus"));
    let five_hour = quota.five_hour.unwrap();
    assert_eq!(five_hour.used_tenths, 270);
    assert_eq!(five_hour.remaining_tenths(), 730);
    assert_eq!(five_hour.reset_at_unix, Some(1_700_000_000));
    assert_eq!(quota.weekly.unwrap().used_tenths, 40);
}

#[test]
fn codex_uses_primary_and_secondary_only_when_seconds_are_absent() {
    let body = r#"{"rate_limit":{"primary_window":{"used_percent":10},"secondary_window":{"used_percent":20}}}"#;
    let quota = parse_provider_quota(QuotaProvider::Codex, body).unwrap();
    assert_eq!(quota.five_hour.unwrap().kind, QuotaWindowKind::FiveHour);
    assert_eq!(quota.weekly.unwrap().used_tenths, 200);
}

#[test]
fn codex_monthly_seconds_do_not_fall_back_to_the_weekly_convention() {
    let body = r#"{"rate_limit":{"primary_window":{"used_percent":10,"limit_window_seconds":2592000},"secondary_window":{"used_percent":20,"limit_window_seconds":2592000}}}"#;
    let quota = parse_provider_quota(QuotaProvider::Codex, body).unwrap();
    assert!(quota.five_hour.is_none());
    assert!(quota.weekly.is_none());
}

#[test]
fn malformed_json_and_out_of_range_percent_are_rejected() {
    assert_eq!(parse_provider_quota(QuotaProvider::Codex, "{"), Err(QuotaFailure::Malformed));
    assert_eq!(parse_provider_quota(QuotaProvider::Codex, "[]"), Err(QuotaFailure::Malformed));
    let body = r#"{"rate_limit":{"primary_window":{"used_percent":150,"limit_window_seconds":18000}}}"#;
    let quota = parse_provider_quota(QuotaProvider::Codex, body).unwrap();
    assert!(quota.five_hour.is_none());
}

#[test]
fn fractional_percent_scales_to_tenths_while_exact_zero_and_one_stay_percents() {
    let body = r#"{"rate_limit":{"five_hour":{"used_percent":0.27,"limit_window_seconds":18000},"weekly":{"used_percent":1,"limit_window_seconds":604800}}}"#;
    let quota = parse_provider_quota(QuotaProvider::Grok, body).unwrap();
    assert_eq!(quota.five_hour.unwrap().used_tenths, 270);
    assert_eq!(quota.weekly.unwrap().used_tenths, 10);
}

#[test]
fn empty_object_is_a_connected_response_without_target_windows() {
    let quota = parse_provider_quota(QuotaProvider::Cursor, "{}").unwrap();
    assert!(quota.five_hour.is_none() && quota.weekly.is_none());
}

#[test]
fn plan_label_drops_email_and_non_ascii() {
    let body = r#"{"plan_type":"a@b.com","rate_limit":{"primary_window":{"used_percent":1,"limit_window_seconds":18000}}}"#;
    let quota = parse_provider_quota(QuotaProvider::Codex, body).unwrap();
    assert!(quota.plan.is_none());
    assert!(quota.five_hour.is_some());
}

#[test]
fn cursor_monthly_billing_cycle_is_not_weekly() {
    let body = r#"{
        "billingCycleStart":"2026-04-02T14:11:55.000Z",
        "billingCycleEnd":"2026-05-02T14:11:55.000Z",
        "membershipType":"enterprise",
        "individualUsage":{"plan":{"used":2000,"limit":2000,"totalPercentUsed":100,"autoPercentUsed":0,"apiPercentUsed":100}}
    }"#;
    let quota = parse_provider_quota(QuotaProvider::Cursor, body).unwrap();
    assert_eq!(quota.plan.as_deref(), Some("enterprise"));
    assert!(quota.five_hour.is_none());
    assert!(quota.weekly.is_none());
}

#[test]
fn cursor_seven_day_cycle_uses_used_over_limit_when_percent_is_absent() {
    let body = r#"{
        "billingCycleStart":"2026-04-02T00:00:00Z",
        "billingCycleEnd":"2026-04-09T00:00:00Z",
        "individualUsage":{"plan":{"used":25,"limit":100}}
    }"#;
    let quota = parse_provider_quota(QuotaProvider::Cursor, body).unwrap();
    let weekly = quota.weekly.unwrap();
    assert_eq!(weekly.used_tenths, 250);
    assert!(quota.five_hour.is_none());
}

#[test]
fn grok_monthly_credits_stay_outside_both_windows() {
    let body = r#"{"plan":"pro","config":{"creditUsagePercent":40,"billingPeriodStart":"2026-04-01T00:00:00Z","billingPeriodEnd":"2026-05-01T00:00:00Z"}}"#;
    let quota = parse_provider_quota(QuotaProvider::Grok, body).unwrap();
    assert_eq!(quota.plan.as_deref(), Some("pro"));
    assert!(quota.five_hour.is_none() && quota.weekly.is_none());
}

#[test]
fn grok_five_hour_period_uses_nested_credit_values() {
    let body = r#"{"config":{"used":{"val":12.5},"monthlyLimit":{"val":100},"currentPeriod":{"start":"2026-04-02T00:00:00Z","end":"2026-04-02T05:00:00Z"}}}"#;
    let quota = parse_provider_quota(QuotaProvider::Grok, body).unwrap();
    assert_eq!(quota.five_hour.unwrap().used_tenths, 125);
    assert!(quota.weekly.is_none());
}

#[test]
fn workbuddy_end_time_alone_is_not_a_weekly_window() {
    let body = r#"{"data":{"Response":{"Data":{"Accounts":[{"CycleCapacitySize":100,"CycleCapacityUsed":25,"CycleEndTime":"2026-04-09 00:00:00"}]}}}}"#;
    let quota = parse_provider_quota(QuotaProvider::WorkBuddy, body).unwrap();
    assert!(quota.five_hour.is_none() && quota.weekly.is_none());
}

#[test]
fn workbuddy_matching_cycle_uses_capacity_and_enterprise_without_duration_stays_empty() {
    let weekly = r#"{"data":{"Response":{"Data":{"Accounts":[{"CycleCapacitySize":100,"CycleCapacityRemain":75,"CycleStartTime":"2026-04-02 00:00:00","CycleEndTime":"2026-04-09 00:00:00"}]}}}}"#;
    let quota = parse_provider_quota(QuotaProvider::WorkBuddy, weekly).unwrap();
    assert_eq!(quota.weekly.unwrap().used_tenths, 250);
    let enterprise = r#"{"data":{"limitNum":100,"credit":10,"cycleResetTime":"2026-05-01 00:00:00"}}"#;
    let quota = parse_provider_quota(QuotaProvider::WorkBuddy, enterprise).unwrap();
    assert!(quota.five_hour.is_none() && quota.weekly.is_none());
    let unlimited = r#"{"data":{"limitNum":-1}}"#;
    let quota = parse_provider_quota(QuotaProvider::WorkBuddy, unlimited).unwrap();
    assert!(quota.five_hour.is_none() && quota.weekly.is_none());
}

#[test]
fn codex_session_snapshot_uses_window_minutes_and_ignores_primary_without_duration() {
    let body = r#"{"type":"event_msg","payload":{"type":"token_count","rate_limits":{"plan_type":"plus","primary":{"used_percent":27,"window_minutes":300,"resets_at":1700000000},"secondary":{"used_percent":4,"window_minutes":10080,"resets_at":1700604800}}}}"#;
    let quota = parse_provider_quota(QuotaProvider::Codex, body).unwrap();
    assert_eq!(quota.plan.as_deref(), Some("plus"));
    assert_eq!(quota.five_hour.unwrap().used_tenths, 270);
    assert_eq!(quota.five_hour.unwrap().reset_at_unix, Some(1_700_000_000));
    assert_eq!(quota.weekly.unwrap().used_tenths, 40);
    let bare = r#"{"rate_limits":{"primary":{"used_percent":27},"secondary":{"used_percent":4}}}"#;
    let quota = parse_provider_quota(QuotaProvider::Codex, bare).unwrap();
    assert!(quota.five_hour.is_none() && quota.weekly.is_none());
    let month = r#"{"rate_limits":{"primary":{"used_percent":50,"window_minutes":43200}}}"#;
    let quota = parse_provider_quota(QuotaProvider::Codex, month).unwrap();
    assert!(quota.five_hour.is_none() && quota.weekly.is_none());
}

#[test]
fn grok_log_line_reads_weekly_period_from_context_config() {
    let body = r#"{"msg":"billing: fetched credits config","ctx":{"config":{"creditUsagePercent":12.5,"currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY","start":"2026-04-02T00:00:00Z","end":"2026-04-09T00:00:00Z"}}}}"#;
    let quota = parse_provider_quota(QuotaProvider::Grok, body).unwrap();
    assert_eq!(quota.weekly.unwrap().used_tenths, 125);
    assert!(quota.five_hour.is_none());
}
