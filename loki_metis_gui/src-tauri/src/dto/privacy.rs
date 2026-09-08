//! 本机设置页需要的最小设置，以及当地时区/UTC 民用日划分标准。

use serde::{Deserialize, Serialize};

use super::{IndexLocationCodeDto, LanguagePreferenceDto, UsageClientKindDto};

/// 看板从统一 AI 目录取得的一个可用选择项。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableAiTypeDto {
    /// 跨区域一致的展示名称。
    pub name: String,
    /// 看板现有设置与路由使用的稳定 wire 值。
    pub value: String,
}

/// 描述本机用量页需要的最小设置。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacySettingsDto {
    /// 当前持久语言偏好；系统语言解析只影响界面展示。
    pub language_preference: LanguagePreferenceDto,
    /// 本机周期扫描使用的间隔分钟数。
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
    /// 用户显式开放监控与本机统计的 Agent；缺省为空。
    pub enabled_agents: Vec<UsageClientKindDto>,
    /// 统一目录中当前可由看板识别的 AI 类型；无法映射的类型已被忽略。
    pub available_ai_types: Vec<AvailableAiTypeDto>,
    /// 用户是否显式开放读取 WorkBuddy 本地用量统计；缺省关闭。
    pub workbuddy_stats_enabled: bool,
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
