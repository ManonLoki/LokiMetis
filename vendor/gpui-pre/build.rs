#![allow(clippy::disallowed_methods, reason = "build scripts are exempt")]

#[path = "../gpui-patch-support/generator.rs"]
mod generator;
use generator::{Patch, Spec};
#[path = "src/element_transform_patches.rs"]
mod element_transform_patches;
use std::path::{Path, PathBuf};

/// 所有上游文件和补丁结果有固定 SHA，编译阶段仅生成独立 OUT_DIR 镜像。
fn generated_sources() -> PathBuf {
    println!("cargo:rerun-if-changed=src/frame_demand_tests.rs");
    let mut spec = Spec {
        entry: "src/gpui.rs",
        root_prefix: include_str!("patches/root-prefix.before"),
        source_manifest_sha256: "6740789276091f55b305d15d7db9d3e81dd79909879869bdc0cc2a77134c8dd7",
        patches: &[
            Patch {
                name: "frame-animation-time-field", path: "src/window.rs",
                before: include_str!("patches/frame-animation-time-field.before"),
                after: include_str!("patches/frame-animation-time-field.after"),
            },
            Patch {
                name: "frame-animation-time-init", path: "src/window.rs",
                before: include_str!("patches/frame-animation-time-init.before"),
                after: include_str!("patches/frame-animation-time-init.after"),
            },
            Patch {
                name: "frame-animation-time-draw", path: "src/window.rs",
                before: include_str!("patches/frame-animation-time-draw.before"),
                after: include_str!("patches/frame-animation-time-draw.after"),
            },
            Patch {
                name: "frame-animation-time-api", path: "src/window.rs",
                before: include_str!("patches/frame-animation-time-api.before"),
                after: include_str!("patches/frame-animation-time-api.after"),
            },
            Patch {
                name: "image-frame-drop", path: "src/window.rs",
                before: include_str!("patches/image-frame-drop.before"),
                after: include_str!("patches/image-frame-drop.after"),
            },
            Patch {
                name: "sprite-clip-layout", path: "src/scene.rs",
                before: include_str!("patches/sprite-clip-layout.before"),
                after: include_str!("patches/sprite-clip-layout.after"),
            },
            Patch {
                name: "image-clip-type", path: "src/window.rs",
                before: include_str!("patches/image-clip-type.before"),
                after: include_str!("patches/image-clip-type.after"),
            },
            Patch {
                name: "image-clip-entry", path: "src/window.rs",
                before: include_str!("patches/image-clip-entry.before"),
                after: include_str!("patches/image-clip-entry.after"),
            },
            Patch {
                name: "image-empty-clip", path: "src/window.rs",
                before: include_str!("patches/image-empty-clip.before"),
                after: include_str!("patches/image-empty-clip.after"),
            },
            Patch {
                name: "image-original-clip-radii", path: "src/window.rs",
                before: include_str!("patches/image-original-clip-radii.before"),
                after: include_str!("patches/image-original-clip-radii.after"),
            },
            Patch {
                name: "image-clip-primitive", path: "src/window.rs",
                before: include_str!("patches/image-clip-primitive.before"),
                after: include_str!("patches/image-clip-primitive.after"),
            },
            Patch {
                name: "emoji-legacy-clip", path: "src/window.rs",
                before: include_str!("patches/emoji-legacy-clip.before"),
                after: include_str!("patches/emoji-legacy-clip.after"),
            },
            Patch {
                name: "painted-image-test-support", path: "src/window.rs",
                before: include_str!("patches/painted-image-test-support.before"),
                after: include_str!("patches/painted-image-test-support.after"),
            },
            Patch {
                name: "focus-generation", path: "src/window.rs",
                before: include_str!("patches/focus-generation.before"),
                after: include_str!("patches/focus-generation.after"),
            },
            Patch {
                name: "deferred-ax-test-field", path: "src/platform/test/window.rs",
                before: include_str!("patches/deferred-ax-test-field.before"),
                after: include_str!("patches/deferred-ax-test-field.after"),
            },
            Patch {
                name: "deferred-ax-test-init", path: "src/platform/test/window.rs",
                before: include_str!("patches/deferred-ax-test-init.before"),
                after: include_str!("patches/deferred-ax-test-init.after"),
            },
            Patch {
                name: "deferred-ax-test-activation", path: "src/platform/test/window.rs",
                before: include_str!("patches/deferred-ax-test-activation.before"),
                after: include_str!("patches/deferred-ax-test-activation.after"),
            },
            Patch {
                name: "deferred-ax-test-retain", path: "src/platform/test/window.rs",
                before: include_str!("patches/deferred-ax-test-retain.before"),
                after: include_str!("patches/deferred-ax-test-retain.after"),
            },
            Patch {
                name: "deferred-ax-test-context", path: "src/app/test_context.rs",
                before: include_str!("patches/deferred-ax-test-context.before"),
                after: include_str!("patches/deferred-ax-test-context.after"),
            },
            Patch {
                name: "deferred-ax-debug-hidden", path: "src/window/a11y/debug.rs",
                before: include_str!("patches/deferred-ax-debug-hidden.before"),
                after: include_str!("patches/deferred-ax-debug-hidden.after"),
            },
            Patch {
                name: "native-control-ax-debug-disabled", path: "src/window/a11y/debug.rs",
                before: include_str!("patches/native-control-ax-debug-disabled.before"),
                after: include_str!("patches/native-control-ax-debug-disabled.after"),
            },
            Patch {
                name: "deferred-ax-helper", path: "src/window/a11y.rs",
                before: include_str!("patches/deferred-ax-helper.before"),
                after: include_str!("patches/deferred-ax-helper.after"),
            },
            Patch {
                name: "deferred-ax-builder-field", path: "src/window/a11y.rs",
                before: include_str!("patches/deferred-ax-builder-field.before"),
                after: include_str!("patches/deferred-ax-builder-field.after"),
            },
            Patch {
                name: "deferred-ax-builder-init", path: "src/window/a11y.rs",
                before: include_str!("patches/deferred-ax-builder-init.before"),
                after: include_str!("patches/deferred-ax-builder-init.after"),
            },
            Patch {
                name: "deferred-ax-record-push", path: "src/window/a11y.rs",
                before: include_str!("patches/deferred-ax-record-push.before"),
                after: include_str!("patches/deferred-ax-record-push.after"),
            },
            Patch {
                name: "deferred-ax-record-leaf", path: "src/window/a11y.rs",
                before: include_str!("patches/deferred-ax-record-leaf.before"),
                after: include_str!("patches/deferred-ax-record-leaf.after"),
            },
            Patch {
                name: "deferred-ax-reset", path: "src/window/a11y.rs",
                before: include_str!("patches/deferred-ax-reset.before"),
                after: include_str!("patches/deferred-ax-reset.after"),
            },
            Patch {
                name: "deferred-ax-finalize", path: "src/window/a11y.rs",
                before: include_str!("patches/deferred-ax-finalize.before"),
                after: include_str!("patches/deferred-ax-finalize.after"),
            },
            Patch {
                name: "deferred-parent-fields", path: "src/window.rs",
                before: include_str!("patches/deferred-parent-fields.before"),
                after: include_str!("patches/deferred-parent-fields.after"),
            },
            Patch {
                name: "deferred-parent-capture-tuple", path: "src/window.rs",
                before: include_str!("patches/deferred-parent-capture-tuple.before"),
                after: include_str!("patches/deferred-parent-capture-tuple.after"),
            },
            Patch {
                name: "deferred-parent-capture-values", path: "src/window.rs",
                before: include_str!("patches/deferred-parent-capture-values.before"),
                after: include_str!("patches/deferred-parent-capture-values.after"),
            },
            Patch {
                name: "deferred-parent-begin-scope", path: "src/window.rs",
                before: include_str!("patches/deferred-parent-begin-scope.before"),
                after: include_str!("patches/deferred-parent-begin-scope.after"),
            },
            Patch {
                name: "deferred-parent-prepaint-mask", path: "src/window.rs",
                before: include_str!("patches/deferred-parent-prepaint-mask.before"),
                after: include_str!("patches/deferred-parent-prepaint-mask.after"),
            },
            Patch {
                name: "deferred-parent-end-scope", path: "src/window.rs",
                before: include_str!("patches/deferred-parent-end-scope.before"),
                after: include_str!("patches/deferred-parent-end-scope.after"),
            },
            Patch {
                name: "deferred-parent-replay", path: "src/window.rs",
                before: include_str!("patches/deferred-parent-replay.before"),
                after: include_str!("patches/deferred-parent-replay.after"),
            },
            Patch {
                name: "deferred-parent-range-query", path: "src/window.rs",
                before: include_str!("patches/deferred-parent-range-query.before"),
                after: include_str!("patches/deferred-parent-range-query.after"),
            },
            Patch {
                name: "deferred-parent-entry", path: "src/window.rs",
                before: include_str!("patches/deferred-parent-entry.before"),
                after: include_str!("patches/deferred-parent-entry.after"),
            },
            Patch {
                name: "deferred-parent-cache-field", path: "src/view.rs",
                before: include_str!("patches/deferred-parent-cache-field.before"),
                after: include_str!("patches/deferred-parent-cache-field.after"),
            },
            Patch {
                name: "deferred-parent-cache-capture", path: "src/view.rs",
                before: include_str!("patches/deferred-parent-cache-capture.before"),
                after: include_str!("patches/deferred-parent-cache-capture.after"),
            },
            Patch {
                name: "deferred-parent-cache-guard", path: "src/view.rs",
                before: include_str!("patches/deferred-parent-cache-guard.before"),
                after: include_str!("patches/deferred-parent-cache-guard.after"),
            },
            Patch {
                name: "deferred-parent-cache-replay-mark", path: "src/view.rs",
                before: include_str!("patches/deferred-parent-cache-replay-mark.before"),
                after: include_str!("patches/deferred-parent-cache-replay-mark.after"),
            },
            Patch {
                name: "deferred-parent-cache-mark", path: "src/view.rs",
                before: include_str!("patches/deferred-parent-cache-mark.before"),
                after: include_str!("patches/deferred-parent-cache-mark.after"),
            },
            Patch {
                name: "hitbox-exclusions-field", path: "src/window.rs",
                before: include_str!("patches/hitbox-exclusions-field.before"),
                after: include_str!("patches/hitbox-exclusions-field.after"),
            },
            Patch {
                name: "hitbox-exclusions-init", path: "src/window.rs",
                before: include_str!("patches/hitbox-exclusions-init.before"),
                after: include_str!("patches/hitbox-exclusions-init.after"),
            },
            Patch {
                name: "hitbox-exclusions-deferred-field", path: "src/window.rs",
                before: include_str!("patches/hitbox-exclusions-deferred-field.before"),
                after: include_str!("patches/hitbox-exclusions-deferred-field.after"),
            },
            Patch {
                name: "hitbox-exclusions-deferred-capture", path: "src/window.rs",
                before: include_str!("patches/hitbox-exclusions-deferred-capture.before"),
                after: include_str!("patches/hitbox-exclusions-deferred-capture.after"),
            },
            Patch {
                name: "hitbox-exclusions-deferred-tuple", path: "src/window.rs",
                before: include_str!("patches/hitbox-exclusions-deferred-tuple.before"),
                after: include_str!("patches/hitbox-exclusions-deferred-tuple.after"),
            },
            Patch {
                name: "hitbox-exclusions-deferred-prepaint", path: "src/window.rs",
                before: include_str!("patches/hitbox-exclusions-deferred-prepaint.before"),
                after: include_str!("patches/hitbox-exclusions-deferred-prepaint.after"),
            },
            Patch {
                name: "hitbox-exclusions-deferred-replay", path: "src/window.rs",
                before: include_str!("patches/hitbox-exclusions-deferred-replay.before"),
                after: include_str!("patches/hitbox-exclusions-deferred-replay.after"),
            },
            Patch {
                name: "hitbox-exclusions-deferred-init", path: "src/window.rs",
                before: include_str!("patches/hitbox-exclusions-deferred-init.before"),
                after: include_str!("patches/hitbox-exclusions-deferred-init.after"),
            },
            Patch {
                name: "hitbox-exclusions-api", path: "src/window.rs",
                before: include_str!("patches/hitbox-exclusions-api.before"),
                after: include_str!("patches/hitbox-exclusions-api.after"),
            },
            Patch {
                name: "hitbox-exclusions-insert", path: "src/window.rs",
                before: include_str!("patches/hitbox-exclusions-insert.before"),
                after: include_str!("patches/hitbox-exclusions-insert.after"),
            },
            Patch {
                name: "hitbox-exclusions-mask-doc", path: "src/window.rs",
                before: include_str!("patches/hitbox-exclusions-mask-doc.before"),
                after: include_str!("patches/hitbox-exclusions-mask-doc.after"),
            },
            Patch {
                name: "hitbox-exclusions-test-ids", path: "src/window.rs",
                before: include_str!("patches/hitbox-exclusions-test-ids.before"),
                after: include_str!("patches/hitbox-exclusions-test-ids.after"),
            },
            Patch {
                name: "hitbox-exclusions-cache-field", path: "src/view.rs",
                before: include_str!("patches/hitbox-exclusions-cache-field.before"),
                after: include_str!("patches/hitbox-exclusions-cache-field.after"),
            },
            Patch {
                name: "hitbox-exclusions-cache-capture", path: "src/view.rs",
                before: include_str!("patches/hitbox-exclusions-cache-capture.before"),
                after: include_str!("patches/hitbox-exclusions-cache-capture.after"),
            },
            Patch {
                name: "hitbox-exclusions-cache-compare", path: "src/view.rs",
                before: include_str!("patches/hitbox-exclusions-cache-compare.before"),
                after: include_str!("patches/hitbox-exclusions-cache-compare.after"),
            },
            Patch {
                name: "hitbox-exclusions-cache-init", path: "src/view.rs",
                before: include_str!("patches/hitbox-exclusions-cache-init.before"),
                after: include_str!("patches/hitbox-exclusions-cache-init.after"),
            },
            // 父圆角裁剪补丁组：scene 图元新增 parent_clip 字段；window 维护父裁剪链，view 缓存与 style/div 提供 clip_rounded_children 入口。
            Patch {
                name: "parent-clip-types", path: "src/scene.rs",
                before: include_str!("patches/parent-clip-types.before"),
                after: include_str!("patches/parent-clip-types.after"),
            },
            Patch {
                name: "parent-clip-scene-fields", path: "src/scene.rs",
                before: include_str!("patches/parent-clip-scene-fields.before"),
                after: include_str!("patches/parent-clip-scene-fields.after"),
            },
            Patch {
                name: "parent-clip-clear", path: "src/scene.rs",
                before: include_str!("patches/parent-clip-clear.before"),
                after: include_str!("patches/parent-clip-clear.after"),
            },
            Patch {
                name: "parent-clip-insert", path: "src/scene.rs",
                before: include_str!("patches/parent-clip-insert.before"),
                after: include_str!("patches/parent-clip-insert.after"),
            },
            Patch {
                name: "parent-clip-replay", path: "src/scene.rs",
                before: include_str!("patches/parent-clip-replay.before"),
                after: include_str!("patches/parent-clip-replay.after"),
            },
            Patch {
                name: "parent-clip-quad-field", path: "src/scene.rs",
                before: include_str!("patches/parent-clip-quad-field.before"),
                after: include_str!("patches/parent-clip-quad-field.after"),
            },
            Patch {
                name: "parent-clip-shadow-field", path: "src/scene.rs",
                before: include_str!("patches/parent-clip-shadow-field.before"),
                after: include_str!("patches/parent-clip-shadow-field.after"),
            },
            Patch {
                name: "parent-clip-underline-field", path: "src/scene.rs",
                before: include_str!("patches/parent-clip-underline-field.before"),
                after: include_str!("patches/parent-clip-underline-field.after"),
            },
            Patch {
                name: "parent-clip-monochromesprite-field", path: "src/scene.rs",
                before: include_str!("patches/parent-clip-monochromesprite-field.before"),
                after: include_str!("patches/parent-clip-monochromesprite-field.after"),
            },
            Patch {
                name: "parent-clip-subpixelsprite-field", path: "src/scene.rs",
                before: include_str!("patches/parent-clip-subpixelsprite-field.before"),
                after: include_str!("patches/parent-clip-subpixelsprite-field.after"),
            },
            Patch {
                name: "parent-clip-polychromesprite-field", path: "src/scene.rs",
                before: include_str!("patches/parent-clip-polychromesprite-field.before"),
                after: include_str!("patches/parent-clip-polychromesprite-field.after"),
            },
            Patch {
                name: "parent-clip-paintsurface-field", path: "src/scene.rs",
                before: include_str!("patches/parent-clip-paintsurface-field.before"),
                after: include_str!("patches/parent-clip-paintsurface-field.after"),
            },
            Patch {
                name: "parent-clip-path-field", path: "src/scene.rs",
                before: include_str!("patches/parent-clip-path-field.before"),
                after: include_str!("patches/parent-clip-path-field.after"),
            },
            Patch {
                name: "parent-clip-path-new", path: "src/scene.rs",
                before: include_str!("patches/parent-clip-path-new.before"),
                after: include_str!("patches/parent-clip-path-new.after"),
            },
            Patch {
                name: "parent-clip-path-scale", path: "src/scene.rs",
                before: include_str!("patches/parent-clip-path-scale.before"),
                after: include_str!("patches/parent-clip-path-scale.after"),
            },
            Patch {
                name: "parent-clip-helpers", path: "src/scene.rs",
                before: include_str!("patches/parent-clip-helpers.before"),
                after: include_str!("patches/parent-clip-helpers.after"),
            },
            Patch {
                name: "parent-clip-tests", path: "src/scene.rs",
                before: include_str!("patches/parent-clip-tests.before"),
                after: include_str!("patches/parent-clip-tests.after"),
            },
            Patch {
                name: "parent-scene-import-visibility", path: "src/scene.rs",
                before: include_str!("patches/parent-scene-import-visibility.before"),
                after: include_str!("patches/parent-scene-import-visibility.after"),
            },
            Patch {
                name: "parent-painted-monochrome-test-support", path: "src/window.rs",
                before: include_str!("patches/parent-painted-monochrome-test-support.before"),
                after: include_str!("patches/parent-painted-monochrome-test-support.after"),
            },
            Patch {
                name: "parent-window-imports", path: "src/window.rs",
                before: include_str!("patches/parent-window-imports.before"),
                after: include_str!("patches/parent-window-imports.after"),
            },
            Patch {
                name: "parent-hitbox-chain", path: "src/window.rs",
                before: include_str!("patches/parent-hitbox-chain.before"),
                after: include_str!("patches/parent-hitbox-chain.after"),
            },
            Patch {
                name: "parent-deferred-chain", path: "src/window.rs",
                before: include_str!("patches/parent-deferred-chain.before"),
                after: include_str!("patches/parent-deferred-chain.after"),
            },
            Patch {
                name: "parent-hit-test", path: "src/window.rs",
                before: include_str!("patches/parent-hit-test.before"),
                after: include_str!("patches/parent-hit-test.after"),
            },
            Patch {
                name: "parent-window-field", path: "src/window.rs",
                before: include_str!("patches/parent-window-field.before"),
                after: include_str!("patches/parent-window-field.after"),
            },
            Patch {
                name: "parent-window-init", path: "src/window.rs",
                before: include_str!("patches/parent-window-init.before"),
                after: include_str!("patches/parent-window-init.after"),
            },
            Patch {
                name: "parent-deferred-prepaint-capture", path: "src/window.rs",
                before: include_str!("patches/parent-deferred-prepaint-capture.before"),
                after: include_str!("patches/parent-deferred-prepaint-capture.after"),
            },
            Patch {
                name: "parent-deferred-prepaint-scope", path: "src/window.rs",
                before: include_str!("patches/parent-deferred-prepaint-scope.before"),
                after: include_str!("patches/parent-deferred-prepaint-scope.after"),
            },
            Patch {
                name: "parent-deferred-paint-scope", path: "src/window.rs",
                before: include_str!("patches/parent-deferred-paint-scope.before"),
                after: include_str!("patches/parent-deferred-paint-scope.after"),
            },
            Patch {
                name: "parent-deferred-reuse", path: "src/window.rs",
                before: include_str!("patches/parent-deferred-reuse.before"),
                after: include_str!("patches/parent-deferred-reuse.after"),
            },
            Patch {
                name: "parent-window-scope", path: "src/window.rs",
                before: include_str!("patches/parent-window-scope.before"),
                after: include_str!("patches/parent-window-scope.after"),
            },
            Patch {
                name: "parent-deferred-insert", path: "src/window.rs",
                before: include_str!("patches/parent-deferred-insert.before"),
                after: include_str!("patches/parent-deferred-insert.after"),
            },
            Patch {
                name: "parent-hitbox-insert", path: "src/window.rs",
                before: include_str!("patches/parent-hitbox-insert.before"),
                after: include_str!("patches/parent-hitbox-insert.after"),
            },
            Patch {
                name: "parent-underline-constructor-2", path: "src/window.rs",
                before: include_str!("patches/parent-underline-constructor-2.before"),
                after: include_str!("patches/parent-underline-constructor-2.after"),
            },
            Patch {
                name: "parent-underline-constructor-1", path: "src/window.rs",
                before: include_str!("patches/parent-underline-constructor-1.before"),
                after: include_str!("patches/parent-underline-constructor-1.after"),
            },
            Patch {
                name: "parent-shadow-constructor-2", path: "src/window.rs",
                before: include_str!("patches/parent-shadow-constructor-2.before"),
                after: include_str!("patches/parent-shadow-constructor-2.after"),
            },
            Patch {
                name: "parent-shadow-constructor-1", path: "src/window.rs",
                before: include_str!("patches/parent-shadow-constructor-1.before"),
                after: include_str!("patches/parent-shadow-constructor-1.after"),
            },
            Patch {
                name: "parent-quad-constructor-1", path: "src/window.rs",
                before: include_str!("patches/parent-quad-constructor-1.before"),
                after: include_str!("patches/parent-quad-constructor-1.after"),
            },
            Patch {
                name: "parent-subpixelsprite-constructor-1", path: "src/window.rs",
                before: include_str!("patches/parent-subpixelsprite-constructor-1.before"),
                after: include_str!("patches/parent-subpixelsprite-constructor-1.after"),
            },
            Patch {
                name: "parent-monochromesprite-constructor-2", path: "src/window.rs",
                before: include_str!("patches/parent-monochromesprite-constructor-2.before"),
                after: include_str!("patches/parent-monochromesprite-constructor-2.after"),
            },
            Patch {
                name: "parent-monochromesprite-constructor-1", path: "src/window.rs",
                before: include_str!("patches/parent-monochromesprite-constructor-1.before"),
                after: include_str!("patches/parent-monochromesprite-constructor-1.after"),
            },
            Patch {
                name: "parent-polychromesprite-constructor-2", path: "src/window.rs",
                before: include_str!("patches/parent-polychromesprite-constructor-2.before"),
                after: include_str!("patches/parent-polychromesprite-constructor-2.after"),
            },
            Patch {
                name: "parent-polychromesprite-constructor-1", path: "src/window.rs",
                before: include_str!("patches/parent-polychromesprite-constructor-1.before"),
                after: include_str!("patches/parent-polychromesprite-constructor-1.after"),
            },
            Patch {
                name: "parent-paintsurface-constructor-1", path: "src/window.rs",
                before: include_str!("patches/parent-paintsurface-constructor-1.before"),
                after: include_str!("patches/parent-paintsurface-constructor-1.after"),
            },
            Patch {
                name: "parent-window-tests", path: "src/window.rs",
                before: include_str!("patches/parent-window-tests.before"),
                after: include_str!("patches/parent-window-tests.after"),
            },
            Patch {
                name: "parent-view-cache-field", path: "src/view.rs",
                before: include_str!("patches/parent-view-cache-field.before"),
                after: include_str!("patches/parent-view-cache-field.after"),
            },
            Patch {
                name: "parent-view-cache-capture", path: "src/view.rs",
                before: include_str!("patches/parent-view-cache-capture.before"),
                after: include_str!("patches/parent-view-cache-capture.after"),
            },
            Patch {
                name: "parent-view-cache-compare", path: "src/view.rs",
                before: include_str!("patches/parent-view-cache-compare.before"),
                after: include_str!("patches/parent-view-cache-compare.after"),
            },
            Patch {
                name: "parent-view-cache-init", path: "src/view.rs",
                before: include_str!("patches/parent-view-cache-init.before"),
                after: include_str!("patches/parent-view-cache-init.after"),
            },
            Patch {
                name: "parent-style-field", path: "src/style.rs",
                before: include_str!("patches/parent-style-field.before"),
                after: include_str!("patches/parent-style-field.after"),
            },
            Patch {
                name: "parent-style-default", path: "src/style.rs",
                before: include_str!("patches/parent-style-default.before"),
                after: include_str!("patches/parent-style-default.after"),
            },
            Patch {
                name: "parent-style-geometry", path: "src/style.rs",
                before: include_str!("patches/parent-style-geometry.before"),
                after: include_str!("patches/parent-style-geometry.after"),
            },
            Patch {
                name: "parent-styled-opt-in", path: "src/styled.rs",
                before: include_str!("patches/parent-styled-opt-in.before"),
                after: include_str!("patches/parent-styled-opt-in.after"),
            },
            Patch {
                name: "parent-div-prepaint", path: "src/elements/div.rs",
                before: include_str!("patches/parent-div-prepaint.before"),
                after: include_str!("patches/parent-div-prepaint.after"),
            },
            Patch {
                name: "parent-div-paint", path: "src/elements/div.rs",
                before: include_str!("patches/parent-div-paint.before"),
                after: include_str!("patches/parent-div-paint.after"),
            },
            Patch {
                name: "parent-debug-quad", path: "src/debug_overlay.rs",
                before: include_str!("patches/parent-debug-quad.before"),
                after: include_str!("patches/parent-debug-quad.after"),
            },
            Patch {
                name: "parent-native-renderer-test-context", path: "src/app/test_context.rs",
                before: include_str!("patches/parent-native-renderer-test-context.before"),
                after: include_str!("patches/parent-native-renderer-test-context.after"),
            },
            Patch {
                name: "parent-used-radii-constructor", path: "src/scene.rs",
                before: include_str!("patches/parent-used-radii-constructor.before"),
                after: include_str!("patches/parent-used-radii-constructor.after"),
            },
            Patch {
                name: "parent-used-radii-contains", path: "src/scene.rs",
                before: include_str!("patches/parent-used-radii-contains.before"),
                after: include_str!("patches/parent-used-radii-contains.after"),
            },
            Patch {
                name: "parent-used-radii-scale", path: "src/scene.rs",
                before: include_str!("patches/parent-used-radii-scale.before"),
                after: include_str!("patches/parent-used-radii-scale.after"),
            },
            Patch {
                name: "parent-used-radii-chain-push", path: "src/scene.rs",
                before: include_str!("patches/parent-used-radii-chain-push.before"),
                after: include_str!("patches/parent-used-radii-chain-push.after"),
            },
            Patch {
                name: "parent-used-radii-doc", path: "src/scene.rs",
                before: include_str!("patches/parent-used-radii-doc.before"),
                after: include_str!("patches/parent-used-radii-doc.after"),
            },
            Patch {
                name: "parent-padding-box-geometry", path: "src/style.rs",
                before: include_str!("patches/parent-padding-box-geometry.before"),
                after: include_str!("patches/parent-padding-box-geometry.after"),
            },

            Patch {
                name: "demand-reentrant-waker", path: "src/window.rs",
                before: include_str!("patches/demand-reentrant-waker.before"),
                after: include_str!("patches/demand-reentrant-waker.after"),
            },
            Patch {
                name: "demand-deferred-presentation-state", path: "src/window.rs",
                before: include_str!("patches/demand-deferred-presentation-state.before"),
                after: include_str!("patches/demand-deferred-presentation-state.after"),
            },
            Patch {
                name: "demand-reentrant-retain", path: "src/window.rs",
                before: include_str!("patches/demand-reentrant-retain.before"),
                after: include_str!("patches/demand-reentrant-retain.after"),
            },
            Patch {
                name: "demand-deferred-presentation-take", path: "src/window.rs",
                before: include_str!("patches/demand-deferred-presentation-take.before"),
                after: include_str!("patches/demand-deferred-presentation-take.after"),
            },
            Patch {
                name: "demand-throttle-restored-flags", path: "src/window.rs",
                before: include_str!("patches/demand-throttle-restored-flags.before"),
                after: include_str!("patches/demand-throttle-restored-flags.after"),
            },
            Patch {
                name: "demand-present-restored-flag", path: "src/window.rs",
                before: include_str!("patches/demand-present-restored-flag.before"),
                after: include_str!("patches/demand-present-restored-flag.after"),
            },
            Patch {
                name: "demand-sustain-schedule", path: "src/window.rs",
                before: include_str!("patches/demand-sustain-schedule.before"),
                after: include_str!("patches/demand-sustain-schedule.after"),
            },
            Patch {
                name: "demand-sustain-wake", path: "src/window.rs",
                before: include_str!("patches/demand-sustain-wake.before"),
                after: include_str!("patches/demand-sustain-wake.after"),
            },
            Patch {
                name: "demand-window-tests", path: "src/window.rs",
                before: include_str!("patches/demand-window-tests.before"),
                after: include_str!("patches/demand-window-tests.after"),
            },

            Patch {
                name: "demand-test-isolate-svg", path: "src/svg_renderer.rs",
                before: include_str!("patches/demand-test-isolate-svg.before"),
                after: include_str!("patches/demand-test-isolate-svg.after"),
            },
            Patch {
                name: "demand-test-isolate-property", path: "src/randomized_element_tree.rs",
                before: include_str!("patches/demand-test-isolate-property.before"),
                after: include_str!("patches/demand-test-isolate-property.after"),
            },

            Patch {
                name: "demand-reentrant-contract", path: "src/window.rs",
                before: include_str!("patches/demand-reentrant-contract.before"),
                after: include_str!("patches/demand-reentrant-contract.after"),
            },

        ],
        generated_hashes: &[
            ("src/element.rs", "db7ed5023c5ea130fea117fbe98b00dd0f401f2caa701864f22f180c677bd9c4"),
            ("src/randomized_element_tree.rs", "98acecacd50fab2862ae06e4fb01fd41e9290a701b77068809392236a888eea4"),
            ("src/svg_renderer.rs", "451bdcdb75315b64e1c6a10109a2dbf6808c9137075c124dd42dc841d92953f8"),
            ("src/app/test_context.rs", "5fc9f53e680a1020dc810ec184a02a6a7bfe97626fef0a8aed6ec1ea4858730d"),
            ("src/debug_overlay.rs", "35ed7c2918c601c298fe1eea0e7fa679763768016cfdc99f10f1ae6bffa4a622"),
            ("src/elements/div.rs", "989a5e69c8b7ad0155d1994ad65d8a6e10faad5a03f61a59fc5ab38d6ac481d9"),
            ("src/gpui.rs", "67e623c4b2ca71e132bd25043ab7c3d1b837d5aa158a518a2a574a8c8815ff3e"),
            ("src/platform/test/window.rs", "5cc6869738c7441976bc036aa469da4521a1a632e4a5805b19bfe2eb28fee303"),
            ("src/scene.rs", "f8e1a899071e298bed53c7eb10f0e65a8039e1663048033ab6f5e79a1a434af4"),
            ("src/style.rs", "770fa179edebbf8f5d79b93f850e002159dec9396a55e1a8d3f178221f341444"),
            ("src/styled.rs", "d7a9d0e6b52885c428ecab1bed2cb6c2141c6fcf55f2b2b6a242f1483723c50d"),
            ("src/view.rs", "04225139881aa7ee457fb8ac7fed4bdd83e261ce486807e808ba22165df31e96"),
            ("src/window.rs", "f5e53fd347fa48c6118c29fef0791dcf0b9e986b815ae65e9745b5d54bcda20d"),
            ("src/window/a11y.rs", "032ccb11a796b6c76e6bf5bafd5b31c1842a136ec7280dcda4e636ded8d83166"),
            ("src/window/a11y/debug.rs", "7bf5c18e3de0fe2e9e459aec0fe67bcce64731a8f86e9ea35dc9345ca05bac49"),
        ],
    };
    let patches: Vec<_> = spec.patches.iter().chain(element_transform_patches::PATCHES).map(|patch| Patch {
        name: patch.name, path: patch.path, before: patch.before, after: patch.after,
    }).collect();
    spec.patches = &patches;
    generator::generate(Path::new(env!("CARGO_MANIFEST_DIR")), &spec)
}

/// 先验证并生成源码，再保留上游泄漏检测与 Windows manifest 构建行为。
fn main() {
    let _generated = generated_sources();
    println!("cargo::rustc-check-cfg=cfg(gles)");

    // Leak detection does bookkeeping on every entity handle, so `test-support`
    // does not enable it: that feature must stay cheap enough to compile into
    // every build. CI sets `GPUI_LEAK_DETECTION`; the `leak-detection` feature
    // turns it on explicitly.
    println!("cargo::rustc-check-cfg=cfg(gpui_leak_detection)");
    println!("cargo::rerun-if-env-changed=GPUI_LEAK_DETECTION");
    let requested_by_environment =
        std::env::var_os("GPUI_LEAK_DETECTION").is_some_and(|value| !value.is_empty());
    if requested_by_environment || std::env::var_os("CARGO_FEATURE_LEAK_DETECTION").is_some() {
        println!("cargo::rustc-cfg=gpui_leak_detection");
    }

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    if target_os == "windows" {
        #[cfg(feature = "windows-manifest")]
        embed_resource(&_generated);
    }
}

#[cfg(feature = "windows-manifest")]
/// Windows 资源来自验证后的生成树，不能读取 shim 根下不存在的 resources。
fn embed_resource(generated: &Path) {
    let resource_dir = generated.join("resources/windows");
    let manifest = resource_dir.join("gpui.manifest.xml");
    let rc_file = resource_dir.join("gpui.rc");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rerun-if-changed={}", rc_file.display());
    embed_resource::compile(rc_file, embed_resource::ParamsIncludeDirs([&resource_dir]))
        .manifest_required()
        .unwrap();
}
