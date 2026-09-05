//! 定义本机周期扫描与保留远端偏好共用的单一扫描间隔。

use crate::bounded_minutes::bounded_minutes_newtype;

/// 扫描间隔的默认分钟数；新安装与缺失值都使用它。
pub const DEFAULT_SCAN_INTERVAL_MINUTES: u16 = 5;
/// 用户可设置的最小扫描间隔，单位为分钟。
pub const MIN_SCAN_INTERVAL_MINUTES: u16 = 1;
/// 用户可设置的最大扫描间隔，单位为分钟。
pub const MAX_SCAN_INTERVAL_MINUTES: u16 = 1_440;

bounded_minutes_newtype! {
    /// 保存经过边界校验的扫描间隔分钟数。本机周期快速扫描与仍保留的远端
    /// 自动读取偏好必须读取同一值，不能再各持独立默认或独立分钟数。
    struct ScanIntervalMinutes;
    min = MIN_SCAN_INTERVAL_MINUTES;
    max = MAX_SCAN_INTERVAL_MINUTES;
    default = DEFAULT_SCAN_INTERVAL_MINUTES;
    /// 表示扫描间隔超出批准的分钟范围。
    error ScanIntervalError = "scan interval is out of range";
}

impl ScanIntervalMinutes {
    /// 返回持久规范化快照使用的毫秒 TTL。
    pub const fn milliseconds(self) -> i64 {
        self.get() as i64 * 60 * 1_000
    }
}

/// 把新字段与旧双字段折叠为单一扫描间隔。
///
/// 选择顺序：新 `scan` 字段优先；否则只用本机字段；否则只用远端字段；
/// 两者都在且不同时取本机值（当前实际在跑的节奏）；都缺失则默认 5。
/// 任一出现的字段越界都会失败，避免坏值被默认值吞掉。
pub fn resolve_scan_interval_minutes(
    scan: Option<u16>,
    local: Option<u16>,
    remote: Option<u16>,
) -> Result<ScanIntervalMinutes, ScanIntervalError> {
    for minutes in [scan, local, remote].into_iter().flatten() {
        ScanIntervalMinutes::new(minutes)?;
    }
    let chosen = match (scan, local, remote) {
        (Some(minutes), _, _) => minutes,
        (None, Some(minutes), _) => minutes,
        (None, None, Some(minutes)) => minutes,
        (None, None, None) => return Ok(ScanIntervalMinutes::default()),
    };
    ScanIntervalMinutes::new(chosen)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// 验证缺省/新值为 5，且 1 与 1440 都能换成调度时长。
    #[test]
    fn defaults_to_five_and_accepts_approved_boundaries() {
        assert_eq!(ScanIntervalMinutes::default().get(), 5);
        assert_eq!(DEFAULT_SCAN_INTERVAL_MINUTES, 5);
        assert_eq!(
            ScanIntervalMinutes::new(1).unwrap().duration(),
            Duration::from_secs(60)
        );
        assert_eq!(
            ScanIntervalMinutes::new(1_440).unwrap().milliseconds(),
            86_400_000
        );
    }

    /// 验证 0 与 1441 都被拒绝。
    #[test]
    fn rejects_out_of_range_minutes() {
        assert_eq!(ScanIntervalMinutes::new(0), Err(ScanIntervalError));
        assert_eq!(ScanIntervalMinutes::new(1_441), Err(ScanIntervalError));
    }

    /// 验证折叠优先使用新字段，缺失时才看旧双字段。
    #[test]
    fn prefers_explicit_scan_field_over_legacy_pairs() {
        assert_eq!(
            resolve_scan_interval_minutes(Some(9), Some(3), Some(15))
                .unwrap()
                .get(),
            9
        );
        assert_eq!(
            resolve_scan_interval_minutes(None, Some(3), Some(15))
                .unwrap()
                .get(),
            3
        );
        assert_eq!(
            resolve_scan_interval_minutes(None, None, Some(15))
                .unwrap()
                .get(),
            15
        );
        assert_eq!(
            resolve_scan_interval_minutes(None, None, None)
                .unwrap()
                .get(),
            5
        );
    }

    /// 验证旧双字段不同时只生效本机扫描分钟，不留下第二套可独立生效的值。
    #[test]
    fn folds_conflicting_legacy_minutes_to_the_local_value() {
        assert_eq!(
            resolve_scan_interval_minutes(None, Some(1), Some(2))
                .unwrap()
                .get(),
            1
        );
        assert_eq!(
            resolve_scan_interval_minutes(None, Some(10), Some(10))
                .unwrap()
                .get(),
            10
        );
    }

    /// 验证任一出现的越界字段都会拒绝，不能回退成默认 5。
    #[test]
    fn rejects_out_of_range_legacy_or_scan_fields() {
        assert_eq!(
            resolve_scan_interval_minutes(Some(0), None, None),
            Err(ScanIntervalError)
        );
        assert_eq!(
            resolve_scan_interval_minutes(None, Some(1_441), Some(5)),
            Err(ScanIntervalError)
        );
        assert_eq!(
            resolve_scan_interval_minutes(None, None, Some(0)),
            Err(ScanIntervalError)
        );
    }
}
