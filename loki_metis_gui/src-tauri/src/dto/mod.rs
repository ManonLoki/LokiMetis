//! 定义 Tauri command 与双语前端之间的稳定、无敏感路径 DTO。
//! DTO = Data Transfer Object：这些结构体只负责“跨进程边界传输数据”，
//! 字段形状为前端服务，不直接复用 core 里的内部业务类型，即使看起来相似
//! （例如 UiMessageCodeDto 这种“语义代码”而不是中文文案本身），这样前端
//! 才能按当前语言自行渲染文案，Rust 后端不需要关心具体展示哪种语言的文本。
//!
//! 本模块按领域拆成多个子文件，每个子文件只负责一类紧密相关的 DTO：
//! `common`、`client`、`usage`、`calls`、`statistics`、`sources`、`scan`、
//! `privacy`、`system`、`workbuddy`。下面统一 `pub use` 回同一层命名空间。

mod calls;
mod charts;
mod client;
mod common;
mod privacy;
mod scan;
mod sources;
mod statistics;
mod system;
mod usage;
mod workbuddy;

pub use calls::*;
pub use charts::*;
pub use client::*;
pub use common::*;
pub use privacy::*;
pub use scan::*;
pub use sources::*;
pub use statistics::*;
pub use system::*;
pub use usage::*;
pub use workbuddy::*;

/// `UsageDimension` 直接作为 DTO 字段类型使用，需向其他模块公开重新导出。
pub use loki_metis_core::UsageDimension;
