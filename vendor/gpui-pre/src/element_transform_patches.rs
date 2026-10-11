//! 元素仿射补丁独立登记，避免主构建脚本混入长绘制接口清单。

use crate::generator::Patch;

/// 固定片段仍由同一生成器逐一检查，顺序在原有补丁之后。
pub const PATCHES: &[Patch] = &[
            Patch {
                name: "element-transform-module", path: "src/window.rs",
                before: include_str!("../patches/element-transform-module.before"),
                after: include_str!("../patches/element-transform-module.after"),
            },
            Patch {
                name: "element-transform-field", path: "src/window.rs",
                before: include_str!("../patches/element-transform-field.before"),
                after: include_str!("../patches/element-transform-field.after"),
            },
            Patch {
                name: "element-transform-init", path: "src/window.rs",
                before: include_str!("../patches/element-transform-init.before"),
                after: include_str!("../patches/element-transform-init.after"),
            },
            Patch {
                name: "element-transform-insert-00", path: "src/window.rs",
                before: include_str!("../patches/element-transform-insert-00.before"),
                after: include_str!("../patches/element-transform-insert-00.after"),
            },
            Patch {
                name: "element-transform-insert-01", path: "src/window.rs",
                before: include_str!("../patches/element-transform-insert-01.before"),
                after: include_str!("../patches/element-transform-insert-01.after"),
            },
            Patch {
                name: "element-transform-insert-02", path: "src/window.rs",
                before: include_str!("../patches/element-transform-insert-02.before"),
                after: include_str!("../patches/element-transform-insert-02.after"),
            },
            Patch {
                name: "element-transform-insert-03", path: "src/window.rs",
                before: include_str!("../patches/element-transform-insert-03.before"),
                after: include_str!("../patches/element-transform-insert-03.after"),
            },
            Patch {
                name: "element-transform-insert-04", path: "src/window.rs",
                before: include_str!("../patches/element-transform-insert-04.before"),
                after: include_str!("../patches/element-transform-insert-04.after"),
            },
            Patch {
                name: "element-transform-insert-05", path: "src/window.rs",
                before: include_str!("../patches/element-transform-insert-05.before"),
                after: include_str!("../patches/element-transform-insert-05.after"),
            },
            Patch {
                name: "element-transform-insert-06", path: "src/window.rs",
                before: include_str!("../patches/element-transform-insert-06.before"),
                after: include_str!("../patches/element-transform-insert-06.after"),
            },
            Patch {
                name: "element-transform-insert-07", path: "src/window.rs",
                before: include_str!("../patches/element-transform-insert-07.before"),
                after: include_str!("../patches/element-transform-insert-07.after"),
            },
            Patch {
                name: "element-transform-insert-08", path: "src/window.rs",
                before: include_str!("../patches/element-transform-insert-08.before"),
                after: include_str!("../patches/element-transform-insert-08.after"),
            },
            Patch {
                name: "element-transform-insert-09", path: "src/window.rs",
                before: include_str!("../patches/element-transform-insert-09.before"),
                after: include_str!("../patches/element-transform-insert-09.after"),
            },
            Patch {
                name: "element-transform-insert-10", path: "src/window.rs",
                before: include_str!("../patches/element-transform-insert-10.before"),
                after: include_str!("../patches/element-transform-insert-10.after"),
            },
            Patch {
                name: "element-transform-insert-11", path: "src/window.rs",
                before: include_str!("../patches/element-transform-insert-11.before"),
                after: include_str!("../patches/element-transform-insert-11.after"),
            },
            Patch {
                name: "element-transform-insert-12", path: "src/window.rs",
                before: include_str!("../patches/element-transform-insert-12.before"),
                after: include_str!("../patches/element-transform-insert-12.after"),
            },
            Patch {
                name: "element-transform-insert-13", path: "src/window.rs",
                before: include_str!("../patches/element-transform-insert-13.before"),
                after: include_str!("../patches/element-transform-insert-13.after"),
            },
            Patch {
                name: "element-transform-insert-14", path: "src/window.rs",
                before: include_str!("../patches/element-transform-insert-14.before"),
                after: include_str!("../patches/element-transform-insert-14.after"),
            },
            Patch {
                name: "element-transform-insert-15", path: "src/window.rs",
                before: include_str!("../patches/element-transform-insert-15.before"),
                after: include_str!("../patches/element-transform-insert-15.after"),
            },
            Patch {
                name: "element-transform-insert-path", path: "src/window.rs",
                before: include_str!("../patches/element-transform-insert-path.before"),
                after: include_str!("../patches/element-transform-insert-path.after"),
            },
            Patch {
                name: "element-transform-layer", path: "src/window.rs",
                before: include_str!("../patches/element-transform-layer.before"),
                after: include_str!("../patches/element-transform-layer.after"),
            },
            Patch {
                name: "element-transform-hitbox", path: "src/window.rs",
                before: include_str!("../patches/element-transform-hitbox.before"),
                after: include_str!("../patches/element-transform-hitbox.after"),
            },
            Patch {
                name: "element-transform-ax", path: "src/element.rs",
                before: include_str!("../patches/element-transform-ax.before"),
                after: include_str!("../patches/element-transform-ax.after"),
            },
            Patch {
                name: "element-transform-cache-field", path: "src/view.rs",
                before: include_str!("../patches/element-transform-cache-field.before"),
                after: include_str!("../patches/element-transform-cache-field.after"),
            },
            Patch {
                name: "element-transform-cache-capture", path: "src/view.rs",
                before: include_str!("../patches/element-transform-cache-capture.before"),
                after: include_str!("../patches/element-transform-cache-capture.after"),
            },
            Patch {
                name: "element-transform-cache-compare", path: "src/view.rs",
                before: include_str!("../patches/element-transform-cache-compare.before"),
                after: include_str!("../patches/element-transform-cache-compare.after"),
            },
            Patch {
                name: "element-transform-cache-init", path: "src/view.rs",
                before: include_str!("../patches/element-transform-cache-init.before"),
                after: include_str!("../patches/element-transform-cache-init.after"),
            },
            Patch {
                name: "element-transform-deferred-field", path: "src/window.rs",
                before: include_str!("../patches/element-transform-deferred-field.before"),
                after: include_str!("../patches/element-transform-deferred-field.after"),
            },
            Patch {
                name: "element-transform-deferred-capture", path: "src/window.rs",
                before: include_str!("../patches/element-transform-deferred-capture.before"),
                after: include_str!("../patches/element-transform-deferred-capture.after"),
            },
            Patch {
                name: "element-transform-deferred-reuse", path: "src/window.rs",
                before: include_str!("../patches/element-transform-deferred-reuse.before"),
                after: include_str!("../patches/element-transform-deferred-reuse.after"),
            },
            Patch {
                name: "element-transform-deferred-prepaint-begin", path: "src/window.rs",
                before: include_str!("../patches/element-transform-deferred-prepaint-begin.before"),
                after: include_str!("../patches/element-transform-deferred-prepaint-begin.after"),
            },
            Patch {
                name: "element-transform-deferred-prepaint-end", path: "src/window.rs",
                before: include_str!("../patches/element-transform-deferred-prepaint-end.before"),
                after: include_str!("../patches/element-transform-deferred-prepaint-end.after"),
            },
            Patch {
                name: "element-transform-deferred-paint-begin", path: "src/window.rs",
                before: include_str!("../patches/element-transform-deferred-paint-begin.before"),
                after: include_str!("../patches/element-transform-deferred-paint-begin.after"),
            },
            Patch {
                name: "element-transform-deferred-paint-end", path: "src/window.rs",
                before: include_str!("../patches/element-transform-deferred-paint-end.before"),
                after: include_str!("../patches/element-transform-deferred-paint-end.after"),
            },
            Patch {
                name: "element-transform-snapped-mask", path: "src/window.rs",
                before: include_str!("../patches/element-transform-snapped-mask.before"),
                after: include_str!("../patches/element-transform-snapped-mask.after"),
            },
            Patch {
                name: "element-transform-ax-debug-bounds", path: "src/window/a11y/debug.rs",
                before: include_str!("../patches/element-transform-ax-debug-bounds.before"),
                after: include_str!("../patches/element-transform-ax-debug-bounds.after"),
            },
];
