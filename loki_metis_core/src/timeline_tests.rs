//! 本机自然日窗口边界与日期成员资格测试。

use super::*;
use crate::{ProviderKind, TimeStandard};
use jiff::ToSpan;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use std::cell::RefCell;

/// 固定偏移回退仍按 6/29 个本地日起点拉开，而不是滚动小时。
#[test]
fn fixed_windows_use_expected_offsets() {
    let observed = 1_760_000_000_000i64;
    let boundaries = WindowBoundaries::for_fixed_offset(observed, 0);
    assert!(boundaries.today <= observed);
    assert_eq!(
        boundaries.seven_days - boundaries.thirty_days,
        23 * 24 * 60 * 60 * 1000
    );
    let _ = WindowBoundaries::for_local_today(observed);
}

/// 用量查询缺省窗口是本周，不得改成概览默认的当天。
#[test]
fn usage_query_default_window_is_this_week() {
    assert_eq!(
        LocalUsageWindow::default_usage_query(),
        LocalUsageWindow::ThisWeek
    );
    assert_eq!(LocalUsageWindow::default(), LocalUsageWindow::Today);
}

/// 上周日期序列长度固定为 7。
#[test]
fn last_week_dates_cover_seven_civil_days() {
    let observed = 1_760_000_000_000i64;
    let dates = local_dates_for_window(LocalUsageWindow::LastWeek, observed)
        .expect("window dates are valid");
    assert_eq!(dates.len(), 7);
}

/// Claude 空 Token 形状不得继承 Codex 的推理字段。
#[test]
fn empty_tokens_for_claude_provider_keeps_unavailable_output_optional() {
    let codex = ProviderKind::RolloutJsonl;
    let claude = ProviderKind::ClaudeTranscriptJsonl;

    let codex_tokens = crate::empty_token_usage_for_provider(codex);
    let claude_tokens = crate::empty_token_usage_for_provider(claude);

    assert_eq!(codex_tokens.cached_input_tokens, Some(0));
    assert_eq!(claude_tokens.cached_input_tokens, Some(0));
    assert!(codex_tokens.reasoning_output_tokens.is_some());
    assert!(claude_tokens.reasoning_output_tokens.is_none());
}

/// 午夜不可表示时回退到当日首个可表示分钟。
#[test]
fn missing_midnight_resolution_uses_first_representable_minute() {
    let date = Date::new(2026, 7, 31).expect("date is valid");
    let gap_end = date.at(0, 30, 15, 0);
    let resolved = first_representable_local_instant(date, |candidate| {
        (candidate >= gap_end).then(|| {
            jiff::tz::Offset::UTC
                .to_timestamp(candidate)
                .expect("candidate is representable")
                .as_millisecond()
        })
    });
    assert!(resolved.is_some());

    let date = Date::new(2026, 7, 31).expect("date is valid");
    let requested = RefCell::new(Vec::new());
    let _ = WindowBoundaries::for_calendar_dates(date, |value| {
        requested.borrow_mut().push(value);
        Some(i64::try_from(requested.borrow().len()).unwrap_or(0))
    });
}

/// 构造系统本地时区下某一民用日期的代表时刻。
fn local_civil_ms(date: Date, hour: i8, minute: i8, second: i8) -> i64 {
    date.at(hour, minute, second, 0)
        .to_zoned(TimeZone::system())
        .expect("civil instant is representable")
        .timestamp()
        .as_millisecond()
}

/// 相对锚点日期前后移动整数本地日。
fn shift_local_date(date: Date, days: i64) -> Date {
    if days >= 0 {
        date.checked_add(days.days()).expect("date shift fits")
    } else {
        date.checked_sub((-days).days()).expect("date shift fits")
    }
}

/// 观测锚定 2026-08-17 本地下午，同一日期的早晚时刻必须同进同出。
#[test]
fn window_membership_uses_civil_dates_not_clock_time() {
    let today = Date::new(2026, 8, 17).expect("anchor date is valid");
    let observed = local_civil_ms(today, 15, 0, 0);
    let yesterday = shift_local_date(today, -1);
    let day_6 = shift_local_date(today, -6);
    let day_7 = shift_local_date(today, -7);
    let day_29 = shift_local_date(today, -29);
    let day_30 = shift_local_date(today, -30);
    let tomorrow = shift_local_date(today, 1);

    let day_before_yesterday = shift_local_date(today, -2);
    let today_early = local_civil_ms(today, 0, 1, 0);
    let today_late = local_civil_ms(today, 23, 59, 0);
    let yesterday_early = local_civil_ms(yesterday, 0, 1, 0);
    let yesterday_late = local_civil_ms(yesterday, 23, 59, 0);
    let day_before_yesterday_late = local_civil_ms(day_before_yesterday, 23, 59, 0);
    let day_6_early = local_civil_ms(day_6, 0, 1, 0);
    let day_6_late = local_civil_ms(day_6, 23, 59, 0);
    let day_7_early = local_civil_ms(day_7, 0, 1, 0);
    let day_7_rolling = local_civil_ms(day_7, 16, 0, 0);
    let day_29_early = local_civil_ms(day_29, 0, 1, 0);
    let day_30_late = local_civil_ms(day_30, 23, 59, 0);
    let tomorrow_early = local_civil_ms(tomorrow, 0, 1, 0);

    assert!(belongs_to_local_window(today_early, LocalUsageWindow::Today, observed).unwrap());
    assert!(belongs_to_local_window(today_late, LocalUsageWindow::Today, observed).unwrap());
    assert!(!belongs_to_local_window(yesterday_late, LocalUsageWindow::Today, observed).unwrap());
    assert!(!belongs_to_local_window(tomorrow_early, LocalUsageWindow::Today, observed).unwrap());

    assert!(
        belongs_to_local_window(yesterday_early, LocalUsageWindow::Yesterday, observed).unwrap()
    );
    assert!(
        belongs_to_local_window(yesterday_late, LocalUsageWindow::Yesterday, observed).unwrap()
    );
    assert!(!belongs_to_local_window(today_early, LocalUsageWindow::Yesterday, observed).unwrap());
    assert!(!belongs_to_local_window(today_late, LocalUsageWindow::Yesterday, observed).unwrap());
    assert!(
        !belongs_to_local_window(
            day_before_yesterday_late,
            LocalUsageWindow::Yesterday,
            observed
        )
        .unwrap()
    );
    assert!(
        !belongs_to_local_window(tomorrow_early, LocalUsageWindow::Yesterday, observed).unwrap()
    );
    let yesterday_dates = local_dates_for_window(LocalUsageWindow::Yesterday, observed).unwrap();
    assert_eq!(yesterday_dates, vec![yesterday]);

    assert!(belongs_to_local_window(today_early, LocalUsageWindow::ThisWeek, observed).unwrap());
    assert!(
        !belongs_to_local_window(yesterday_late, LocalUsageWindow::ThisWeek, observed).unwrap()
    );
    assert!(!belongs_to_local_window(day_6_late, LocalUsageWindow::ThisWeek, observed).unwrap());

    assert!(belongs_to_local_window(yesterday_late, LocalUsageWindow::LastWeek, observed).unwrap());
    assert!(belongs_to_local_window(day_6_early, LocalUsageWindow::LastWeek, observed).unwrap());
    assert!(belongs_to_local_window(day_7_early, LocalUsageWindow::LastWeek, observed).unwrap());
    assert!(!belongs_to_local_window(today_early, LocalUsageWindow::LastWeek, observed).unwrap());
    assert!(
        !belongs_to_local_window(day_7_rolling, LocalUsageWindow::ThisWeek, observed).unwrap(),
        "the prior Monday belongs to last week, not this week"
    );

    assert!(belongs_to_local_window(today_late, LocalUsageWindow::ThisMonth, observed).unwrap());
    assert!(belongs_to_local_window(day_6_late, LocalUsageWindow::ThisMonth, observed).unwrap());
    assert!(!belongs_to_local_window(day_29_early, LocalUsageWindow::ThisMonth, observed).unwrap());
    assert!(belongs_to_local_window(day_29_early, LocalUsageWindow::LastMonth, observed).unwrap());
    assert!(belongs_to_local_window(day_30_late, LocalUsageWindow::LastMonth, observed).unwrap());
    assert!(!belongs_to_local_window(today_early, LocalUsageWindow::LastMonth, observed).unwrap());

    let this_week = local_dates_for_window(LocalUsageWindow::ThisWeek, observed).unwrap();
    assert_eq!(this_week, vec![today]);
    let last_week = local_dates_for_window(LocalUsageWindow::LastWeek, observed).unwrap();
    assert_eq!(last_week.len(), 7);
    assert_eq!(last_week[0], day_7);
    assert_eq!(last_week[6], yesterday);
    assert!(!last_week.contains(&today));
}

/// 构造固定偏移时区下某一民用时刻的 epoch 毫秒。
fn zoned_civil_ms(
    year: i16,
    month: i8,
    day: i8,
    hour: i8,
    minute: i8,
    second: i8,
    zone: &TimeZone,
) -> i64 {
    Date::new(year, month, day)
        .expect("date is valid")
        .at(hour, minute, second, 0)
        .to_zoned(zone.clone())
        .expect("civil instant is representable")
        .timestamp()
        .as_millisecond()
}

/// UTC+8 下同一瞬间在本地与远端可分属不同自然日，窗口下界分别是该时区午夜与 UTC 午夜。
#[test]
fn local_and_remote_standards_split_the_same_instant_across_days() {
    let device_tz = TimeZone::fixed(jiff::tz::offset(8));
    let observed = zoned_civil_ms(2026, 8, 19, 15, 0, 0, &device_tz);
    let local_early = zoned_civil_ms(2026, 8, 19, 1, 0, 0, &device_tz);
    let local_late = zoned_civil_ms(2026, 8, 19, 23, 59, 0, &device_tz);
    let local_yesterday_late = zoned_civil_ms(2026, 8, 18, 23, 59, 0, &device_tz);

    assert!(
        belongs_to_window(
            local_early,
            LocalUsageWindow::Today,
            observed,
            &TimeStandard::Local,
            &device_tz,
        )
        .unwrap()
    );
    assert!(
        !belongs_to_window(
            local_early,
            LocalUsageWindow::Today,
            observed,
            &TimeStandard::utc(),
            &device_tz,
        )
        .unwrap(),
        "01:00 UTC+8 is still 17:00 previous UTC day"
    );
    assert!(
        belongs_to_window(
            local_late,
            LocalUsageWindow::Today,
            observed,
            &TimeStandard::Local,
            &device_tz,
        )
        .unwrap()
    );
    assert!(
        belongs_to_window(
            local_late,
            LocalUsageWindow::Today,
            observed,
            &TimeStandard::utc(),
            &device_tz,
        )
        .unwrap()
    );
    assert!(
        !belongs_to_window(
            local_yesterday_late,
            LocalUsageWindow::Today,
            observed,
            &TimeStandard::Local,
            &device_tz,
        )
        .unwrap()
    );

    let local_bounds = WindowBoundaries::for_standard(observed, &TimeStandard::Local, &device_tz);
    let remote_bounds = WindowBoundaries::for_standard(observed, &TimeStandard::utc(), &device_tz);
    assert_eq!(
        TimeStandard::utc().viewing_time_zone(&device_tz),
        TimeZone::UTC
    );
    assert_ne!(TimeStandard::utc().viewing_time_zone(&device_tz), device_tz);
    assert_eq!(
        local_bounds.today,
        zoned_civil_ms(2026, 8, 19, 0, 0, 0, &device_tz)
    );
    assert_eq!(
        remote_bounds.today,
        zoned_civil_ms(2026, 8, 19, 0, 0, 0, &TimeZone::UTC)
    );
    assert_eq!(
        local_bounds.seven_days,
        zoned_civil_ms(2026, 8, 13, 0, 0, 0, &device_tz)
    );
    assert_eq!(
        remote_bounds.seven_days,
        zoned_civil_ms(2026, 8, 13, 0, 0, 0, &TimeZone::UTC)
    );

    let remote_dates = dates_for_window(
        LocalUsageWindow::Today,
        observed,
        &TimeStandard::utc(),
        &device_tz,
    )
    .unwrap();
    assert_eq!(remote_dates, vec![Date::new(2026, 8, 19).unwrap()]);
    let yesterday = dates_for_window(
        LocalUsageWindow::Yesterday,
        observed,
        &TimeStandard::Local,
        &device_tz,
    )
    .unwrap();
    assert_eq!(yesterday, vec![Date::new(2026, 8, 18).unwrap()]);
    let remote_yesterday = dates_for_window(
        LocalUsageWindow::Yesterday,
        observed,
        &TimeStandard::utc(),
        &device_tz,
    )
    .unwrap();
    assert_eq!(remote_yesterday, vec![Date::new(2026, 8, 18).unwrap()]);
    assert!(
        belongs_to_window(
            local_yesterday_late,
            LocalUsageWindow::Yesterday,
            observed,
            &TimeStandard::Local,
            &device_tz,
        )
        .unwrap()
    );
    assert!(
        !belongs_to_window(
            local_early,
            LocalUsageWindow::Yesterday,
            observed,
            &TimeStandard::Local,
            &device_tz,
        )
        .unwrap()
    );
    let two_day_count = dates_for_day_count(2, observed, &TimeStandard::Local, &device_tz).unwrap();
    assert_eq!(
        two_day_count,
        vec![
            Date::new(2026, 8, 18).unwrap(),
            Date::new(2026, 8, 19).unwrap()
        ]
    );
}

/// 90 天保留下界是观测日往前第 89 个所选标准日的起点，早于固定 30 日下界。
#[test]
fn ninety_day_retention_cutoff_is_earlier_than_thirty_day_window() {
    let device_tz = TimeZone::fixed(jiff::tz::offset(8));
    let observed = zoned_civil_ms(2026, 8, 19, 15, 0, 0, &device_tz);
    let days = crate::RetentionDays::new(90).expect("90 days is approved");
    let cutoff = retention_cutoff_epoch_ms(days, observed, &TimeStandard::Local, &device_tz)
        .expect("cutoff is representable");
    assert_eq!(cutoff, zoned_civil_ms(2026, 5, 22, 0, 0, 0, &device_tz));
    let thirty =
        WindowBoundaries::for_standard(observed, &TimeStandard::Local, &device_tz).thirty_days;
    assert!(cutoff < thirty);
    assert_eq!(thirty, zoned_civil_ms(2026, 7, 21, 0, 0, 0, &device_tz));
}

/// 同一瞬间在当地时间 UTC+8 与 UTC 时间可落入不同民用日；自定义 IANA 不得再生效。
#[test]
fn local_device_and_custom_zone_split_the_same_instant_across_days() {
    let device_tz = TimeZone::fixed(jiff::tz::offset(8));
    let custom = TimeStandard::resolve(
        crate::TimeStandardMode::Custom,
        Some("America/Los_Angeles"),
        "UTC",
    );
    assert_eq!(custom, TimeStandard::utc());
    assert_eq!(custom.viewing_time_zone(&device_tz), TimeZone::UTC);
    let observed = zoned_civil_ms(2026, 8, 19, 15, 0, 0, &device_tz);
    let local_early = zoned_civil_ms(2026, 8, 19, 1, 0, 0, &device_tz);

    assert!(
        belongs_to_window(
            local_early,
            LocalUsageWindow::Today,
            observed,
            &TimeStandard::Local,
            &device_tz,
        )
        .unwrap()
    );
    assert!(
        !belongs_to_window(
            local_early,
            LocalUsageWindow::Today,
            observed,
            &custom,
            &device_tz,
        )
        .unwrap(),
        "01:00 UTC+8 is still the previous UTC civil day"
    );
    let local_today =
        WindowBoundaries::for_standard(observed, &TimeStandard::Local, &device_tz).today;
    let custom_today = WindowBoundaries::for_standard(observed, &custom, &device_tz).today;
    assert_eq!(
        local_today,
        zoned_civil_ms(2026, 8, 19, 0, 0, 0, &device_tz)
    );
    assert_eq!(
        custom_today,
        zoned_civil_ms(2026, 8, 19, 0, 0, 0, &TimeZone::UTC)
    );
    assert_ne!(local_today, custom_today);
}

/// 观测为当日 15:00 时当地 23:59:59 仍属当地当天；UTC 下界是 UTC 午夜，同一瞬间按 UTC 民用日判定。
#[test]
fn today_includes_civil_end_after_observation_and_utc_uses_utc_midnight() {
    let device_tz = TimeZone::fixed(jiff::tz::offset(8));
    let observed = zoned_civil_ms(2026, 8, 19, 15, 0, 0, &device_tz);
    let local_end = zoned_civil_ms(2026, 8, 19, 23, 59, 59, &device_tz);
    let utc_end = zoned_civil_ms(2026, 8, 19, 23, 59, 59, &TimeZone::UTC);

    assert!(
        belongs_to_window(
            local_end,
            LocalUsageWindow::Today,
            observed,
            &TimeStandard::Local,
            &device_tz,
        )
        .unwrap(),
        "15:00 observation must not cap local today before 23:59:59"
    );
    assert!(
        belongs_to_window(
            local_end,
            LocalUsageWindow::Today,
            observed,
            &TimeStandard::utc(),
            &device_tz,
        )
        .unwrap(),
        "23:59:59 UTC+8 is still 15:59:59 UTC on the same UTC civil day"
    );
    assert!(
        !belongs_to_window(
            utc_end,
            LocalUsageWindow::Today,
            observed,
            &TimeStandard::Local,
            &device_tz,
        )
        .unwrap(),
        "23:59:59 UTC is already the next UTC+8 civil morning"
    );
    assert!(
        belongs_to_window(
            utc_end,
            LocalUsageWindow::Today,
            observed,
            &TimeStandard::utc(),
            &device_tz,
        )
        .unwrap(),
        "UTC today still includes 23:59:59 UTC after a 07:00 UTC observation"
    );

    let local_bounds = WindowBoundaries::for_standard(observed, &TimeStandard::Local, &device_tz);
    let utc_bounds = WindowBoundaries::for_standard(observed, &TimeStandard::utc(), &device_tz);
    assert_eq!(
        local_bounds.today,
        zoned_civil_ms(2026, 8, 19, 0, 0, 0, &device_tz)
    );
    assert_eq!(
        utc_bounds.today,
        zoned_civil_ms(2026, 8, 19, 0, 0, 0, &TimeZone::UTC)
    );
    assert_ne!(
        utc_bounds.today, local_bounds.today,
        "UTC window start must not be the device-timezone midnight"
    );
    assert_eq!(
        utc_bounds.seven_days,
        zoned_civil_ms(2026, 8, 13, 0, 0, 0, &TimeZone::UTC)
    );
    assert_eq!(
        utc_bounds.thirty_days,
        zoned_civil_ms(2026, 7, 21, 0, 0, 0, &TimeZone::UTC)
    );
}
