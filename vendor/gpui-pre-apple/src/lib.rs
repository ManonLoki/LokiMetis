#![cfg(any(target_os = "macos", target_os = "ios"))]
//! Shared Apple rendering and task dispatch for GPUI.
//!
//! This crate renders GPUI scenes directly with Metal on Apple platforms. It
//! owns GPU resources and shaders while leaving application lifecycle,
//! windowing, and input to each platform backend.

// 根属性由 shim 保留；完整模块、相对资源与 crate:: 路径来自生成树。
include!(concat!(env!("OUT_DIR"), "/gpui-patched/src/gpui_apple.rs"));

#[cfg(test)]
mod rounded_clip_tests;
