#![allow(clippy::disallowed_methods, reason = "build scripts are exempt")]

#[path = "../gpui-patch-support/generator.rs"]
#[macro_use]
mod generator;
use generator::{Patch, Spec};
#[path = "../gpui-patch-support/scene_patches.rs"]
mod scene_patches;
#[path = "src/image_frame_patches.rs"]
mod image_frame_patches;
#[path = "src/demand_patches.rs"]
mod demand_patches;
#[path = "src/parent_clip_patches.rs"]
mod parent_clip_patches;
#[path = "src/element_transform_patches.rs"]
mod element_transform_patches;
use std::path::{Path, PathBuf};

/// 所有上游文件和补丁结果有固定 SHA，编译阶段仅生成独立 OUT_DIR 镜像。
fn generated_sources() -> PathBuf {
    println!("cargo:rerun-if-changed=src/frame_demand_tests.rs");
    // 补丁按组登记；同一文件上的片段共享上下文，组间顺序（图片帧 → 父裁剪 → 按需帧 → 元素变换）不可调换。
    let patches: Vec<Patch> = [
        &scene_patches::all("src/scene.rs")[..],
        image_frame_patches::PATCHES,
        parent_clip_patches::PATCHES,
        demand_patches::PATCHES,
        element_transform_patches::PATCHES,
        &[
            Patch {
                name: "linux-hidden-window-map",
                path: "src/window.rs",
                before: include_str!("patches/linux-hidden-window-map.before"),
                after: include_str!("patches/linux-hidden-window-map.after"),
            },
            Patch {
                name: "test-window-map-count-field",
                path: "src/platform/test/window.rs",
                before: include_str!("patches/test-window-map-count-field.before"),
                after: include_str!("patches/test-window-map-count-field.after"),
            },
            Patch {
                name: "test-window-map-count-init",
                path: "src/platform/test/window.rs",
                before: include_str!("patches/test-window-map-count-init.before"),
                after: include_str!("patches/test-window-map-count-init.after"),
            },
            Patch {
                name: "test-window-map-count-api",
                path: "src/platform/test/window.rs",
                before: include_str!("patches/test-window-map-count-api.before"),
                after: include_str!("patches/test-window-map-count-api.after"),
            },
            Patch {
                name: "label-value",
                path: "src/elements/div.rs",
                before: include_str!("patches/label-value.before"),
                after: include_str!("patches/label-value.after"),
            },
            Patch {
                name: "style-tests-perf-attribute",
                path: "src/style.rs",
                before: include_str!("patches/style-tests-perf-attribute.before"),
                after: include_str!("patches/style-tests-perf-attribute.after"),
            },
            Patch {
                name: "test-window-frame-demand-access",
                path: "src/app/test_context.rs",
                before: include_str!("patches/test-window-frame-demand-access.before"),
                after: include_str!("patches/test-window-frame-demand-access.after"),
            },
        ],
    ].concat();
    let spec = Spec {
        entry: "src/gpui.rs",
        root_prefix: include_str!("patches/root-prefix.before"),
        source_manifest_sha256: "4222c84aea30d33b71bd7c092a6c2312f32dcf41163d46251203e84a34c09805",
        patches: &patches,
        generated_hashes: &[
            ("src/element.rs", "070fd78f0621ec254efe8a79c5b038c8fcda26c966fbf7001aae49bd8f214c86"),
            ("src/randomized_element_tree.rs", "a0ffba6aadf4f4ff574acd8250b2884f7fa0a9df4965e056d37a79b347280646"),
            ("src/svg_renderer.rs", "695506c802ae08c404779cdb4e95e0c43b7b5856c286754058b4c2da2965ef1e"),
            ("src/app/test_context.rs", "201509f02a3a955b90f7749c7e807fbc9a3e8b53a0ed01bdfdcc8873b272366e"),
            ("src/debug_overlay.rs", "35ed7c2918c601c298fe1eea0e7fa679763768016cfdc99f10f1ae6bffa4a622"),
            ("src/elements/div.rs", "af6e9a94538362868f6cead62326435f619b4eedc859b2f6e177be4d8f6926f9"),
            ("src/gpui.rs", "a0b85dbc661669efafd4b40ba505edac3a224a44f040cb6ee43b38f5dc9387c3"),
            ("src/platform/test/window.rs", "86d70fda02629d5db42501c41de388bc802c90aa292092560bbf1ee7dade4d8d"),
            ("src/scene.rs", "3c2e1598169001ed10a3b99df37718619558fe4c192f7a5423ced9ae1762ca9b"),
            ("src/style.rs", "afc90a84ce6b6aaa4de6aab9da60d0e8d48e4125bd3e8542be26c712cc2f0835"),
            ("src/styled.rs", "1b0fae562d25438df2572bae8c9c0dc45cef575c2f6df3afe83bff77f83196a9"),
            ("src/view.rs", "cb926e26ff8feabeb208a63419b7ebd885dd4c967bc63ac37511ccecd9ea9aba"),
            ("src/window.rs", "86a9dd73b9ff5676626144f99047791fddc096573f86b58b3def72ddd196e49f"),
            ("src/window/a11y.rs", "032ccb11a796b6c76e6bf5bafd5b31c1842a136ec7280dcda4e636ded8d83166"),
            ("src/window/a11y/debug.rs", "7bf5c18e3de0fe2e9e459aec0fe67bcce64731a8f86e9ea35dc9345ca05bac49"),
        ],
    };
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
