#![allow(clippy::disallowed_methods, reason = "build scripts are exempt")]

#[path = "../gpui-patch-support/generator.rs"]
mod generator;
use generator::{Patch, Spec};
use std::path::{Path, PathBuf};

/// 所有上游文件和补丁结果有固定 SHA，编译阶段仅生成独立 OUT_DIR 镜像。
fn generated_sources() -> PathBuf {
    let spec = Spec {
        entry: "src/gpui.rs",
        root_prefix: include_str!("patches/root-prefix.before"),
        source_manifest_sha256: "6740789276091f55b305d15d7db9d3e81dd79909879869bdc0cc2a77134c8dd7",
        patches: &[
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
        ],
        generated_hashes: &[
            ("src/gpui.rs", "67e623c4b2ca71e132bd25043ab7c3d1b837d5aa158a518a2a574a8c8815ff3e"),
            ("src/scene.rs", "30e96981faabefeed0f63e04d1011c7a7864aa6c9826484eea67e9313ea14baf"),
            ("src/window.rs", "8e36ba60d9dfa45ec1833faaa8d3b5daa0b6e921ff282a1be2c781a8fef0509b"),
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
