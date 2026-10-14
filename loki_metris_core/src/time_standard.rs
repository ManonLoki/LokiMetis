//! 查看用自然日划分标准：当地时间（设备时区）或固定 UTC。

use jiff::tz::TimeZone;

/// 划分本机统计自然日所用的时间标准。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TimeStandard {
    /// 按设备当前时区的民用日划分。
    #[default]
    Local,
    /// 按 UTC 民用日划分；`time_zone` 只保留磁盘兼容字段，查看始终用 UTC。
    Custom {
        /// 兼容已保存自定义字段；加载与解析后固定为 `UTC`。
        time_zone: String,
    },
}

impl TimeStandard {

    /// UTC 时间；旧 `remote` 与旧非 UTC 自定义 IANA 都迁移到此值。
    pub fn utc() -> Self {
        Self::Custom {
            time_zone: "UTC".to_owned(),
        }
    }

    /// 划分民用日时使用的时区：当地时间用设备时区，UTC 模式固定 `TimeZone::UTC`。
    pub fn viewing_time_zone(&self, device_tz: &TimeZone) -> TimeZone {
        match self {
            Self::Local => device_tz.clone(),
            Self::Custom { .. } => TimeZone::UTC,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UTC 模式划分民用日始终使用 UTC，当地模式沿用设备时区。
    #[test]
    fn viewing_time_zone_follows_standard() {
        let device_tz = TimeZone::fixed(jiff::tz::offset(8));
        assert_eq!(TimeStandard::utc().viewing_time_zone(&device_tz), TimeZone::UTC);
        assert_ne!(TimeStandard::utc().viewing_time_zone(&device_tz), device_tz);
        assert_eq!(TimeStandard::Local.viewing_time_zone(&device_tz), device_tz);
    }
}
