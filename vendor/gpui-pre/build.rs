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
        source_manifest_sha256: "34525d22e585a0e44848e9440efc127874fe14703cca9b7cae49bab5cc7bb163",
        patches: &patches,
        generated_hashes: &[
            ("src/element.rs", "34c1bce0a19624576243d3887ba6460e7e00a9b47a848a3141672622f27de5e6"),
            ("src/randomized_element_tree.rs", "0037f6467213be6580f5b873db359104f30ac9ba304993fc5cf3d5f860836f88"),
            ("src/svg_renderer.rs", "451bdcdb75315b64e1c6a10109a2dbf6808c9137075c124dd42dc841d92953f8"),
            ("src/app/test_context.rs", "8a59a8bda5ea64be449e208276621b1492e6d863d6bc52b43e0b70ab8338ed91"),
            ("src/debug_overlay.rs", "35ed7c2918c601c298fe1eea0e7fa679763768016cfdc99f10f1ae6bffa4a622"),
            ("src/elements/div.rs", "c796612c3efe0cc91ce464c67e129d31ff9d1433752a169885dadf546397a47e"),
            ("src/gpui.rs", "b4ff931355301a3fc762779d3b25e5605c37dfbb86d1a29d211a897acba6338e"),
            ("src/platform/test/window.rs", "9b95b0b5ec9b100d6270695b9d090b3931ab4113f3ea050d4ef026aae2eafda7"),
            ("src/scene.rs", "aa89a20f138be9bd0c2f7a9e424d91e326dd43af5988b75919c87969403f113b"),
            ("src/style.rs", "332c53a090e87cef0ee71e2b10e807be8ef4e376c654a84fe10c0b5886ae19cd"),
            ("src/styled.rs", "d7a9d0e6b52885c428ecab1bed2cb6c2141c6fcf55f2b2b6a242f1483723c50d"),
            ("src/view.rs", "0c7224c816769a66bed49d14c857b2d0976320faae3b15c54b19fd22daa04ef6"),
            ("src/window.rs", "a69cb772d09e7fc84c7fc0dce799e40c6eb7abdba26d93e8e510b246805093f1"),
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
