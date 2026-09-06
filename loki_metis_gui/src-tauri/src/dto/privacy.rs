//! 隐私页需要的最小本地设置，以及查看时区/UTC 民用日划分标准。

use serde::{Deserialize, Serialize};

use super::{IndexLocationCodeDto, LanguagePreferenceDto, UsageClientKindDto};

/// 描述隐私页需要的最小本地设置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacySettingsDto {
    /// 当前持久语言偏好；系统语言解析只影响界面展示。
    pub language_preference: LanguagePreferenceDto,
    /// 保留的仅本机偏好；ADR-107 已永久移出官方读取，该字段任一值都不再启用它。
    pub local_only: bool,
    /// 首次从系统会话候选初始化且可由用户修改或清除的标签；不表示唯一身份。
    pub device_username: Option<String>,
    /// 只读 OS 主机名快照；不可由用户修改。
    pub device_name: Option<String>,
    /// 已持久化则原样保留的只读设备唯一 ID；缺失时优先稳定设备 ID，否则生成。
    pub device_unique_id: Option<String>,
    /// 本机周期扫描与保留远端偏好共用的扫描间隔分钟数。
    pub scan_interval_minutes: u16,
    /// 派生用量自动清理天数；缺省 90。
    pub retention_days: u16,
    /// 本产品索引的安全位置说明，不包含绝对路径。
    pub index_location_label: String,
    /// 索引位置的稳定本地化代码。
    pub index_location_code: IndexLocationCodeDto,
    /// 当前索引体积；未知时为空。
    pub index_size_bytes: Option<u64>,
    /// 最近一次清空本产品索引的 Unix 毫秒时间戳。
    pub last_cleared_at_epoch_ms: Option<i64>,
    /// 用户显式开放监控和上报的本机 Agent；缺省为空。
    pub enabled_agents: Vec<UsageClientKindDto>,
    /// 用户是否显式开放读取 WorkBuddy 本地用量统计；缺省关闭。
    pub workbuddy_stats_enabled: bool,
    /// 当前设备 IANA 时区，供 Collect 信封使用，不持久化为用户选择。
    pub device_time_zone: String,
}

/// 看板页头可选的自然日划分标准。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeStandardDto {
    /// 当地时间或 UTC 时间。
    pub mode: TimeStandardModeDto,
    /// UTC 模式固定为 `UTC`；当地时间模式为空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_time_zone: Option<String>,
}

/// 时间标准模式的 IPC 取值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TimeStandardModeDto {
    /// 按设备当前时区民用日。
    Local,
    /// 按 UTC 民用日。
    Custom,
}

impl Default for TimeStandardDto {
    /// 新进程页面状态固定从设备当地民用日开始。
    fn default() -> Self {
        Self {
            mode: TimeStandardModeDto::Local,
            custom_time_zone: None,
        }
    }
}

impl From<loki_metis_core::TimeStandard> for TimeStandardDto {
    /// 将 core 时间标准映射为前端只读模式枚举。
    fn from(value: loki_metis_core::TimeStandard) -> Self {
        Self {
            mode: match value.mode() {
                loki_metis_core::TimeStandardMode::Local => TimeStandardModeDto::Local,
                loki_metis_core::TimeStandardMode::Custom => TimeStandardModeDto::Custom,
            },
            custom_time_zone: value.custom_time_zone_name().map(str::to_owned),
        }
    }
}

impl From<TimeStandardModeDto> for loki_metis_core::TimeStandardMode {
    /// 将前端时间标准模式映射为 core 领域值。
    fn from(value: TimeStandardModeDto) -> Self {
        match value {
            TimeStandardModeDto::Local => Self::Local,
            TimeStandardModeDto::Custom => Self::Custom,
        }
    }
}

impl TimeStandardDto {
    /// 把 IPC 输入解析成 core 标准；自定义一律写成 UTC。
    pub fn into_time_standard(self, device_time_zone: &str) -> loki_metis_core::TimeStandard {
        loki_metis_core::TimeStandard::resolve(
            self.mode.into(),
            self.custom_time_zone.as_deref(),
            device_time_zone,
        )
    }
}
