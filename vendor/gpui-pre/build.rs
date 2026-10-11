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
        &[Patch {
            name: "label-value",
            path: "src/elements/div.rs",
            before: include_str!("patches/label-value.before"),
            after: include_str!("patches/label-value.after"),
        }],
    ].concat();
    let spec = Spec {
        entry: "src/gpui.rs",
        root_prefix: include_str!("patches/root-prefix.before"),
        source_manifest_sha256: "6740789276091f55b305d15d7db9d3e81dd79909879869bdc0cc2a77134c8dd7",
        patches: &patches,
        generated_hashes: &[
            ("src/element.rs", "db7ed5023c5ea130fea117fbe98b00dd0f401f2caa701864f22f180c677bd9c4"),
            ("src/randomized_element_tree.rs", "98acecacd50fab2862ae06e4fb01fd41e9290a701b77068809392236a888eea4"),
            ("src/svg_renderer.rs", "451bdcdb75315b64e1c6a10109a2dbf6808c9137075c124dd42dc841d92953f8"),
            ("src/app/test_context.rs", "5fc9f53e680a1020dc810ec184a02a6a7bfe97626fef0a8aed6ec1ea4858730d"),
            ("src/debug_overlay.rs", "35ed7c2918c601c298fe1eea0e7fa679763768016cfdc99f10f1ae6bffa4a622"),
            ("src/elements/div.rs", "1eade4ee979ab0e9c4a1d275f4730ac8f4a598baa2b3e079fe5e8189b627ef07"),
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
