#![cfg(target_os = "windows")]

// 根属性由 shim 保留；完整模块、相对资源与 crate:: 路径来自生成树。
include!(concat!(env!("OUT_DIR"), "/gpui-patched/src/gpui_windows.rs"));
