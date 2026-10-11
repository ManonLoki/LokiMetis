//! 把 WorkBuddy usage 事件按民用小时折叠成今日/昨日固定 24 桶趋势。

use std::collections::BTreeSet;

use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

use super::WorkbuddyUsageOrigin;
use super::model_usage::sum_optional_credit;
use super::record::ValidatedWorkbuddyUsageRecord;
use crate::{LocalUsageWindow, TimeStandard, civil_date_for_timestamp, inclusive_calendar_range};

/// 单日固定输出的民用小时数；DST 重复小时合并、缺口小时补零。
const HOURS_PER_DAY: usize = 24;

/// 单日窗口内一个民用小时的请求、会话与用量。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbuddyHourlyBucket {
    /// 稳定时间键，格式 `YYYY-MM-DDTHH`。
    pub key: String,
    /// 横轴短标签，格式 `HH:00`。
    pub label: String,
    /// 该小时内产生过请求的去重会话数量。
    pub session_count: u64,
    /// 该小时上游请求数。
    pub request_count: u64,
    /// 顶层会话请求数。
    pub top_level_request_count: u64,
    /// subagent 请求数。
    pub subagent_request_count: u64,
    /// 该小时请求的总 Token。
    pub tokens: u64,
    /// 该小时逐请求积分；任一记录缺失时保持未提供。
    pub credits: Option<f64>,
    /// 该小时是否仍是观测时刻所在的进行中小时。
    pub in_progress: bool,
}

/// 今日或昨日单日窗口的完整 24 小时趋势。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbuddyHourlyTrend {
    /// 只会是 `Today` 或 `Yesterday`。
    pub window: LocalUsageWindow,
    /// 该窗口对应的民用日期，格式 `YYYY-MM-DD`。
    pub date: String,
    /// 从 00:00 到 23:00 顺序排列的 24 个小时桶。
    pub buckets: Vec<WorkbuddyHourlyBucket>,
}

/// 今日与昨日按查看时间标准生成小时趋势；观测时刻无法解析时返回空。
pub(super) fn build_workbuddy_hourly_trends(
    records: &[ValidatedWorkbuddyUsageRecord],
    now_epoch_ms: i64,
    time_standard: &TimeStandard,
    device_tz: &TimeZone,
) -> Vec<WorkbuddyHourlyTrend> {
    let Some(today) = civil_date_for_timestamp(now_epoch_ms, time_standard, device_tz) else {
        return Vec::new();
    };
    [LocalUsageWindow::Today, LocalUsageWindow::Yesterday]
        .into_iter()
        .filter_map(|window| {
            let (date, _) = inclusive_calendar_range(window, today).ok()?;
            Some(WorkbuddyHourlyTrend {
                window,
                date: date.to_string(),
                buckets: hourly_buckets_for_date(
                    records,
                    date,
                    now_epoch_ms,
                    time_standard,
                    device_tz,
                ),
            })
        })
        .collect()
}

/// 为指定民用日构造固定 24 个小时桶并累计逐事件事实。
fn hourly_buckets_for_date(
    records: &[ValidatedWorkbuddyUsageRecord],
    date: Date,
    now_epoch_ms: i64,
    time_standard: &TimeStandard,
    device_tz: &TimeZone,
) -> Vec<WorkbuddyHourlyBucket> {
    let zone = time_standard.viewing_time_zone(device_tz);
    let observed_hour = Timestamp::from_millisecond(now_epoch_ms)
        .ok()
        .map(|value| value.to_zoned(zone.clone()))
        .filter(|zoned| zoned.date() == date)
        .map(|zoned| usize::from(zoned.hour().unsigned_abs()));

    let mut buckets: Vec<WorkbuddyHourlyBucket> = (0..HOURS_PER_DAY)
        .map(|hour| WorkbuddyHourlyBucket {
            key: format!(
                "{:04}-{:02}-{:02}T{hour:02}",
                date.year(),
                date.month(),
                date.day()
            ),
            label: format!("{hour:02}:00"),
            session_count: 0,
            request_count: 0,
            top_level_request_count: 0,
            subagent_request_count: 0,
            tokens: 0,
            credits: Some(0.0),
            in_progress: observed_hour == Some(hour),
        })
        .collect();
    let mut sessions_by_hour: [BTreeSet<&str>; HOURS_PER_DAY] =
        std::array::from_fn(|_| BTreeSet::new());

    for record in records {
        let Ok(occurred_at) = Timestamp::from_millisecond(record.occurred_at_epoch_ms) else {
            continue;
        };
        let zoned = occurred_at.to_zoned(zone.clone());
        if zoned.date() != date {
            continue;
        }
        let hour = usize::from(zoned.hour().unsigned_abs());
        let Some(bucket) = buckets.get_mut(hour) else {
            continue;
        };
        sessions_by_hour[hour].insert(record.session_key.as_str());
        let request_count = u64::from(record.request_count);
        bucket.request_count = bucket.request_count.saturating_add(request_count);
        bucket.tokens = bucket.tokens.saturating_add(record.usage.total_tokens);
        bucket.credits = sum_optional_credit(bucket.credits, record.credit);
        match record.origin {
            WorkbuddyUsageOrigin::TopLevel => {
                bucket.top_level_request_count =
                    bucket.top_level_request_count.saturating_add(request_count);
            }
            WorkbuddyUsageOrigin::Subagent => {
                bucket.subagent_request_count =
                    bucket.subagent_request_count.saturating_add(request_count);
            }
        }
    }
    for (bucket, sessions) in buckets.iter_mut().zip(sessions_by_hour) {
        bucket.session_count = u64::try_from(sessions.len()).unwrap_or(u64::MAX);
    }
    buckets
}

#[cfg(test)]
#[path = "hourly_tests.rs"]
mod tests;
