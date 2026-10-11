// 本文件是 timeline_tests.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：观测为当日 15:00 时当地 23:59:59 仍属当地当天；UTC 下界是 UTC 午夜，同一瞬间按 UTC 民用日判定。；下一自然日起点必须按民用日推进，不能把“明天”简化为固定增加 24 小时。

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

/// 下一自然日起点必须按民用日推进，不能把“明天”简化为固定增加 24 小时。
#[test]
fn next_day_start_respects_the_selected_time_zone() {
    let device_tz = TimeZone::fixed(jiff::tz::offset(8));
    let observed = zoned_civil_ms(2026, 8, 19, 15, 0, 0, &device_tz);

    assert_eq!(
        next_day_start_epoch_ms(observed, &TimeStandard::Local, &device_tz),
        Some(zoned_civil_ms(2026, 8, 20, 0, 0, 0, &device_tz))
    );
    assert_eq!(
        next_day_start_epoch_ms(observed, &TimeStandard::utc(), &device_tz),
        Some(zoned_civil_ms(2026, 8, 20, 0, 0, 0, &TimeZone::UTC))
    );

    let new_york = TimeZone::get("America/New_York").expect("IANA fixture exists");
    let dst_observed = zoned_civil_ms(2026, 3, 8, 0, 0, 0, &new_york);
    let dst_next_day = next_day_start_epoch_ms(dst_observed, &TimeStandard::Local, &new_york)
        .expect("next civil day starts");
    assert_eq!(dst_next_day, zoned_civil_ms(2026, 3, 9, 0, 0, 0, &new_york));
    assert_eq!(dst_next_day - dst_observed, 23 * 60 * 60 * 1_000);
}
