//! 扫描间隔、Agent 选择与设置路径共用规则。

mod agent_label;
mod retention_days;
mod scan_interval;
mod usage;
pub use usage::{UsageSettings,UsageSettingField};

pub use agent_label::agent_wire_label;

pub use retention_days::{
    DEFAULT_RETENTION_DAYS, MAX_RETENTION_DAYS, MIN_RETENTION_DAYS, RetentionDays,
    RetentionDaysError,
};
pub use scan_interval::{
    DEFAULT_SCAN_INTERVAL_MINUTES, MAX_SCAN_INTERVAL_MINUTES, MIN_SCAN_INTERVAL_MINUTES,
    ScanIntervalError, ScanIntervalMinutes,
};
