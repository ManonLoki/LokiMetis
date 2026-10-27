//! 各提供商用量正文的纯解释。未知字段忽略，月度周期只进入月窗口。

use serde_json::Value;

use super::{
    QuotaFailure, QuotaProvider, QuotaWindow, QuotaWindowKind, ProviderQuota, classify_window_seconds,
};

const MAX_PLAN_CHARS: usize = 24;
/// statusline 输入大小上限；解析后仅保留已知额度字段。
const MAX_STATUSLINE_INPUT: usize = 256 * 1024;

/// 解释一份用量正文。空对象是合法的“没有目标窗口”；无效 JSON 或非对象根为格式错误。
pub(super) fn parse_provider_quota(provider: QuotaProvider, body: &str) -> Result<ProviderQuota, QuotaFailure> {
    let value: Value = serde_json::from_str(body).map_err(|_| QuotaFailure::Malformed)?;
    let object = value.as_object().ok_or(QuotaFailure::Malformed)?;
    let mut quota = ProviderQuota { provider, plan: None, five_hour: None, weekly: None, monthly: None };
    quota.plan = plan_label(provider, object);
    absorb_named_windows(&mut quota, object.get("rate_limit"));
    absorb_named_windows(&mut quota, object.get("rateLimit"));
    absorb_named_windows(&mut quota, object.get("rate_limits"));
    absorb_named_windows(&mut quota, nested_rate_limits(object));
    match provider {
        QuotaProvider::Codex => absorb_codex_convention(&mut quota, object.get("rate_limit").or_else(|| object.get("rateLimit"))),
        QuotaProvider::Claude => absorb_claude(&mut quota, object.get("rate_limits")),
        QuotaProvider::Cursor => absorb_cursor(&mut quota, object),
        QuotaProvider::Grok => absorb_grok(&mut quota, object),
        QuotaProvider::WorkBuddy => absorb_workbuddy(&mut quota, object),
    }
    // 只有 Cursor 按账单月计额度；其它提供商的月度周期不是目标窗口。
    if provider != QuotaProvider::Cursor {
        quota.monthly = None;
    }
    Ok(quota)
}

/// 从已知窗口字段和窗口数组中收集带明确时长的额度。
fn absorb_named_windows(quota: &mut ProviderQuota, rate_limit: Option<&Value>) {
    let Some(rate_limit) = rate_limit.and_then(Value::as_object) else { return };
    for key in ["primary_window", "primaryWindow", "secondary_window", "secondaryWindow", "primary", "secondary", "five_hour", "fiveHour", "weekly"] {
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

/// 仅在秒数缺席时按 Codex 固定窗口约定解释用量。
fn conventional_window(rate_limit: &serde_json::Map<String, Value>, keys: &[&str], kind: QuotaWindowKind) -> Option<QuotaWindow> {
    let value = keys.iter().find_map(|key| rate_limit.get(*key))?;
    if value.get("limit_window_seconds").or_else(|| value.get("limitWindowSeconds")).is_some() {
        return None;
    }
    let used = used_tenths(value)?;
    Some(QuotaWindow { kind, used_tenths: used, reset_at_unix: reset_unix(value) })
}

/// Claude Code statusline 的窗口名本身就是时长，`used_percentage` 恒为 0..100 的百分数。
fn absorb_claude(quota: &mut ProviderQuota, rate_limits: Option<&Value>) {
    let Some(rate_limits) = rate_limits.and_then(Value::as_object) else { return };
    for (key, kind) in [("five_hour", QuotaWindowKind::FiveHour), ("seven_day", QuotaWindowKind::Weekly)] {
        let Some(window) = rate_limits.get(key).and_then(Value::as_object) else { continue };
        let Some(percent) = window.get("used_percentage").and_then(Value::as_f64).filter(|percent| percent.is_finite() && *percent >= 0.0) else { continue };
        let used = (percent.min(100.0) * 10.0).round() as u16;
        let reset = window.get("resets_at").and_then(json_i64).filter(|reset| *reset >= 0);
        assign(quota, QuotaWindow { kind, used_tenths: used, reset_at_unix: reset });
    }
}

/// statusline 输入可能带完整会话信息，落盘前只复制两个已知窗口的两个数值字段。
pub(super) fn claude_statusline_record(input: &[u8]) -> Option<Vec<u8>> {
    if input.len() > MAX_STATUSLINE_INPUT {
        return None;
    }
    let value: Value = serde_json::from_slice(input).ok()?;
    let rate_limits = value.get("rate_limits")?.as_object()?;
    let mut kept = serde_json::Map::new();
    for key in ["five_hour", "seven_day"] {
        let Some(window) = rate_limits.get(key).and_then(Value::as_object) else { continue };
        let Some(used) = window.get("used_percentage").filter(|used| used.is_number()) else { continue };
        let mut copy = serde_json::Map::new();
        copy.insert("used_percentage".to_owned(), used.clone());
        if let Some(reset) = window.get("resets_at").filter(|reset| reset.is_number()) {
            copy.insert("resets_at".to_owned(), reset.clone());
        }
        kept.insert(key.to_owned(), Value::Object(copy));
    }
    if kept.is_empty() {
        return None;
    }
    serde_json::to_vec(&serde_json::json!({ "rate_limits": kept })).ok()
}

/// 按账单周期时长归类 Cursor 额度并保留真实重置时间。
fn absorb_cursor(quota: &mut ProviderQuota, object: &serde_json::Map<String, Value>) {
    if let Some(info) = object.get("planInfo").and_then(Value::as_object) {
        absorb_cursor_dashboard(quota, object, info);
        return;
    }
    let start = object.get("billingCycleStart").and_then(Value::as_str).and_then(parse_unix);
    let end = object.get("billingCycleEnd").and_then(Value::as_str).and_then(parse_unix);
    let kind = start.zip(end).and_then(|(start, end)| classify_window_seconds(end.saturating_sub(start)));
    let Some(kind) = kind else { return };
    let Some(used) = cursor_used_tenths(object) else { return };
    assign(quota, QuotaWindow { kind, used_tenths: used, reset_at_unix: end });
}

/// Cursor 官方 DashboardService 的周期枚举优先于账单月，所有时间字段为 Unix 毫秒。
fn absorb_cursor_dashboard(quota: &mut ProviderQuota, object: &serde_json::Map<String, Value>, info: &serde_json::Map<String, Value>) {
    if object.get("enabled").and_then(Value::as_bool) != Some(true) {
        return;
    }
    let Some(plan) = object.get("planUsage").and_then(Value::as_object) else { return };
    let Some(period) = info.get("includedUsagePeriod") else { return };
    let kind = match (period.as_i64(), period.as_str()) {
        (Some(1), _) | (_, Some("INCLUDED_USAGE_PERIOD_MONTHLY")) => QuotaWindowKind::Monthly,
        (Some(2), _) | (_, Some("INCLUDED_USAGE_PERIOD_WEEKLY")) => QuotaWindowKind::Weekly,
        _ => return,
    };
    let reset = match kind {
        QuotaWindowKind::Weekly => info.get("includedUsageResetsAt"),
        _ => info.get("billingCycleEnd").or_else(|| object.get("billingCycleEnd")),
    }.and_then(cursor_millis);
    let Some(reset) = reset else { return };
    let used = plan.get("totalPercentUsed").and_then(Value::as_f64)
        .and_then(|percent| tenths_from_ratio(percent / 100.0))
        .or_else(|| {
            let used = plan.get("includedSpend")?.as_f64()?;
            let limit = plan.get("limit")?.as_f64().filter(|limit| *limit > 0.0)?;
            tenths_from_ratio(used / limit)
        });
    let Some(used) = used else { return };
    assign(quota, QuotaWindow { kind, used_tenths: used, reset_at_unix: Some(reset) });
}

/// Protobuf 的 int64 JSON 以十进制字符串或整数传输；不把毫秒误作秒。
fn cursor_millis(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str()?.parse().ok()).filter(|millis| *millis > 0).map(|millis| millis / 1000)
}

/// 优先读取明确百分数，缺失时才由已用量和上限计算比例。
fn cursor_used_tenths(object: &serde_json::Map<String, Value>) -> Option<u16> {
    let plan = object.get("individualUsage")?.get("plan")?.as_object()?;
    if let Some(percent) = plan.get("totalPercentUsed").and_then(number_tenths) {
        return Some(percent);
    }
    let used = plan.get("used").and_then(Value::as_f64)?;
    let limit = plan.get("limit").and_then(Value::as_f64).filter(|limit| *limit > 0.0)?;
    tenths_from_ratio(used / limit)
}

/// 从 Grok 账单周期和积分字段解释具有明确时长的窗口。
fn absorb_grok(quota: &mut ProviderQuota, object: &serde_json::Map<String, Value>) {
    let config = grok_config(object);
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

/// 定位直接或日志上下文中嵌套的 Grok 账单配置。
fn grok_config(object: &serde_json::Map<String, Value>) -> &serde_json::Map<String, Value> {
    object.get("config").and_then(Value::as_object)
        .or_else(|| object.get("ctx").and_then(Value::as_object).and_then(|ctx| ctx.get("config")).and_then(Value::as_object))
        .unwrap_or(object)
}

/// 从会话事件正文提取嵌套额度对象。
fn nested_rate_limits(object: &serde_json::Map<String, Value>) -> Option<&Value> {
    let payload = object.get("payload")?.as_object()?;
    payload.get("rate_limits").or_else(|| payload.get("rate_limit")).or_else(|| payload.get("rateLimit"))
}

/// 兼容数值本身及带 val 字段的积分表示。
fn nested_val(value: &Value) -> Option<f64> {
    value.get("val").and_then(Value::as_f64).or_else(|| value.as_f64())
}

/// 仅将具有可确认周期的账户额度映射为目标窗口。
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

/// 按已知响应封装收集 WorkBuddy 账户记录。
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

/// 由完整周期起止时间计算窗口长度，缺一端时拒绝猜测。
fn cycle_seconds(account: &serde_json::Map<String, Value>) -> Option<i64> {
    let start = account.get("CycleStartTime").and_then(Value::as_str).and_then(parse_unix)?;
    let end = account.get("CycleEndTime").and_then(Value::as_str).and_then(parse_unix)?;
    Some(end.saturating_sub(start))
}

/// 优先使用百分数，否则从容量与已用或剩余量推导用量。
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

/// 用明确秒数或分钟数归类窗口并解释百分数和重置时间。
fn window_from_value(value: &Value) -> Option<QuotaWindow> {
    let seconds = value.get("limit_window_seconds").or_else(|| value.get("limitWindowSeconds")).and_then(json_i64)
        .or_else(|| value.get("window_minutes").or_else(|| value.get("windowMinutes")).and_then(json_i64).map(|minutes| minutes.saturating_mul(60)))?;
    let kind = classify_window_seconds(seconds)?;
    let used = used_tenths(value)?;
    Some(QuotaWindow { kind, used_tenths: used, reset_at_unix: reset_unix(value) })
}

/// 按已知字段名读取百分数，保持百分数单位。
fn used_tenths(value: &Value) -> Option<u16> {
    value.get("used_percent").or_else(|| value.get("usedPercent")).and_then(number_tenths)
}

/// 将零到一百的百分数转换为十分之一百分点，不猜测比例单位。
fn number_tenths(value: &Value) -> Option<u16> {
    let percent = value.as_f64()?;
    if !(0.0..=100.0).contains(&percent) {
        return None;
    }
    Some((percent * 10.0).round().clamp(0.0, 1000.0) as u16)
}

/// 只对已用量除以上限的比例换算百分数并限制最大值。
fn tenths_from_ratio(ratio: f64) -> Option<u16> {
    if !ratio.is_finite() || ratio < 0.0 {
        return None;
    }
    let percent = (ratio * 100.0).clamp(0.0, 100.0);
    Some((percent * 10.0).round() as u16)
}

/// 兼容已知重置字段中的 Unix 秒和带时区时间。
fn reset_unix(value: &Value) -> Option<i64> {
    ["reset_at", "resetAt", "resets_at", "resetsAt"].iter().find_map(|key| value.get(*key).and_then(json_i64))
        .or_else(|| ["reset_at", "resetAt", "resets_at", "resetsAt"].iter().find_map(|key| value.get(*key).and_then(Value::as_str).and_then(parse_unix)))
}

/// 读取有界整数或有限浮点秒数，拒绝超出整数范围的值。
fn json_i64(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_f64().filter(|number| number.is_finite() && *number >= i64::MIN as f64 && *number <= i64::MAX as f64).map(|number| number as i64))
}

/// 按字段优先序读取账单周期的时间字符串。
fn period_unix(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|key| object.get(*key).and_then(Value::as_str).and_then(parse_unix))
}

/// 解析带时区时间，兼容无时区账单日期的 UTC 跨度计算。
fn parse_unix(text: &str) -> Option<i64> {
    let text = text.trim();
    if let Ok(timestamp) = text.parse::<jiff::Timestamp>() {
        return Some(timestamp.as_second());
    }
    // WorkBuddy 周期字段常用空格分隔的本地时间，没有时区时按 UTC 计算跨度。
    let (date, time) = text.split_once(' ')?;
    format!("{date}T{time}Z").parse::<jiff::Timestamp>().ok().map(|timestamp| timestamp.as_second())
}

/// 按窗口类型填充首个合法值，避免后续弱来源覆盖既有窗口。
fn assign(quota: &mut ProviderQuota, window: QuotaWindow) {
    let slot = match window.kind {
        QuotaWindowKind::FiveHour => &mut quota.five_hour,
        QuotaWindowKind::Weekly => &mut quota.weekly,
        QuotaWindowKind::Monthly => &mut quota.monthly,
    };
    if slot.is_none() {
        *slot = Some(window);
    }
}

/// 按提供商提取有界套餐名，过滤邮箱及不安全字符。
fn plan_label(provider: QuotaProvider, object: &serde_json::Map<String, Value>) -> Option<String> {
    let raw = match provider {
        QuotaProvider::Codex => object.get("plan_type").or_else(|| object.get("planType"))
            .or_else(|| object.get("rate_limits").and_then(|limits| limits.get("plan_type").or_else(|| limits.get("planType"))))
            .or_else(|| nested_rate_limits(object).and_then(|limits| limits.get("plan_type").or_else(|| limits.get("planType")))),
        QuotaProvider::Claude => None,
        QuotaProvider::Cursor => object.get("planInfo").and_then(|info| info.get("planName")).or_else(|| object.get("membershipType")),
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
