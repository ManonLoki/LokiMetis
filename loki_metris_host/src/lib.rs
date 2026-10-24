//! core 的宿主驱动：只读 Agent 数据源发现与文件读取、增量索引扫描编排、应用私有文件与 SQLite 端口。
//!
//! 数据库规则、业务口径、会话日志逐行解析与稳定领域错误由共享 core 维护；本 crate 只拥有文件系统、
//! OS 目录 API 与读取驱动。它不依赖任何 UI 框架，依赖方向固定为 service → host → core。

pub mod file_stamp;
pub mod local_index;

pub mod workbuddy;

pub mod scan_orchestration;
pub mod source_catalog;

pub mod notification_state;
