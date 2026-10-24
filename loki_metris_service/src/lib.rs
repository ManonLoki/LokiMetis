//! 应用服务层：单 writer 扫描队列、只读查询邮箱、今日摘要与里程碑通知 worker，以及换肤、
//! Hook 监控与设备联动的后台 owner。
//!
//! 服务层只交付 Tokio oneshot 回执与不依赖界面框架的 DTO；界面轮询与原生图片转换属于 GUI。
//! 依赖方向固定为 gui → service → host → core。

pub mod monitor;
pub mod notification_events;
pub mod skins;
pub mod usage_service;
