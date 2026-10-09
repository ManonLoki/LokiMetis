#![doc = include_str!("../upstream/README.md")]
#![warn(missing_docs)]
#![allow(clippy::type_complexity)] // Not useful, GPUI makes heavy use of callbacks
#![allow(clippy::collapsible_else_if)] // False positives in platform specific code
#![allow(unused_mut)] // False positives in platform specific code

// 根属性由 shim 保留；完整模块、相对资源与 crate:: 路径来自生成树。
include!(concat!(env!("OUT_DIR"), "/gpui-patched/src/gpui.rs"));
