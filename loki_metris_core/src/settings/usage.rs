//! 用量设置的全局权威值及单字段变更；宿主只负责保存已校验快照。

use super::{RetentionDays,ScanIntervalMinutes};

/// 所有物理 Agent 共用扫描节奏和派生用量保留窗口。
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub struct UsageSettings {
    /// 周期扫描之间的等待分钟数。
    pub scan_interval:ScanIntervalMinutes,
    /// 派生用量记录保留的本地自然日数量。
    pub retention_days:RetentionDays,
}

/// 两张设置卡各自提交一个字段，不使用过时的整页草稿覆盖另一字段。
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum UsageSettingField {
    /// 周期扫描等待间隔。
    ScanInterval,
    /// 派生用量保留天数。
    RetentionDays,
}

impl UsageSettings {
    /// 值域沿用既有强类型；失败时返回稳定字段身份并保留原快照。
    pub fn updated(self,field:UsageSettingField,value:u16)->Result<Self,UsageSettingField> {
        Ok(match field {
            UsageSettingField::ScanInterval=>Self{scan_interval:ScanIntervalMinutes::new(value).map_err(|_|field)?,..self},
            UsageSettingField::RetentionDays=>Self{retention_days:RetentionDays::new(value).map_err(|_|field)?,..self},
        })
    }
}
