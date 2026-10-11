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
            ("src/app/test_context.rs", "70ad17984b35f317d430b74e0d5763835e78fcee724c2c93126faf6833c1ffda"),
            ("src/element.rs", "db7ed5023c5ea130fea117fbe98b00dd0f401f2caa701864f22f180c677bd9c4"),
            ("src/gpui.rs", "67e623c4b2ca71e132bd25043ab7c3d1b837d5aa158a518a2a574a8c8815ff3e"),
            ("src/platform/test/window.rs", "5cc6869738c7441976bc036aa469da4521a1a632e4a5805b19bfe2eb28fee303"),
            ("src/randomized_element_tree.rs", "98acecacd50fab2862ae06e4fb01fd41e9290a701b77068809392236a888eea4"),
            ("src/scene.rs", "30e96981faabefeed0f63e04d1011c7a7864aa6c9826484eea67e9313ea14baf"),
            ("src/svg_renderer.rs", "451bdcdb75315b64e1c6a10109a2dbf6808c9137075c124dd42dc841d92953f8"),
            ("src/view.rs", "18bc272b416bb3fc55d7a66a5e263d00847fa9b307154f6c5342b500ed6c1cd2"),
            ("src/window.rs", "0d8b4e21eec64559717e0ebef8f5e737bbb03c4bc010d34cdc064b005456872f"),
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
