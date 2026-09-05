//! 查看用自然日划分标准：当地时间（设备时区）或固定 UTC。

use jiff::tz::TimeZone;

/// 看板页头可选的两种查看模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeStandardMode {
    /// 按当前设备时区民用日划分。
    Local,
    /// 按 UTC 民用日划分；磁盘标签仍为 `custom`。
    Custom,
}

/// 划分自然日所用的时间标准；Collect 上报不读取该值。
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
    /// 当地时间（设备时区）模式。
    pub const fn local() -> Self {
        Self::Local
    }

    /// UTC 时间；旧 `remote` 与旧非 UTC 自定义 IANA 都迁移到此值。
    pub fn utc() -> Self {
        Self::Custom {
            time_zone: "UTC".to_owned(),
        }
    }

    /// 按用户选择解析查看标准：当地时间忽略时区名；自定义一律写成 UTC。
    pub fn resolve(
        mode: TimeStandardMode,
        _custom_time_zone: Option<&str>,
        _device_time_zone: &str,
    ) -> Self {
        match mode {
            TimeStandardMode::Local => Self::Local,
            TimeStandardMode::Custom => Self::utc(),
        }
    }

    /// 从磁盘标签恢复：`remote` 与任意已保存自定义名都视为 UTC。
    pub fn from_stored_parts(
        label: Option<&str>,
        _custom_time_zone: Option<&str>,
        _device_time_zone: &str,
    ) -> Self {
        match label.unwrap_or("local") {
            "remote" | "custom" => Self::utc(),
            _ => Self::Local,
        }
    }

    /// 写回磁盘的模式标签；旧 `remote` 不再写出。
    pub fn stored_label(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Custom { .. } => "custom",
        }
    }

    /// UTC 模式才写出时区名；当地时间不保存时区字段。
    pub fn stored_custom_time_zone(&self) -> Option<&str> {
        match self {
            Self::Local => None,
            Self::Custom { .. } => Some("UTC"),
        }
    }

    /// 页头分段控件对应的模式。
    pub fn mode(&self) -> TimeStandardMode {
        match self {
            Self::Local => TimeStandardMode::Local,
            Self::Custom { .. } => TimeStandardMode::Custom,
        }
    }

    /// UTC 模式的时区名；当地时间模式为空。
    pub fn custom_time_zone_name(&self) -> Option<&str> {
        self.stored_custom_time_zone()
    }

    /// 划分民用日时使用的时区：当地时间用设备时区，UTC 模式固定 `TimeZone::UTC`。
    pub fn viewing_time_zone(&self, device_tz: &TimeZone) -> TimeZone {
        match self {
            Self::Local => device_tz.clone(),
            Self::Custom { .. } => TimeZone::UTC,
        }
    }
}

/// 返回当前设备 IANA 时区名；系统时区没有 IANA 名时回退 `UTC`。
pub fn device_time_zone_name() -> String {
    TimeZone::system()
        .iana_name()
        .filter(|name| is_known_iana_time_zone(name))
        .unwrap_or("UTC")
        .to_owned()
}

/// 判定字符串是否为 jiff 可解析的 IANA 时区。
pub fn is_known_iana_time_zone(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty() && TimeZone::get(trimmed).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 旧 remote 必须变成 UTC，不得改回设备时区。
    #[test]
    fn legacy_remote_becomes_custom_utc() {
        let loaded = TimeStandard::from_stored_parts(Some("remote"), None, "Asia/Shanghai");
        assert_eq!(loaded.mode(), TimeStandardMode::Custom);
        assert_eq!(loaded.custom_time_zone_name(), Some("UTC"));
        assert_eq!(loaded.viewing_time_zone(&TimeZone::UTC), TimeZone::UTC);
    }

    /// 已保存非 UTC 自定义 IANA 加载后必须是 UTC，查看也不得继续用该 IANA。
    #[test]
    fn legacy_custom_non_utc_iana_loads_as_utc() {
        let loaded = TimeStandard::from_stored_parts(
            Some("custom"),
            Some("Asia/Shanghai"),
            "America/New_York",
        );
        assert_eq!(loaded, TimeStandard::utc());
        assert_eq!(loaded.custom_time_zone_name(), Some("UTC"));
        let device_tz = TimeZone::get("Asia/Shanghai").expect("IANA zone exists");
        assert_eq!(loaded.viewing_time_zone(&device_tz), TimeZone::UTC);
    }

    /// 选择自定义一律写成 UTC；当地时间忽略传入的时区名。
    #[test]
    fn custom_always_resolves_to_utc_and_local_ignores_name() {
        let custom_named =
            TimeStandard::resolve(TimeStandardMode::Custom, Some("America/New_York"), "UTC");
        assert_eq!(custom_named, TimeStandard::utc());
        let custom_empty =
            TimeStandard::resolve(TimeStandardMode::Custom, None, "America/New_York");
        assert_eq!(custom_empty, TimeStandard::utc());
        let local = TimeStandard::resolve(
            TimeStandardMode::Local,
            Some("America/New_York"),
            "Asia/Shanghai",
        );
        assert_eq!(local, TimeStandard::Local);
        assert_eq!(local.stored_custom_time_zone(), None);
        let device_tz = TimeZone::fixed(jiff::tz::offset(8));
        assert_eq!(
            TimeStandard::utc().viewing_time_zone(&device_tz),
            TimeZone::UTC
        );
        assert_ne!(TimeStandard::utc().viewing_time_zone(&device_tz), device_tz);
    }
}
