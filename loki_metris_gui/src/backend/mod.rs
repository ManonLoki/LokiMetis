//! 只读 Agent 数据源驱动；数据库规则由共享 core 维护。

pub(crate) mod local_index;

pub(crate) mod workbuddy;

pub(crate) mod scan_orchestration;
pub(crate) mod source_catalog;

pub(crate) mod notification_state;
