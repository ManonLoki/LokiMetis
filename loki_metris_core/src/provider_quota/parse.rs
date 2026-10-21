//! 各提供商用量正文的纯解释。未知字段忽略，不把月度周期写成周窗口。

use serde_json::Value;

use super::{
    QuotaFailure, QuotaProvider, QuotaWindow, QuotaWindowKind, ProviderQuota, classify_window_seconds,
};

const MAX_PLAN_CHARS: usize = 24;

/// 解释一份用量正文。空对象是合法的“没有目标窗口”，坏 JSON 才是格式错误。
pub(super) fn parse_provider_quota(provider: QuotaProvider, body: &str) -> Result<ProviderQuota, QuotaFailure> {
    let value: Value = serde_json::from_str(body).map_err(|_| QuotaFailure::Malformed)?;
    let object = value.as_object().ok_or(QuotaFailure::Malformed)?;
    let mut quota = ProviderQuota { provider, plan: None, five_hour: None, weekly: None };
    quota.plan = plan_label(provider, object);
    absorb_named_windows(&mut quota, object.get("rate_limit"));
    absorb_named_windows(&mut quota, object.get("rateLimit"));
    match provider {
        QuotaProvider::Codex => absorb_codex_convention(&mut quota, object.get("rate_limit").or_else(|| object.get("rateLimit"))),
        QuotaProvider::Cursor => absorb_cursor(&mut quota, object),
        QuotaProvider::Grok => absorb_grok(&mut quota, object),
        QuotaProvider::WorkBuddy => absorb_workbuddy(&mut quota, object),
    }
    Ok(quota)
}

fn absorb_named_windows(quota: &mut ProviderQuota, rate_limit: Option<&Value>) {
    let Some(rate_limit) = rate_limit.and_then(Value::as_object) else { return };
    for key in ["primary_window", "primaryWindow", "secondary_window", "secondaryWindow", "five_hour", "fiveHour", "weekly"] {
        if let Some(window) = rate_limit.get(key).and_then(window_from_value) {
            assign(quota, window);
        }
    }
    if let Some(list) = rate_limit.get("windows").and_then(Value::as_array) {
        for item in list {
            if let Some(window) = window_from_value(item) {
                assign(quota, window);
            }
        }
    }
}

/// Codex 在缺秒数时，primary 是 5 小时、secondary 是周窗口。有秒数时以秒数为准。
fn absorb_codex_convention(quota: &mut ProviderQuota, rate_limit: Option<&Value>) {
    let Some(rate_limit) = rate_limit.and_then(Value::as_object) else { return };
    if quota.five_hour.is_none() {
        if let Some(window) = conventional_window(rate_limit, &["primary_window", "primaryWindow"], QuotaWindowKind::FiveHour) {
            assign(quota, window);
        }
    }
    if quota.weekly.is_none() {
        if let Some(window) = conventional_window(rate_limit, &["secondary_window", "secondaryWindow"], QuotaWindowKind::Weekly) {
            assign(quota, window);
        }
    }
}

fn conventional_window(rate_limit: &serde_json::Map<String, Value>, keys: &[&str], kind: QuotaWindowKind) -> Option<QuotaWindow> {
    let value = keys.iter().find_map(|key| rate_limit.get(*key))?;
    if value.get("limit_window_seconds").or_else(|| value.get("limitWindowSeconds")).is_some() {
        return None;
    }
    let used = used_tenths(value)?;
    Some(QuotaWindow { kind, used_tenths: used, reset_at_unix: reset_unix(value) })
}

fn absorb_cursor(quota: &mut ProviderQuota, object: &serde_json::Map<String, Value>) {
    let start = object.get("billingCycleStart").and_then(Value::as_str).and_then(parse_unix);
    let end = object.get("billingCycleEnd").and_then(Value::as_str).and_then(parse_unix);
    let kind = start.zip(end).and_then(|(start, end)| classify_window_seconds(end.saturating_sub(start)));
    let Some(kind) = kind else { return };
    let Some(used) = cursor_used_tenths(object) else { return };
    assign(quota, QuotaWindow { kind, used_tenths: used, reset_at_unix: end });
}

fn cursor_used_tenths(object: &serde_json::Map<String, Value>) -> Option<u16> {
    let plan = object.get("individualUsage")?.get("plan")?.as_object()?;
    if let Some(percent) = plan.get("totalPercentUsed").and_then(number_tenths) {
        return Some(percent);
    }
    let used = plan.get("used").and_then(Value::as_f64)?;
    let limit = plan.get("limit").and_then(Value::as_f64).filter(|limit| *limit > 0.0)?;
    tenths_from_ratio(used / limit)
}

fn absorb_grok(quota: &mut ProviderQuota, object: &serde_json::Map<String, Value>) {
    let config = object.get("config").and_then(Value::as_object).unwrap_or(object);
    let start = period_unix(config, &["billingPeriodStart", "start"]);
    let end = period_unix(config, &["billingPeriodEnd", "end"]);
    let nested = config.get("currentPeriod").and_then(Value::as_object);
    let start = start.or_else(|| nested.and_then(|period| period_unix(period, &["start"])));
    let end = end.or_else(|| nested.and_then(|period| period_unix(period, &["end"])));
    let kind = start.zip(end).and_then(|(start, end)| classify_window_seconds(end.saturating_sub(start)));
    let Some(kind) = kind else { return };
    let used = config.get("creditUsagePercent").and_then(number_tenths).or_else(|| {
        let used = config.get("used").and_then(nested_val).or_else(|| config.get("used").and_then(Value::as_f64))?;
        let limit = config.get("monthlyLimit").and_then(nested_val).filter(|limit| *limit > 0.0)?;
        tenths_from_ratio(used / limit)
    });
    let Some(used) = used else { return };
    assign(quota, QuotaWindow { kind, used_tenths: used, reset_at_unix: end });
}

fn nested_val(value: &Value) -> Option<f64> {
    value.get("val").and_then(Value::as_f64).or_else(|| value.as_f64())
}

fn absorb_workbuddy(quota: &mut ProviderQuota, object: &serde_json::Map<String, Value>) {
    let accounts = workbuddy_accounts(object);
    for account in accounts {
        let Some(account) = account.as_object() else { continue };
        let seconds = account.get("limit_window_seconds").or_else(|| account.get("limitWindowSeconds")).and_then(Value::as_i64)
            .or_else(|| cycle_seconds(account));
        let Some(kind) = seconds.and_then(classify_window_seconds) else { continue };
        let used = account_used_tenths(account);
        let Some(used) = used else { continue };
        let reset = account.get("CycleEndTime").and_then(Value::as_str).and_then(parse_unix)
            .or_else(|| account.get("reset_at").or_else(|| account.get("resetAt")).and_then(reset_unix));
        assign(quota, QuotaWindow { kind, used_tenths: used, reset_at_unix: reset });
    }
}

fn workbuddy_accounts(object: &serde_json::Map<String, Value>) -> Vec<&Value> {
    let mut found = Vec::new();
    if let Some(list) = object.get("Accounts").and_then(Value::as_array) {
        found.extend(list);
    }
    for key in ["data", "Response", "Data"] {
        if let Some(child) = object.get(key).and_then(Value::as_object) {
            found.extend(workbuddy_accounts(child));
        }
    }
    found
}

fn cycle_seconds(account: &serde_json::Map<String, Value>) -> Option<i64> {
    let start = account.get("CycleStartTime").and_then(Value::as_str).and_then(parse_unix)?;
    let end = account.get("CycleEndTime").and_then(Value::as_str).and_then(parse_unix)?;
    Some(end.saturating_sub(start))
}

fn account_used_tenths(account: &serde_json::Map<String, Value>) -> Option<u16> {
    if let Some(percent) = account.get("used_percent").or_else(|| account.get("usedPercent")).and_then(number_tenths) {
        return Some(percent);
    }
    let size = account.get("CycleCapacitySize").and_then(Value::as_f64).filter(|size| *size > 0.0)?;
    let used = account.get("CycleCapacityUsed").and_then(Value::as_f64).or_else(|| {
        let remain = account.get("CycleCapacityRemain").and_then(Value::as_f64)?;
        Some((size - remain).max(0.0))
    })?;
    tenths_from_ratio(used / size)
}

fn window_from_value(value: &Value) -> Option<QuotaWindow> {
    let seconds = value.get("limit_window_seconds").or_else(|| value.get("limitWindowSeconds")).and_then(Value::as_i64)?;
    let kind = classify_window_seconds(seconds)?;
    let used = used_tenths(value)?;
    Some(QuotaWindow { kind, used_tenths: used, reset_at_unix: reset_unix(value) })
}

fn used_tenths(value: &Value) -> Option<u16> {
    value.get("used_percent").or_else(|| value.get("usedPercent")).and_then(number_tenths)
}

fn number_tenths(value: &Value) -> Option<u16> {
    let number = value.as_f64()?;
    let percent = if (0.0..=1.0).contains(&number) && number != 0.0 && number != 1.0 { number * 100.0 } else { number };
    if !(0.0..=100.0).contains(&percent) {
        return None;
    }
    Some((percent * 10.0).round().clamp(0.0, 1000.0) as u16)
}

fn tenths_from_ratio(ratio: f64) -> Option<u16> {
    if !ratio.is_finite() || ratio < 0.0 {
        return None;
    }
    let percent = (ratio * 100.0).clamp(0.0, 100.0);
    Some((percent * 10.0).round() as u16)
}

fn reset_unix(value: &Value) -> Option<i64> {
    value.get("reset_at").or_else(|| value.get("resetAt")).and_then(Value::as_i64)
        .or_else(|| value.get("reset_at").or_else(|| value.get("resetAt")).and_then(Value::as_str).and_then(parse_unix))
}

fn period_unix(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|key| object.get(*key).and_then(Value::as_str).and_then(parse_unix))
}

fn parse_unix(text: &str) -> Option<i64> {
    let text = text.trim();
    if let Ok(timestamp) = text.parse::<jiff::Timestamp>() {
        return Some(timestamp.as_second());
    }
    // WorkBuddy 周期字段常用空格分隔的本地时间，没有时区时按 UTC 计算跨度。
    let (date, time) = text.split_once(' ')?;
    format!("{date}T{time}Z").parse::<jiff::Timestamp>().ok().map(|timestamp| timestamp.as_second())
}

fn assign(quota: &mut ProviderQuota, window: QuotaWindow) {
    let slot = match window.kind {
        QuotaWindowKind::FiveHour => &mut quota.five_hour,
        QuotaWindowKind::Weekly => &mut quota.weekly,
    };
    if slot.is_none() {
        *slot = Some(window);
    }
}

fn plan_label(provider: QuotaProvider, object: &serde_json::Map<String, Value>) -> Option<String> {
    let raw = match provider {
        QuotaProvider::Codex => object.get("plan_type").or_else(|| object.get("planType")),
        QuotaProvider::Cursor => object.get("membershipType"),
        QuotaProvider::Grok => object.get("plan").or_else(|| object.get("plan_type")),
        QuotaProvider::WorkBuddy => object.get("plan"),
    }?;
    let text = raw.as_str()?.trim();
    if text.is_empty() || text.chars().count() > MAX_PLAN_CHARS || text.contains('@') {
        return None;
    }
    if !text.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '_' | '-' | '.')) {
        return None;
    }
    Some(text.to_owned())
}
