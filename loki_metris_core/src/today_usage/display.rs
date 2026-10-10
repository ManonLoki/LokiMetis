//! 所有接口共用今日展示资格，缺失、失败和过期不伪装为可信零。

use super::{MAX_OBSERVATION_AGE_MS, TodayAvailability, TodayUsageSnapshot, UsageDay};
use crate::{Completeness, Confidence};

/// 展示投影只携带数值与质量，不复制来源字符串或通知账本。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TodayUsageDisplay {
    /// 首次读取或来源变化后等待当前摘要。
    #[default]
    Pending,
    /// 聚合尚无可解释记录，不能把占位零当可信零。
    NoRecords,
    /// 本轮摘要读取失败或已启用来源不可用。
    Unavailable,
    /// 旧日、未来观测时间或超过统一新鲜度上限。
    Expired,
    /// 可用值保持整数精度，并披露部分、推算或疑似质量。
    Value {
        /// 当前本地日的聚合 Token 整数。
        total_tokens: u64,
        /// 来源覆盖的完整程度。
        completeness: Completeness,
        /// 总量的可信程度。
        confidence: Confidence,
    },
}

/// 来源代次由宿主核对；日界和新鲜度独立于通知是否开启及账本是否存在。
pub fn project_today_usage_display(
    snapshot: Option<&TodayUsageSnapshot>,
    now_ms: i64,
    current_day: Option<&UsageDay>,
    source_current: bool,
    read_failed: bool,
) -> TodayUsageDisplay {
    if !source_current {
        return TodayUsageDisplay::Pending;
    }
    if read_failed {
        return TodayUsageDisplay::Unavailable;
    }
    let Some(snapshot) = snapshot else {
        return TodayUsageDisplay::Pending;
    };
    if current_day.is_none() {
        return TodayUsageDisplay::Unavailable;
    }
    if current_day != Some(&snapshot.day)
        || !snapshot.day.contains(now_ms)
        || !(0..=MAX_OBSERVATION_AGE_MS).contains(&now_ms.saturating_sub(snapshot.observed_ms))
    {
        return TodayUsageDisplay::Expired;
    }
    match snapshot.availability {
        TodayAvailability::NoRecords => TodayUsageDisplay::NoRecords,
        TodayAvailability::Unavailable => TodayUsageDisplay::Unavailable,
        TodayAvailability::Available => TodayUsageDisplay::Value {
            total_tokens: snapshot.total_tokens,
            completeness: snapshot.completeness,
            confidence: snapshot.confidence,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UsageDay;

    /// 使用当前日真边界，零值与质量属于事实而非格式化字符串。
    fn snapshot(now: i64) -> TodayUsageSnapshot {
        TodayUsageSnapshot {
            day: UsageDay::capture(now, &jiff::tz::TimeZone::UTC).unwrap(),
            source: "all".into(),
            total_tokens: 0,
            observed_ms: now,
            availability: TodayAvailability::Available,
            completeness: Completeness::Complete,
            confidence: Confidence::Exact,
        }
    }

    /// 可信零、无记录、读取失败和来源重置分别表达，旧值不渗透新来源。
    #[test]
    fn zero_missing_failure_and_source_reset_remain_distinct() {
        let now = 1_790_000_000_000;
        let mut value = snapshot(now);
        assert!(matches!(
            project_today_usage_display(Some(&value), now, Some(&value.day), true, false),
            TodayUsageDisplay::Value {
                total_tokens: 0,
                ..
            }
        ));
        value.availability = TodayAvailability::NoRecords;
        assert_eq!(
            project_today_usage_display(Some(&value), now, Some(&value.day), true, false),
            TodayUsageDisplay::NoRecords
        );
        assert_eq!(
            project_today_usage_display(Some(&value), now, Some(&value.day), true, true),
            TodayUsageDisplay::Unavailable
        );
        assert_eq!(
            project_today_usage_display(Some(&value), now, Some(&value.day), false, false),
            TodayUsageDisplay::Pending
        );
        assert_eq!(
            project_today_usage_display(None, now, Some(&value.day), true, false),
            TodayUsageDisplay::Pending
        );
        assert_eq!(
            project_today_usage_display(None, now, Some(&value.day), true, true),
            TodayUsageDisplay::Unavailable
        );
    }

    /// 午夜边界立即撤下旧值，不等待后台轮询；新鲜度端点与未来观测也严格区分。
    #[test]
    fn midnight_age_and_future_observation_withdraw_old_values() {
        let now = 1_790_000_000_000;
        let mut value = snapshot(now);
        assert!(matches!(
            project_today_usage_display(Some(&value), now + 90_000, Some(&value.day), true, false),
            TodayUsageDisplay::Value { .. }
        ));
        assert_eq!(
            project_today_usage_display(Some(&value), now + 90_001, Some(&value.day), true, false),
            TodayUsageDisplay::Expired
        );
        assert_eq!(
            project_today_usage_display(Some(&value), now - 1, Some(&value.day), true, false),
            TodayUsageDisplay::Expired
        );
        value.observed_ms = value.day.end_ms - 1;
        assert!(matches!(
            project_today_usage_display(
                Some(&value),
                value.day.end_ms - 1,
                Some(&value.day),
                true,
                false
            ),
            TodayUsageDisplay::Value { .. }
        ));
        assert_eq!(
            project_today_usage_display(
                Some(&value),
                value.day.end_ms,
                Some(&value.day),
                true,
                false
            ),
            TodayUsageDisplay::Expired
        );
    }

    /// 非精确或部分值可披露质量，不能升级成通知资格或丢失大整数精度。
    #[test]
    fn partial_and_derived_values_preserve_quality_and_integer() {
        let now = 1_790_000_000_000;
        let mut value = snapshot(now);
        value.total_tokens = u64::MAX;
        value.completeness = Completeness::Partial;
        value.confidence = Confidence::Derived;
        assert_eq!(
            project_today_usage_display(Some(&value), now, Some(&value.day), true, false),
            TodayUsageDisplay::Value {
                total_tokens: u64::MAX,
                completeness: Completeness::Partial,
                confidence: Confidence::Derived
            }
        );
        assert!(!value.eligible(now, &value.day, &value.source));
    }

    /// 同一瞬间落在两个时区的日区间内，仍须核对完整日身份。
    #[test]
    fn overlapping_timezone_days_and_missing_day_withdraw_old_value() {
        let now = 1_790_000_000_000;
        let value = snapshot(now);
        let other =
            UsageDay::capture(now, &jiff::tz::TimeZone::get("Asia/Shanghai").unwrap()).unwrap();
        assert!(value.day.contains(now) && other.contains(now));
        assert_ne!(value.day, other);
        assert_eq!(
            project_today_usage_display(Some(&value), now, Some(&other), true, false),
            TodayUsageDisplay::Expired
        );
        assert_eq!(
            project_today_usage_display(Some(&value), now, None, true, false),
            TodayUsageDisplay::Unavailable
        );
    }
}
