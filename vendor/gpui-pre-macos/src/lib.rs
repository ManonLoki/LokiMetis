#![cfg(target_os = "macos")]
//! macOS platform implementation for GPUI.
//!
//! macOS screens have a y axis that goes up from the bottom of the screen and
//! an origin at the bottom left of the main display.

// 上游仍用已弃用的 cocoa/objc 绑定；迁移 objc2 属上游范围，此处仅压制第三方告警。
#![allow(deprecated)]

// 根 cfg/doc 属性由 shim 保留；其余原始模块路径来自生成树。
include!(concat!(env!("OUT_DIR"), "/gpui-patched/src/gpui_macos.rs"));

mod frame_demand;
