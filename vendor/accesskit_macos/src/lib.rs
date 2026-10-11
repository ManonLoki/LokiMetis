//! 保留 AccessKit 0.26.3 的原始模块，仅从构建输出装配带标题兼容补丁的节点模块。
#![deny(unsafe_op_in_unsafe_fn)]

#[path = "../upstream/src/context.rs"]
mod context;
#[path = "../upstream/src/filters.rs"]
mod filters;
#[path = "../upstream/src/util.rs"]
mod util;

// 生成文件包含完整 mod node，以保留上游节点文件的模块级 inner attribute。
include!(concat!(env!("OUT_DIR"), "/node.rs"));

#[path = "../upstream/src/adapter.rs"]
mod adapter;
pub use adapter::Adapter;

#[path = "../upstream/src/event.rs"]
mod event;
pub use event::QueuedEvents;

#[path = "../upstream/src/patch.rs"]
mod patch;
pub use patch::add_focus_forwarder_to_window_class;

#[path = "../upstream/src/subclass.rs"]
mod subclass;
pub use subclass::SubclassingAdapter;

pub use objc2_foundation::{NSArray, NSObject, NSPoint};
