//! 用量解释的成功路径与最高风险误读：月度周期不得变成周窗口，只有 Cursor 保留月窗口。

use crate::AiTool;

use super::{
    QuotaFailure, QuotaProvider, QuotaWindowKind, classify_window_seconds, parse_provider_quota,
    supported_quota_providers,
};

/// 终端投影独立保留周额度、按活动目录排序，0余额不会被过滤。
#[test]
fn terminal_quota_projection_orders_windows_and_omits_unavailable_sources() {
    let codex = parse_provider_quota(QuotaProvider::Codex, r#"{"rate_limits":{"primary":{"used_percent":100,"window_minutes":10080}}}"#).unwrap();
    let grok = parse_provider_quota(QuotaProvider::Grok, r#"{"rate_limit":{"five_hour":{"used_percent":8,"limit_window_seconds":18000}}}"#).unwrap();
    let unsupported = parse_provider_quota(QuotaProvider::WorkBuddy, r#"{"rate_limit":{"five_hour":{"used_percent":8,"limit_window_seconds":18000}}}"#).unwrap();
    assert!(super::ProviderQuotaSnapshot::project(None, &[]).is_none());
    let snapshot = super::ProviderQuotaSnapshot::project(Some(123), &[grok, unsupported, codex]).unwrap();
    snapshot.validate().unwrap();
    assert_eq!(snapshot.providers.iter().map(|q| q.provider.slug()).collect::<Vec<_>>(), ["codex", "grok"]);
    assert!(snapshot.providers[0].five_hour.is_none());
    assert_eq!(snapshot.providers[0].weekly.unwrap().remaining_tenths(), 0);
    let value = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(value["providers"][0]["weekly"]["usedTenths"], 1000);
    assert!(value["providers"][0].get("fiveHour").is_none());
    let cleared = super::ProviderQuotaSnapshot::project(Some(124), &[]).unwrap();
    assert!(cleared.providers.is_empty());
}

/// 跨端额度拒绝越界、错槽、重复提供商与可能包含身份信息的方案名。
#[test]
fn terminal_quota_validation_rejects_invalid_values_and_duplicates() {
    let value = serde_json::json!({"observedAtUnix":123,"providers":[{"provider":"codex","weekly":{"kind":"weekly","usedTenths":970}}]});
    let quota: super::ProviderQuotaSnapshot = serde_json::from_value(value.clone()).unwrap();
    quota.validate().unwrap();
    for (field, invalid) in [("usedTenths", serde_json::json!(1001)), ("kind", serde_json::json!("fiveHour")), ("resetAtUnix", serde_json::json!(-1))] {
        let mut changed = value.clone(); changed["providers"][0]["weekly"][field] = invalid;
        assert!(serde_json::from_value::<super::ProviderQuotaSnapshot>(changed).unwrap().validate().is_err());
    }
    let mut duplicate = quota.clone(); duplicate.providers.push(quota.providers[0].clone());
    assert!(duplicate.validate().is_err());
    let mut private_plan = quota; private_plan.providers[0].plan = Some("user@example.com".into());
    assert!(private_plan.validate().is_err());
}

/// 额度活动目录保持确认顺序，并排除不可用来源。
#[test]
fn providers_follow_requested_order_and_omit_unavailable_tools() {
    let providers = supported_quota_providers();
    assert_eq!(providers.map(QuotaProvider::slug), ["codex", "claude", "cursor", "grok"]);
    assert_eq!(QuotaProvider::Claude.ai_tool(), AiTool::ClaudeCode);
    assert!(providers.into_iter().all(|provider| provider.ai_tool() != AiTool::WorkBuddy));
}

/// 周额度可以单独展示，错放类型不算有效窗口，零余额仍是有效短窗。
#[test]
fn quota_display_keeps_either_real_window_including_weekly_only() {
    let mut quota = parse_provider_quota(QuotaProvider::Codex, "{}").unwrap();
    assert!(!quota.has_usage_window());
    let weekly = super::QuotaWindow { kind: QuotaWindowKind::Weekly, used_tenths: 1000, reset_at_unix: None };
    quota.weekly = Some(weekly);
    assert!(quota.has_usage_window());
    quota.weekly = None;
    quota.five_hour = Some(weekly);
    assert!(!quota.has_usage_window());
    quota.five_hour = Some(super::QuotaWindow { kind: QuotaWindowKind::FiveHour, ..weekly });
    assert!(quota.has_usage_window());
}

/// 窗口分类严格覆盖五小时、七日与账单月的边界。
#[test]
fn window_seconds_match_five_hours_seven_days_or_a_billing_month() {
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
    assert_eq!(classify_window_seconds(2_332_799), None);
    assert_eq!(classify_window_seconds(2_332_800), Some(QuotaWindowKind::Monthly));
    assert_eq!(classify_window_seconds(2_592_000), Some(QuotaWindowKind::Monthly));
    assert_eq!(classify_window_seconds(2_764_800), Some(QuotaWindowKind::Monthly));
    assert_eq!(classify_window_seconds(2_764_801), None);
}

/// Codex 保留显式短窗、周窗、套餐与重置时间。
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

/// Codex 仅在缺时长时使用主次窗口约定。
#[test]
fn codex_uses_primary_and_secondary_only_when_seconds_are_absent() {
    let body = r#"{"rate_limit":{"primary_window":{"used_percent":10},"secondary_window":{"used_percent":20}}}"#;
    let quota = parse_provider_quota(QuotaProvider::Codex, body).unwrap();
    assert_eq!(quota.five_hour.unwrap().kind, QuotaWindowKind::FiveHour);
    assert_eq!(quota.weekly.unwrap().used_tenths, 200);
}

/// 显式月周期不能被主次窗口约定误标为五小时或周。
#[test]
fn codex_monthly_seconds_do_not_fall_back_to_the_weekly_convention() {
    let body = r#"{"rate_limit":{"primary_window":{"used_percent":10,"limit_window_seconds":2592000},"secondary_window":{"used_percent":20,"limit_window_seconds":2592000}}}"#;
    let quota = parse_provider_quota(QuotaProvider::Codex, body).unwrap();
    assert!(quota.five_hour.is_none());
    assert!(quota.weekly.is_none());
}

/// 坏正文和越界百分数不能形成可用额度窗口。
#[test]
fn malformed_json_and_out_of_range_percent_are_rejected() {
    assert_eq!(parse_provider_quota(QuotaProvider::Codex, "{"), Err(QuotaFailure::Malformed));
    assert_eq!(parse_provider_quota(QuotaProvider::Codex, "[]"), Err(QuotaFailure::Malformed));
    let body = r#"{"rate_limit":{"primary_window":{"used_percent":150,"limit_window_seconds":18000}}}"#;
    let quota = parse_provider_quota(QuotaProvider::Codex, body).unwrap();
    assert!(quota.five_hour.is_none());
}

/// 小数百分数保持自身单位，避免低用量被放大百倍。
#[test]
fn fractional_percent_stays_a_percentage_instead_of_becoming_a_ratio() {
    let body = r#"{"rate_limit":{"five_hour":{"used_percent":0.27,"limit_window_seconds":18000},"weekly":{"used_percent":1,"limit_window_seconds":604800}}}"#;
    let quota = parse_provider_quota(QuotaProvider::Grok, body).unwrap();
    assert_eq!(quota.five_hour.unwrap().used_tenths, 3);
    assert_eq!(quota.weekly.unwrap().used_tenths, 10);
}

/// 百分数字段在低用量边界保持 0..100 单位，桌面与终端获得相同剩余额度。
#[test]
fn sub_one_percent_values_keep_units_for_all_percentage_fields() {
    for (percent, expected) in [(0.0, 0), (0.1, 1), (0.5, 5), (0.99, 10), (1.0, 10)] {
        for provider in [QuotaProvider::Codex, QuotaProvider::Cursor, QuotaProvider::Grok, QuotaProvider::WorkBuddy] {
            let body = serde_json::json!({"rate_limits":{"primary":{"used_percent":percent,"window_minutes":300}}});
            let quota = parse_provider_quota(provider, &body.to_string()).unwrap();
            assert_eq!(quota.five_hour.unwrap().used_tenths, expected, "{provider:?}: {percent}");
        }
        let cursor = serde_json::json!({"billingCycleStart":"2026-04-02T00:00:00Z","billingCycleEnd":"2026-05-02T00:00:00Z","individualUsage":{"plan":{"totalPercentUsed":percent}}});
        assert_eq!(parse_provider_quota(QuotaProvider::Cursor, &cursor.to_string()).unwrap().monthly.unwrap().used_tenths, expected);
        let grok = serde_json::json!({"config":{"creditUsagePercent":percent,"billingPeriodStart":"2026-04-02T00:00:00Z","billingPeriodEnd":"2026-04-09T00:00:00Z"}});
        assert_eq!(parse_provider_quota(QuotaProvider::Grok, &grok.to_string()).unwrap().weekly.unwrap().used_tenths, expected);
    }
}

/// 合法空正文表示无目标窗口，不伪造零用量。
#[test]
fn empty_object_is_a_connected_response_without_target_windows() {
    let quota = parse_provider_quota(QuotaProvider::Cursor, "{}").unwrap();
    assert!(quota.five_hour.is_none() && quota.weekly.is_none());
}

/// 套餐标签过滤身份信息而不丢弃合法额度。
#[test]
fn plan_label_drops_email_and_non_ascii() {
    let body = r#"{"plan_type":"a@b.com","rate_limit":{"primary_window":{"used_percent":1,"limit_window_seconds":18000}}}"#;
    let quota = parse_provider_quota(QuotaProvider::Codex, body).unwrap();
    assert!(quota.plan.is_none());
    assert!(quota.five_hour.is_some());
}

/// Cursor 账单月进入独立月槽并可安全投影给终端。
#[test]
fn cursor_monthly_billing_cycle_fills_only_the_monthly_window() {
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
    let monthly = quota.monthly.unwrap();
    assert_eq!((monthly.kind, monthly.used_tenths), (QuotaWindowKind::Monthly, 1000));
    assert_eq!(monthly.reset_at_unix, Some(1_777_731_115));
    assert!(quota.has_usage_window());
    let snapshot = super::ProviderQuotaSnapshot::project(Some(1), &[quota]).unwrap();
    snapshot.validate().unwrap();
    assert_eq!(serde_json::to_value(&snapshot).unwrap()["providers"][0]["monthly"]["kind"], "monthly");
}

/// Cursor 缺百分数时由容量比例解释七日额度。
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

/// Grok 月积分不冒充五小时、周或 Cursor 月窗口。
#[test]
fn grok_monthly_credits_stay_outside_both_windows() {
    let body = r#"{"plan":"pro","config":{"creditUsagePercent":40,"billingPeriodStart":"2026-04-01T00:00:00Z","billingPeriodEnd":"2026-05-01T00:00:00Z"}}"#;
    let quota = parse_provider_quota(QuotaProvider::Grok, body).unwrap();
    assert_eq!(quota.plan.as_deref(), Some("pro"));
    assert!(quota.five_hour.is_none() && quota.weekly.is_none() && quota.monthly.is_none());
}

/// Grok 嵌套积分值在明确五小时周期内正确换算。
#[test]
fn grok_five_hour_period_uses_nested_credit_values() {
    let body = r#"{"config":{"used":{"val":12.5},"monthlyLimit":{"val":100},"currentPeriod":{"start":"2026-04-02T00:00:00Z","end":"2026-04-02T05:00:00Z"}}}"#;
    let quota = parse_provider_quota(QuotaProvider::Grok, body).unwrap();
    assert_eq!(quota.five_hour.unwrap().used_tenths, 125);
    assert!(quota.weekly.is_none());
}

/// 只有账单终点不能证明存在周周期。
#[test]
fn workbuddy_end_time_alone_is_not_a_weekly_window() {
    let body = r#"{"data":{"Response":{"Data":{"Accounts":[{"CycleCapacitySize":100,"CycleCapacityUsed":25,"CycleEndTime":"2026-04-09 00:00:00"}]}}}}"#;
    let quota = parse_provider_quota(QuotaProvider::WorkBuddy, body).unwrap();
    assert!(quota.five_hour.is_none() && quota.weekly.is_none());
}

/// 有明确周期的容量可换算额度，无时长或无限额不伪造窗口。
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

/// Codex 会话分钟数决定窗口，缺失时长的本地记录不猜测。
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

/// Grok 日志上下文中的真实七日积分窗口可以解释。
#[test]
fn grok_log_line_reads_weekly_period_from_context_config() {
    let body = r#"{"msg":"billing: fetched credits config","ctx":{"config":{"creditUsagePercent":12.5,"currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY","start":"2026-04-02T00:00:00Z","end":"2026-04-09T00:00:00Z"}}}}"#;
    let quota = parse_provider_quota(QuotaProvider::Grok, body).unwrap();
    assert_eq!(quota.weekly.unwrap().used_tenths, 125);
    assert!(quota.five_hour.is_none());
}

/// statusline 记录只带两个窗口的数值，丢掉会话、路径、模型等其它字段。
#[test]
fn claude_statusline_record_keeps_only_rate_limit_numbers() {
    let input = br#"{"session_id":"s","cwd":"/private/path","model":{"id":"m"},"rate_limits":{"five_hour":{"used_percentage":12.5,"resets_at":1700000000,"extra":"x"},"seven_day":{"used_percentage":0.4,"resets_at":1700600000},"spend_limit":{"used_percentage":3,"resets_at":1}}}"#;
    let record = super::claude_statusline_quota_record(input).unwrap();
    let text = String::from_utf8(record).unwrap();
    assert!(!text.contains("private") && !text.contains("session") && !text.contains("spend_limit") && !text.contains("extra"));
    let quota = parse_provider_quota(QuotaProvider::Claude, &text).unwrap();
    assert_eq!(quota.five_hour.unwrap().used_tenths, 125);
    assert_eq!(quota.five_hour.unwrap().reset_at_unix, Some(1_700_000_000));
    // 0.4 是百分数，不能按 0..1 比例放大成 40%。
    assert_eq!(quota.weekly.unwrap().used_tenths, 4);
    assert!(quota.monthly.is_none() && quota.plan.is_none());
    assert!(super::claude_statusline_quota_record(br#"{"session_id":"s"}"#).is_none());
    assert!(super::claude_statusline_quota_record(br#"{"rate_limits":{"five_hour":{"resets_at":1}}}"#).is_none());
    assert!(super::claude_statusline_quota_record(b"not json").is_none());
    assert!(super::claude_statusline_quota_record(&vec![b' '; 256 * 1024 + 1]).is_none());
}

/// 过了重置时刻的本机窗口被丢弃，未给重置时刻的窗口保留。
#[test]
fn expired_windows_are_dropped_against_the_current_time() {
    let body = r#"{"rate_limits":{"five_hour":{"used_percentage":50,"resets_at":100},"seven_day":{"used_percentage":10,"resets_at":200}}}"#;
    let mut quota = parse_provider_quota(QuotaProvider::Claude, body).unwrap();
    quota.drop_expired_windows(100);
    assert!(quota.five_hour.is_none());
    assert_eq!(quota.weekly.unwrap().used_tenths, 100);
    assert_eq!(quota.windows().count(), 1);
    quota.drop_expired_windows(200);
    assert!(!quota.has_usage_window());
}

/// 仅启用Codex和Cursor时额度严格随开关裁剪，全禁用明确撤下旧列表。
#[test]
fn terminal_quota_follows_enabled_agents_without_changing_windows() {
    let quotas = supported_quota_providers().map(|provider| {
        let mut quota = parse_provider_quota(provider, "{}").unwrap();
        quota.weekly = Some(super::QuotaWindow { kind: QuotaWindowKind::Weekly, used_tenths: 370, reset_at_unix: Some(456) });
        quota
    });
    let snapshot = super::ProviderQuotaSnapshot::project(Some(123), &quotas).unwrap();
    let visible = snapshot.clone().for_enabled_agents(&[AiTool::Codex, AiTool::Cursor]);
    assert_eq!(visible.providers.iter().map(|quota| quota.provider).collect::<Vec<_>>(), [QuotaProvider::Codex, QuotaProvider::Cursor]);
    assert_eq!(visible.observed_at_unix, snapshot.observed_at_unix);
    assert_eq!(visible.providers[0], snapshot.providers[0]);
    assert_eq!(visible.providers[1], snapshot.providers[2]);
    assert_eq!(visible.clone().for_enabled_agents(&[AiTool::Codex, AiTool::Cursor]), visible);
    assert_eq!(snapshot.clone().for_enabled_agents(&[AiTool::ClaudeCode]).providers, vec![snapshot.providers[1].clone()]);
    assert!(snapshot.for_enabled_agents(&[]).providers.is_empty());
}

/// 联名套餐以明确的周用量周期和毫秒重置时刻展示，不能沿用账单月。
#[test]
fn cursor_dashboard_shared_weekly_pool_uses_plan_period() {
    let body = serde_json::json!({
        "enabled": true,
        "billingCycleStart": "1775088000000",
        "billingCycleEnd": "1777680000000",
        "planInfo": {"planName": "Super", "includedUsagePeriod": "INCLUDED_USAGE_PERIOD_WEEKLY", "includedUsageResetsAt": "1775692800000"},
        "planUsage": {"totalPercentUsed": 12.34, "includedSpend": 1234, "limit": 10000}
    });
    let quota = parse_provider_quota(QuotaProvider::Cursor, &body.to_string()).unwrap();
    assert_eq!(quota.plan.as_deref(), Some("Super"));
    assert!(quota.monthly.is_none());
    let weekly = quota.weekly.expect("shared plan must expose a weekly window");
    assert_eq!(weekly.used_tenths, 123);
    assert_eq!(weekly.reset_at_unix, Some(1_775_692_800));
    super::ProviderQuotaSnapshot::project(Some(123), &[quota]).unwrap().validate().unwrap();
}

/// 新接口月套餐继续显示月额度，缺百分数时由同一额度桶的已用和上限计算。
#[test]
fn cursor_dashboard_monthly_plan_and_numeric_enum_keep_units() {
    let body = serde_json::json!({
        "enabled": true,
        "billingCycleEnd": "1777680000000",
        "planInfo": {"planName": "Pro", "includedUsagePeriod": 1, "billingCycleEnd": "1777680000000"},
        "planUsage": {"includedSpend": 500, "limit": 2000}
    });
    let quota = parse_provider_quota(QuotaProvider::Cursor, &body.to_string()).unwrap();
    assert_eq!(quota.plan.as_deref(), Some("Pro"));
    assert!(quota.weekly.is_none());
    assert_eq!(quota.monthly.unwrap().used_tenths, 250);
    assert_eq!(quota.monthly.unwrap().reset_at_unix, Some(1_777_680_000));
    super::ProviderQuotaSnapshot::project(Some(123), &[quota]).unwrap().validate().unwrap();
}

/// 停用、未知周期、未知比例和坏重置时刻不能伪造成零用量或错误的月窗口。
#[test]
fn cursor_dashboard_rejects_unavailable_or_incomplete_pool() {
    let base = serde_json::json!({
        "enabled": true,
        "billingCycleStart": "1775088000000", "billingCycleEnd": "1777680000000",
        "planInfo": {"planName": "Super", "includedUsagePeriod": 2, "includedUsageResetsAt": "1775692800000"},
        "planUsage": {"totalPercentUsed": 0}
    });
    let mut disabled = base.clone(); disabled["enabled"] = false.into();
    let mut unknown = base.clone(); unknown["planInfo"]["includedUsagePeriod"] = 3.into();
    let mut absent = base.clone(); absent["planUsage"] = serde_json::json!({});
    let mut zero_limit = base.clone(); zero_limit["planUsage"] = serde_json::json!({"includedSpend": 0, "limit": 0});
    let mut bad_reset = base.clone(); bad_reset["planInfo"]["includedUsageResetsAt"] = "bad".into();
    for body in [disabled, unknown, absent, zero_limit, bad_reset] {
        assert!(!parse_provider_quota(QuotaProvider::Cursor, &body.to_string()).unwrap().has_usage_window(), "{body}");
    }
    assert_eq!(parse_provider_quota(QuotaProvider::Cursor, &base.to_string()).unwrap().weekly.unwrap().remaining_tenths(), 1000);
    let mut exceeded = base;
    exceeded["planUsage"]["totalPercentUsed"] = 123.4.into();
    assert_eq!(parse_provider_quota(QuotaProvider::Cursor, &exceeded.to_string()).unwrap().weekly.unwrap().remaining_tenths(), 0);
}
