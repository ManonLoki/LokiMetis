//! 按需帧（demand）补丁独立登记；须排在父圆角裁剪补丁之后，因其与 window.rs 相邻片段共用上下文。

use crate::generator::Patch;

/// 顺序与原单表一致；每个片段仍由生成器逐一检查唯一性。
pub const PATCHES: &[Patch] = &[
    patch!("../patches/", "demand-reentrant-waker", "src/window.rs"),
    patch!("../patches/", "demand-deferred-presentation-state", "src/window.rs"),
    patch!("../patches/", "demand-reentrant-retain", "src/window.rs"),
    patch!("../patches/", "demand-deferred-presentation-take", "src/window.rs"),
    patch!("../patches/", "demand-throttle-restored-flags", "src/window.rs"),
    patch!("../patches/", "demand-present-restored-flag", "src/window.rs"),
    patch!("../patches/", "demand-sustain-schedule", "src/window.rs"),
    patch!("../patches/", "demand-sustain-wake", "src/window.rs"),
    patch!("../patches/", "demand-window-tests", "src/window.rs"),
    patch!("../patches/", "demand-test-isolate-svg", "src/svg_renderer.rs"),
    patch!("../patches/", "demand-test-isolate-property", "src/randomized_element_tree.rs"),
    patch!("../patches/", "demand-reentrant-contract", "src/window.rs"),
];
