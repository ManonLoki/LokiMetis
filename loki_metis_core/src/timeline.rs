//! 业务域内与时间窗口、时间戳边界和本地窗口聚合相关的纯函数。

use thiserror::Error;

use crate::aggregate::copy_matching_snapshots;
use crate::{CanonicalUsageSet, RetentionDays, TimeStandard, filter_canonical_usage};
use jiff::civil::{Date, DateTime, Time};
use jiff::tz::{AmbiguousOffset, TimeZone};
use jiff::{Timestamp, ToSpan};
use serde::{Deserialize, Serialize};

/// 标识看板、图表、用量统计与排行榜共用的日历窗口。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LocalUsageWindow {
    /// 观测日当天。
    #[default]
    Today,
    /// 观测日的前一个完整民用日，不含当天。
    Yesterday,
    /// 当前自然周：周一至当天。
    ThisWeek,
    /// 上一自然周：周一至周日。
    LastWeek,
    /// 当前自然月：当月 1 日至当天。
    ThisMonth,
    /// 上一自然月：1 日至月末。
    LastMonth,
}

impl LocalUsageWindow {
    /// 概览固定装配的六个日历窗口，顺序与界面筛选一致。
    pub const OVERVIEW_WINDOWS: [Self; 6] = [
        Self::Today,
        Self::Yesterday,
        Self::ThisWeek,
        Self::LastWeek,
        Self::ThisMonth,
        Self::LastMonth,
    ];

    /// 用量统计页缺省窗口；旧设置缺该字段时使用，与概览默认当天不同。
    pub const fn default_usage_query() -> Self {
        Self::ThisWeek
    }
}

/// 本机自然日窗口所需的 UTC 毫秒边界。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowBoundaries {
    /// 当天起点。
    pub today: i64,
    /// 昨天完整民用日起点。
    pub yesterday: i64,
    /// 近七天窗口起点。
    pub seven_days: i64,
    /// 近三十天窗口起点。
    pub thirty_days: i64,
}

impl WindowBoundaries {
    /// 从系统本地时区计算自然日边界；转换失败时回退到固定偏移。
    pub fn for_local_today(now_epoch_ms: i64) -> Self {
        Self::for_standard(now_epoch_ms, &TimeStandard::Local, &TimeZone::system())
    }

    /// 按已保存时间标准和设备时区计算扫描/保留用的当天、昨天、近 7、近 30 下界。
    pub fn for_standard(now_epoch_ms: i64, standard: &TimeStandard, device_tz: &TimeZone) -> Self {
        let zone = zone_for_standard(standard, device_tz);
        let Some(now) = Timestamp::from_millisecond(now_epoch_ms)
            .ok()
            .map(|value| value.to_zoned(zone.clone()))
        else {
            return Self::for_fixed_offset(now_epoch_ms, 0);
        };

        let Some(boundaries) = Self::for_calendar_dates(now.date(), |date| {
            day_start_epoch_ms(date, standard, device_tz)
        }) else {
            return Self::for_fixed_offset(now_epoch_ms, now.offset().seconds());
        };

        boundaries
    }

    /// 按指定观测日解析扫描/保留用的当天、昨天、近 7、近 30 边界；失败返回 `None`。
    pub fn for_calendar_dates(
        today_date: Date,
        mut day_start_epoch_ms: impl FnMut(Date) -> Option<i64>,
    ) -> Option<Self> {
        let yesterday_date = today_date.checked_sub(1.days()).ok()?;
        let seven_day_date = today_date.checked_sub(6.days()).ok()?;
        let thirty_day_date = today_date.checked_sub(29.days()).ok()?;
        Some(Self {
            today: day_start_epoch_ms(today_date)?,
            yesterday: day_start_epoch_ms(yesterday_date)?,
            seven_days: day_start_epoch_ms(seven_day_date)?,
            thirty_days: day_start_epoch_ms(thirty_day_date)?,
        })
    }

    /// 按固定 UTC 偏移构造时间线，供当地时间窗口计算使用。
    fn for_fixed_offset(now_epoch_ms: i64, offset_seconds: i32) -> Self {
        const DAY_MS: i64 = 24 * 60 * 60 * 1_000;
        let offset_ms = i64::from(offset_seconds).saturating_mul(1_000);
        let shifted_now = now_epoch_ms.saturating_add(offset_ms);
        let shifted_midnight = shifted_now.div_euclid(DAY_MS).saturating_mul(DAY_MS);
        let today = shifted_midnight.saturating_sub(offset_ms);
        Self {
            today,
            yesterday: today.saturating_sub(DAY_MS),
            seven_days: today.saturating_sub(6 * DAY_MS),
            thirty_days: today.saturating_sub(29 * DAY_MS),
        }
    }
}

/// 表示窗口边界与时间解析的输入校验失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TimelineError {
    /// 传入时间戳或日期序列无法解析。
    #[error("observed timestamp is invalid")]
    InvalidObservedTimestamp,
}

/// 选择划分民用日时使用的时区：当地时间用设备时区，UTC 模式固定 UTC。
fn zone_for_standard(standard: &TimeStandard, device_tz: &TimeZone) -> TimeZone {
    standard.viewing_time_zone(device_tz)
}

/// 从窗口与当前观测时刻生成持续窗口的自然日序列。
pub fn local_dates_for_window(
    window: LocalUsageWindow,
    observed_at_epoch_ms: i64,
) -> Result<Vec<Date>, TimelineError> {
    dates_for_window(
        window,
        observed_at_epoch_ms,
        &TimeStandard::Local,
        &TimeZone::system(),
    )
}

/// 按时间标准生成窗口内从最旧到最新的民用日序列。
pub fn dates_for_window(
    window: LocalUsageWindow,
    observed_at_epoch_ms: i64,
    standard: &TimeStandard,
    device_tz: &TimeZone,
) -> Result<Vec<Date>, TimelineError> {
    let observed = Timestamp::from_millisecond(observed_at_epoch_ms)
        .ok()
        .map(|value| value.to_zoned(zone_for_standard(standard, device_tz)))
        .ok_or(TimelineError::InvalidObservedTimestamp)?;
    let (from, to) = inclusive_calendar_range(window, observed.date())?;
    dates_inclusive(from, to)
}

/// 按观测民用日解析统一日历窗口的闭区间起止日期。
///
/// 今日与昨日各为单日；本周为当前周周一至当天；上周为上一自然周周一至周日；
/// 本月为当月 1 日至当天；上月为上一自然月整月。结束日不得晚于观测日。
pub fn inclusive_calendar_range(
    window: LocalUsageWindow,
    today: Date,
) -> Result<(Date, Date), TimelineError> {
    match window {
        LocalUsageWindow::Today => Ok((today, today)),
        LocalUsageWindow::Yesterday => {
            let yesterday = today
                .checked_sub(1.days())
                .map_err(|_| TimelineError::InvalidObservedTimestamp)?;
            Ok((yesterday, yesterday))
        }
        LocalUsageWindow::ThisWeek => Ok((monday_of_week(today)?, today)),
        LocalUsageWindow::LastWeek => {
            let this_monday = monday_of_week(today)?;
            let last_monday = this_monday
                .checked_sub(7.days())
                .map_err(|_| TimelineError::InvalidObservedTimestamp)?;
            let last_sunday = this_monday
                .checked_sub(1.days())
                .map_err(|_| TimelineError::InvalidObservedTimestamp)?;
            Ok((last_monday, last_sunday))
        }
        LocalUsageWindow::ThisMonth => Ok((today.first_of_month(), today)),
        LocalUsageWindow::LastMonth => {
            let last_last = today
                .first_of_month()
                .checked_sub(1.days())
                .map_err(|_| TimelineError::InvalidObservedTimestamp)?;
            Ok((last_last.first_of_month(), last_last))
        }
    }
}

/// 返回包含 `from` 与 `to` 的连续民用日序列；起止颠倒时失败。
fn dates_inclusive(from: Date, to: Date) -> Result<Vec<Date>, TimelineError> {
    if to < from {
        return Err(TimelineError::InvalidObservedTimestamp);
    }
    let span_days = to
        .since(from)
        .map_err(|_| TimelineError::InvalidObservedTimestamp)?
        .get_days();
    let day_count = u16::try_from(span_days)
        .ok()
        .and_then(|days| days.checked_add(1))
        .ok_or(TimelineError::InvalidObservedTimestamp)?;
    dates_from(from, day_count)
}

/// 返回从 `first` 起连续 `day_count` 个民用日的序列，从最旧到最新排列。
fn dates_from(first: Date, day_count: u16) -> Result<Vec<Date>, TimelineError> {
    (0..day_count)
        .map(|offset| {
            first
                .checked_add(i64::from(offset).days())
                .map_err(|_| TimelineError::InvalidObservedTimestamp)
        })
        .collect()
}

/// 返回包含该日的自然周周一。
fn monday_of_week(today: Date) -> Result<Date, TimelineError> {
    let offset = i64::from(today.weekday().to_monday_zero_offset());
    today
        .checked_sub(offset.days())
        .map_err(|_| TimelineError::InvalidObservedTimestamp)
}

/// 从观测时刻生成「当天及其往前连续 `day_count - 1` 个本地日」的日期序列。
///
/// 成员资格只看民用日期：`day_count = 1` 仅为观测日，`2` 为当天加前一日。
/// 扫描回补、保留窗口和 Collect 手动近 30 日仍用连续自然日，不对应界面日历窗口。
/// 日期从最旧到最新排列。
pub fn local_dates_for_day_count(
    day_count: u16,
    observed_at_epoch_ms: i64,
) -> Result<Vec<Date>, TimelineError> {
    dates_for_day_count(
        day_count,
        observed_at_epoch_ms,
        &TimeStandard::Local,
        &TimeZone::system(),
    )
}

/// 返回保留窗口下界：观测日及其往前连续 `days - 1` 个所选标准民用日的首日起点。
pub fn retention_cutoff_epoch_ms(
    days: RetentionDays,
    observed_at_epoch_ms: i64,
    standard: &TimeStandard,
    device_tz: &TimeZone,
) -> Result<i64, TimelineError> {
    let dates = dates_for_day_count(days.get(), observed_at_epoch_ms, standard, device_tz)?;
    let first = dates
        .first()
        .copied()
        .ok_or(TimelineError::InvalidObservedTimestamp)?;
    day_start_epoch_ms(first, standard, device_tz).ok_or(TimelineError::InvalidObservedTimestamp)
}

/// 生成「当天及其往前连续 `day_count - 1` 个所选标准民用日」的日期序列。
pub fn dates_for_day_count(
    day_count: u16,
    observed_at_epoch_ms: i64,
    standard: &TimeStandard,
    device_tz: &TimeZone,
) -> Result<Vec<Date>, TimelineError> {
    if day_count == 0 {
        return Err(TimelineError::InvalidObservedTimestamp);
    }

    let observed = Timestamp::from_millisecond(observed_at_epoch_ms)
        .ok()
        .map(|value| value.to_zoned(zone_for_standard(standard, device_tz)))
        .ok_or(TimelineError::InvalidObservedTimestamp)?;

    let back_span = i64::from(day_count - 1).days();
    let first = observed
        .date()
        .checked_sub(back_span)
        .map_err(|_| TimelineError::InvalidObservedTimestamp)?;

    dates_from(first, day_count)
}

/// 判断一次调用是否属于指定本机窗口：只比较系统本地民用日期，忽略时刻。
pub fn belongs_to_local_window(
    occurred_at_epoch_ms: i64,
    window: LocalUsageWindow,
    observed_at_epoch_ms: i64,
) -> Result<bool, TimelineError> {
    belongs_to_window(
        occurred_at_epoch_ms,
        window,
        observed_at_epoch_ms,
        &TimeStandard::Local,
        &TimeZone::system(),
    )
}

/// 判断一次调用是否属于指定窗口：只比较所选标准下的民用日期。
pub fn belongs_to_window(
    occurred_at_epoch_ms: i64,
    window: LocalUsageWindow,
    observed_at_epoch_ms: i64,
    standard: &TimeStandard,
    device_tz: &TimeZone,
) -> Result<bool, TimelineError> {
    let dates = dates_for_window(window, observed_at_epoch_ms, standard, device_tz)?;
    Ok(occurred_on_dates(
        occurred_at_epoch_ms,
        &dates,
        standard,
        device_tz,
    ))
}

/// 判断发生时刻的本地日期是否落在给定日期集合内。
pub fn occurred_on_local_dates(occurred_at_epoch_ms: i64, dates: &[Date]) -> bool {
    occurred_on_dates(
        occurred_at_epoch_ms,
        dates,
        &TimeStandard::Local,
        &TimeZone::system(),
    )
}

/// 判断发生时刻在所选标准下的民用日是否落在给定日期集合内。
pub fn occurred_on_dates(
    occurred_at_epoch_ms: i64,
    dates: &[Date],
    standard: &TimeStandard,
    device_tz: &TimeZone,
) -> bool {
    civil_date_for_timestamp(occurred_at_epoch_ms, standard, device_tz)
        .is_some_and(|date| dates.contains(&date))
}

/// 按窗口与观测时刻筛选 canonical 调用，成员资格只看本地日期。
pub fn filter_canonical_usage_for_local_window(
    canonical: &CanonicalUsageSet,
    window: LocalUsageWindow,
    observed_at_epoch_ms: i64,
) -> Result<CanonicalUsageSet, TimelineError> {
    filter_canonical_usage_for_window(
        canonical,
        window,
        observed_at_epoch_ms,
        &TimeStandard::Local,
        &TimeZone::system(),
    )
}

/// 按窗口、观测时刻和时间标准筛选 canonical 调用。
pub fn filter_canonical_usage_for_window(
    canonical: &CanonicalUsageSet,
    window: LocalUsageWindow,
    observed_at_epoch_ms: i64,
    standard: &TimeStandard,
    device_tz: &TimeZone,
) -> Result<CanonicalUsageSet, TimelineError> {
    let dates = dates_for_window(window, observed_at_epoch_ms, standard, device_tz)?;
    Ok(filter_canonical_usage_for_dates(
        canonical, &dates, standard, device_tz,
    ))
}

/// 按给定本地日期集合筛选 canonical 调用。
pub fn filter_canonical_usage_for_local_dates(
    canonical: &CanonicalUsageSet,
    dates: &[Date],
) -> CanonicalUsageSet {
    filter_canonical_usage_for_dates(canonical, dates, &TimeStandard::Local, &TimeZone::system())
}

/// 按给定民用日集合与时间标准筛选 canonical 调用。
pub fn filter_canonical_usage_for_dates(
    canonical: &CanonicalUsageSet,
    dates: &[Date],
    standard: &TimeStandard,
    device_tz: &TimeZone,
) -> CanonicalUsageSet {
    let filtered = filter_canonical_usage(canonical, |call| {
        occurred_on_dates(call.occurred_at_epoch_ms, dates, standard, device_tz)
    });
    copy_matching_snapshots(canonical, filtered, |snapshot| {
        occurred_on_dates(snapshot.occurred_at_epoch_ms, dates, standard, device_tz)
    })
}

/// 按单个本地日期筛选 canonical 调用，供日桶与 Collect 逐日快照共用。
pub fn filter_canonical_usage_for_local_date(
    canonical: &CanonicalUsageSet,
    date: Date,
) -> CanonicalUsageSet {
    filter_canonical_usage_for_local_dates(canonical, std::slice::from_ref(&date))
}

/// 按单个民用日与时间标准筛选 canonical 调用。
pub fn filter_canonical_usage_for_date(
    canonical: &CanonicalUsageSet,
    date: Date,
    standard: &TimeStandard,
    device_tz: &TimeZone,
) -> CanonicalUsageSet {
    filter_canonical_usage_for_dates(canonical, std::slice::from_ref(&date), standard, device_tz)
}

/// 把 Unix 毫秒时间戳转为系统时区本地日期。
pub fn local_date_for_timestamp(epoch_ms: i64) -> Option<Date> {
    civil_date_for_timestamp(epoch_ms, &TimeStandard::Local, &TimeZone::system())
}

/// 把 Unix 毫秒转为所选标准下的民用日期。
pub fn civil_date_for_timestamp(
    epoch_ms: i64,
    standard: &TimeStandard,
    device_tz: &TimeZone,
) -> Option<Date> {
    Timestamp::from_millisecond(epoch_ms).ok().map(|value| {
        value
            .to_zoned(zone_for_standard(standard, device_tz))
            .date()
    })
}

/// 返回给定日期在系统时区下的第一秒可表示时间。
pub fn local_day_start_epoch_ms(date: Date) -> Option<i64> {
    day_start_epoch_ms(date, &TimeStandard::Local, &TimeZone::system())
}

/// 返回给定民用日在所选标准下的首个可表示瞬间。
pub fn day_start_epoch_ms(
    date: Date,
    standard: &TimeStandard,
    device_tz: &TimeZone,
) -> Option<i64> {
    let zone = zone_for_standard(standard, device_tz);
    first_representable_local_instant(date, |candidate| {
        earliest_epoch_ms_in_zone(candidate, &zone)
    })
}

/// 在指定时区解析民用时刻；缺口返回 `None`，重叠取转换前偏移。
pub fn earliest_epoch_ms_in_zone(candidate: DateTime, zone: &TimeZone) -> Option<i64> {
    let offset = match zone.to_ambiguous_zoned(candidate).offset() {
        AmbiguousOffset::Unambiguous { offset } => offset,
        AmbiguousOffset::Fold { before, .. } => before,
        AmbiguousOffset::Gap { .. } => return None,
    };
    offset
        .to_timestamp(candidate)
        .ok()
        .map(|timestamp| timestamp.as_millisecond())
}

/// 按分钟再按秒搜索当天首个可表示时刻。
pub fn first_representable_local_instant(
    mut date: Date,
    mut resolve: impl FnMut(DateTime) -> Option<i64>,
) -> Option<i64> {
    const MAX_CONSECUTIVE_SKIPPED_DAYS: u8 = 2;

    for _ in 0..=MAX_CONSECUTIVE_SKIPPED_DAYS {
        if let Some(epoch_ms) = first_representable_in_date(date, &mut resolve) {
            return Some(epoch_ms);
        }
        date = date.checked_add(1.day()).ok()?;
    }

    None
}

/// 在单日内按分钟再按秒搜索可表示时刻。
pub fn first_representable_in_date(
    date: Date,
    resolve: &mut impl FnMut(DateTime) -> Option<i64>,
) -> Option<i64> {
    const MINUTES_PER_DAY: u32 = 24 * 60;
    const SECONDS_PER_MINUTE: u32 = 60;

    let midnight = civil_at(date, 0, 0, 0)?;
    if let Some(epoch_ms) = resolve(midnight) {
        return Some(epoch_ms);
    }

    for minute in 1..MINUTES_PER_DAY {
        let minute_second = minute * SECONDS_PER_MINUTE;
        let candidate = civil_at(date, minute_second / 3_600, minute % 60, 0)?;
        if resolve(candidate).is_none() {
            continue;
        }

        let previous_minute_second = (minute - 1) * SECONDS_PER_MINUTE;
        for second in (previous_minute_second + 1)..=minute_second {
            let candidate = civil_at(
                date,
                second / 3_600,
                (second % 3_600) / SECONDS_PER_MINUTE,
                second % SECONDS_PER_MINUTE,
            )?;
            if let Some(epoch_ms) = resolve(candidate) {
                return Some(epoch_ms);
            }
        }
    }

    let last_minute_second = (MINUTES_PER_DAY - 1) * SECONDS_PER_MINUTE;
    for second in (last_minute_second + 1)..(MINUTES_PER_DAY * SECONDS_PER_MINUTE) {
        let candidate = civil_at(date, 23, 59, second % SECONDS_PER_MINUTE)?;
        if let Some(epoch_ms) = resolve(candidate) {
            return Some(epoch_ms);
        }
    }

    None
}

/// 生成带边界检查的民用时刻。
pub fn civil_at(date: Date, hour: u32, minute: u32, second: u32) -> Option<DateTime> {
    let time = Time::new(
        hour.try_into().ok()?,
        minute.try_into().ok()?,
        second.try_into().ok()?,
        0,
    )
    .ok()?;
    Some(DateTime::from_parts(date, time))
}

#[cfg(test)]
#[path = "timeline_tests.rs"]
mod tests;
