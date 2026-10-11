#![allow(clippy::disallowed_methods, reason = "build scripts are exempt")]

#[path = "../gpui-patch-support/generator.rs"]
#[macro_use]
mod generator;
use generator::{Patch, Spec};
#[path = "../gpui-patch-support/scene_patches.rs"]
mod scene_patches;
#[path = "src/metal_patches.rs"]
mod metal_patches;
use std::path::{Path, PathBuf};

/// 所有上游文件和补丁结果有固定 SHA，编译阶段仅生成独立 OUT_DIR 镜像。
fn generated_sources() -> PathBuf {
    // scene.rs 补丁与 gpui-pre 共用，其余为 Metal 专属。
    let patches: Vec<Patch> = [
        &scene_patches::all("vendor/gpui/src/scene.rs")[..],
        metal_patches::PATCHES,
    ].concat();
    let spec = Spec {
        entry: "src/gpui_apple.rs",
        root_prefix: include_str!("patches/root-prefix.before"),
        source_manifest_sha256: "3a0648847f84019c27d80c8d030a34e709136c4e65c1c69643b1d527cc41c95d",
        patches: &patches,
        generated_hashes: &[
            ("src/metal_atlas.rs", "6fa211297007a14a21b1b547dac528133c6b5dc7c1271923b63ea71fc978b084"),
            ("src/metal_renderer.rs", "91e4b5cde950223e8c8c2d11e4e4b17803b6d5bc4e0c911cdc8e908adfe34f5c"),
            ("src/gpui_apple.rs", "13f5d77ff374cd40082f29f20aeb6a2d185643e15d5031bcbe6a8538ad3d2f7f"),
            ("vendor/gpui/src/scene.rs", "f8e1a899071e298bed53c7eb10f0e65a8039e1663048033ab6f5e79a1a434af4"),
            ("src/shaders.metal", "0293a36fef5f5bbde11fcad44ac903c6d70bb6c83c9db86c26a831f6ae53da3c"),
        ],
    };
    generator::generate(Path::new(env!("CARGO_MANIFEST_DIR")), &spec)
}

/// 验证原包并生成全树后，才运行平台原有的 shader 构建。
fn main() {
    let _generated = generated_sources();
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    apple_build::run(&_generated);
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple_build {
    use std::{
        env,
        path::{Path, PathBuf},
    };

    use cbindgen::Config;

    /// 使用生成的镜像和 Metal 源构建同一 ABI 的 header 与 shader。
    pub fn run(generated: &Path) {
        let header_path = generate_shader_bindings(generated);

        #[cfg(feature = "runtime_shaders")]
        emit_stitched_shaders(&header_path, generated);
        #[cfg(not(feature = "runtime_shaders"))]
        compile_metal_shaders(&header_path, generated);
    }

    /// cbindgen 读取生成镜像，避免仍从 immutable 上游读取旧图元结构。
    fn generate_shader_bindings(generated: &Path) -> PathBuf {
        let output_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("scene.h");

        let gpui_dir = generated.join("vendor/gpui");

        let mut config = Config {
            include_guard: Some("SCENE_H".into()),
            language: cbindgen::Language::C,
            no_includes: true,
            ..Default::default()
        };
        config.export.include.extend([
            "Bounds".into(),
            "Corners".into(),
            "Edges".into(),
            "Size".into(),
            "Pixels".into(),
            "PointF".into(),
            "Hsla".into(),
            "ContentMask".into(),
            // 导出父裁剪类型，使 Metal 头文件可见。
            "ParentClipId".into(),
            "ParentClipNode".into(),
            "Uniforms".into(),
            "AtlasTile".into(),
            "PathRasterizationInputIndex".into(),
            "PathVertex_ScaledPixels".into(),
            "PathRasterizationVertex".into(),
            "ShadowInputIndex".into(),
            "Shadow".into(),
            "QuadInputIndex".into(),
            "Underline".into(),
            "UnderlineInputIndex".into(),
            "Quad".into(),
            "BorderStyle".into(),
            "SpriteInputIndex".into(),
            "MonochromeSprite".into(),
            "PolychromeSprite".into(),
            "PathSprite".into(),
            "SurfaceInputIndex".into(),
            "SurfaceBounds".into(),
            "TransformationMatrix".into(),
        ]);
        // 让 cbindgen 把 Rust feature 映射为 C 宏，头文件内 #if 受其控制。
        config.defines.insert("feature = parent-rounded-clip".into(), "GPUI_PARENT_ROUNDED_CLIP".into());
        config.no_includes = true;
        config.enumeration.prefix_with_name = true;

        let mut builder = cbindgen::Builder::new();

        let crate_dir = generated.to_owned();

        // Source files from gpui that define types used in shaders
        let gpui_src_paths = [
            gpui_dir.join("src/scene.rs"),
            gpui_dir.join("src/geometry.rs"),
            gpui_dir.join("src/color.rs"),
            gpui_dir.join("src/window.rs"),
            gpui_dir.join("src/platform.rs"),
        ];

        // Source files from this crate
        let local_src_paths = [crate_dir.join("src/metal_renderer.rs")];

        for src_path in gpui_src_paths.iter().chain(local_src_paths.iter()) {
            builder = builder.with_src(src_path);
        }

        builder
            .with_config(config)
            .generate()
            .expect("Unable to generate bindings")
            .write_to_file(&output_path);

        // cbindgen 的 defines 只生成 #if 守卫、不会定义该宏，故两者缺一不可；
        // 此处补 #define，使预编译与运行时 shader 读取同一份头文件。
        #[cfg(feature = "parent-rounded-clip")]
        {
            let contents = std::fs::read_to_string(&output_path).unwrap();
            std::fs::write(&output_path, format!("#define GPUI_PARENT_ROUNDED_CLIP 1\n{contents}")).unwrap();
        }
        output_path
    }

    /// To enable runtime compilation, we need to "stitch" the shaders file with the generated header
    /// so that it is self-contained.
    /// 运行时编译拼接同一已验证镜像的着色器与C头，确保两种编译模式共用图元ABI。
    #[cfg(feature = "runtime_shaders")]
    fn emit_stitched_shaders(header_path: &Path, generated: &Path) {
        /// runtime_shaders 使用同一已验证的 header 与 shader，保持源码编译路径一致。
        fn stitch_header(header: &Path, shader_path: &Path) -> std::io::Result<PathBuf> {
            let header_contents = std::fs::read_to_string(header)?;
            let shader_contents = std::fs::read_to_string(shader_path)?;
            let stitched_contents = format!("{header_contents}\n{shader_contents}");
            let out_path =
                PathBuf::from(env::var("OUT_DIR").unwrap()).join("stitched_shaders.metal");
            std::fs::write(&out_path, stitched_contents)?;
            Ok(out_path)
        }
        let shader_path = generated.join("src/shaders.metal");
        stitch_header(header_path, &shader_path).unwrap();
    }

    #[cfg(not(feature = "runtime_shaders"))]
    /// 目标 SDK、版本门槛和 Metal 参数沿用上游，只改变输入文件的来源。
    fn compile_metal_shaders(header_path: &Path, generated: &Path) {
        use std::process::{self, Command};

        // Build scripts run on the host, so the target platform must come from
        // Cargo's environment rather than `cfg!`. The target environment, not the
        // target name, identifies simulators: `x86_64-apple-ios` has no `-sim` suffix.
        let target_os = env::var("CARGO_CFG_TARGET_OS")
            .expect("Cargo sets CARGO_CFG_TARGET_OS for build scripts");
        let target_env = env::var("CARGO_CFG_TARGET_ENV")
            .expect("Cargo sets CARGO_CFG_TARGET_ENV for build scripts");
        let (sdk, minimum_version_argument) = match (target_os.as_str(), target_env.as_str()) {
            ("macos", _) => ("macosx", "-mmacosx-version-min=10.15.7"),
            ("ios", "sim") => ("iphonesimulator", "-mios-simulator-version-min=15.0"),
            ("ios", "") => ("iphoneos", "-mios-version-min=15.0"),
            _ => {
                println!(
                    "cargo::error=unsupported Metal shader target: target_os={target_os}, target_env={target_env}"
                );
                process::exit(1);
            }
        };

        let shader_path = generated.join("src/shaders.metal");
        let air_output_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("shaders.air");
        let metallib_output_path =
            PathBuf::from(env::var("OUT_DIR").unwrap()).join("shaders.metallib");

        // The metal compiler records the resolved absolute path of its input
        // unconditionally. Compile a copy staged in OUT_DIR so the recorded
        // location is the build's canonical output directory, never the
        // checkout (corgi rejects artifacts that embed the build path).
        let staged_shader_path = PathBuf::from(env::var("OUT_DIR").unwrap()).join("shaders.metal");
        std::fs::copy(shader_path, &staged_shader_path).unwrap();

        let output = Command::new("xcrun")
            .args([
                "-sdk",
                sdk,
                "metal",
                "-gline-tables-only",
                minimum_version_argument,
                "-MO",
                "-c",
            ])
            .arg(&staged_shader_path)
            .args(["-include", header_path.to_str().unwrap(), "-o"])
            .arg(&air_output_path)
            .output()
            .unwrap();

        if !output.status.success() {
            println!(
                "cargo::error=metal shader compilation failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            process::exit(1);
        }

        let output = Command::new("xcrun")
            .args(["-sdk", sdk, "metallib"])
            .arg(&air_output_path)
            .arg("-o")
            .arg(metallib_output_path)
            .output()
            .unwrap();

        if !output.status.success() {
            println!(
                "cargo::error=metallib compilation failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            process::exit(1);
        }

        // The .air intermediate records the compiler's working directory in
        // its debug info; the metallib built from it does not. Nothing reads
        // the .air after this point, so drop it rather than leave a
        // checkout-path-bearing file in OUT_DIR.
        std::fs::remove_file(&air_output_path).unwrap();
    }
}
