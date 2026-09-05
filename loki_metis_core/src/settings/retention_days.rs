//! 用户可设置的派生用量自动清理窗口，单位为自然日。

/// 新安装与旧文件缺字段使用的默认保留天数。
pub const DEFAULT_RETENTION_DAYS: u16 = 90;
/// 用户可设置的最小保留天数。
pub const MIN_RETENTION_DAYS: u16 = 1;
/// 用户可设置的最大保留天数。
pub const MAX_RETENTION_DAYS: u16 = 3_650;

/// 经过边界校验的派生用量保留天数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionDays(u16);

/// 表示保留天数超出批准范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("retention days are out of range")]
pub struct RetentionDaysError;

impl RetentionDays {
    /// 校验天数位于批准范围。
    pub const fn new(days: u16) -> Result<Self, RetentionDaysError> {
        if days < MIN_RETENTION_DAYS || days > MAX_RETENTION_DAYS {
            return Err(RetentionDaysError);
        }
        Ok(Self(days))
    }

    /// 返回用于持久化与界面展示的整数天数。
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl Default for RetentionDays {
    /// 返回批准的默认 90 天。
    fn default() -> Self {
        Self(DEFAULT_RETENTION_DAYS)
    }
}

/// 缺字段视为默认 90 天；出现越界值则拒绝，避免坏值被默认吞掉。
pub fn resolve_retention_days(days: Option<u16>) -> Result<RetentionDays, RetentionDaysError> {
    match days {
        None => Ok(RetentionDays::default()),
        Some(days) => RetentionDays::new(days),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 缺省为 90，边界 1 与 3650 合法。
    #[test]
    fn defaults_to_ninety_and_accepts_approved_boundaries() {
        assert_eq!(RetentionDays::default().get(), 90);
        assert_eq!(DEFAULT_RETENTION_DAYS, 90);
        assert_eq!(RetentionDays::new(1).unwrap().get(), 1);
        assert_eq!(RetentionDays::new(3_650).unwrap().get(), 3_650);
        assert_eq!(resolve_retention_days(None).unwrap().get(), 90);
        assert_eq!(resolve_retention_days(Some(120)).unwrap().get(), 120);
    }

    /// 0 与超过上限都必须拒绝。
    #[test]
    fn rejects_out_of_range_days() {
        assert_eq!(RetentionDays::new(0), Err(RetentionDaysError));
        assert_eq!(RetentionDays::new(3_651), Err(RetentionDaysError));
        assert_eq!(resolve_retention_days(Some(0)), Err(RetentionDaysError));
        assert_eq!(resolve_retention_days(Some(3_651)), Err(RetentionDaysError));
    }
}
