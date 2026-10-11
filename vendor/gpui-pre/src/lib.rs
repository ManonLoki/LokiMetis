#![doc = include_str!("../upstream/README.md")]
#![warn(missing_docs)]
#![allow(clippy::type_complexity)] // Not useful, GPUI makes heavy use of callbacks
#![allow(clippy::collapsible_else_if)] // False positives in platform specific code
#![allow(unused_imports)] // 生成树中 a11y 的未用导入属上游代码
#![allow(unused_mut)] // False positives in platform specific code

// 根属性由 shim 保留；完整模块、相对资源与 crate:: 路径来自生成树。
include!(concat!(env!("OUT_DIR"), "/gpui-patched/src/gpui.rs"));

// 仅允许已有实际证据的macOS Metal目标；default-off仍沿所有平台原ABI编译。
#[cfg(all(feature = "parent-rounded-clip", not(target_os = "macos")))]
compile_error!("parent-rounded-clip currently supports only macOS Apple Metal; iOS and other targets remain unvalidated");
