#![allow(clippy::disallowed_methods, reason = "build scripts are exempt")]

#[path = "../gpui-patch-support/generator.rs"]
#[macro_use]
mod generator;
use generator::{Patch, Spec};
#[path = "../gpui-patch-support/scene_patches.rs"]
mod scene_patches;
#[path = "src/metal_patches.rs"]
mod metal_patches;
#[path = "src/font_patches.rs"]
mod font_patches;
mod fixture_verify;
use std::path::{Path, PathBuf};

/// 所有上游文件和补丁结果有固定 SHA，编译阶段仅生成独立 OUT_DIR 镜像。
fn generated_sources() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for name in [
        "src/variable_font.rs",
        "src/semantic_identity.rs",
        "src/descriptor_identity.rs",
        "src/variable_font_tests.rs",
        "src/variable_font_tests_fallback.rs",
        "src/descriptor_identity_tests.rs",
        "fixture_verify.rs",
        "test-fixtures",
        "test-fixtures.sha256",
    ] {
        println!("cargo:rerun-if-changed={name}");
    }
    // 可变字体回归使用的九份原样字体/OFL 先逐字节核验，再生成补丁树。
    fixture_verify::verify(root).unwrap_or_else(|error| panic!("{error}"));
    // scene.rs 补丁与 gpui-pre 共用；Metal 与可变字体为本包专属。
    let patches: Vec<Patch> = [
        &scene_patches::all("vendor/gpui/src/scene.rs")[..],
        metal_patches::PATCHES,
        font_patches::PATCHES,
    ].concat();
    let spec = Spec {
        entry: "src/gpui_apple.rs",
        root_prefix: include_str!("patches/root-prefix.before"),
        source_manifest_sha256: "0a61d4ba3284175c7298773cdb5c0cbade41b9ea4c047903aeb71a026c588800",
        patches: &patches,
        generated_hashes: &[
            ("src/metal_atlas.rs", "fe35252e935046f46b06666a58be7dc026c4c1fa90f9080dc958ff97d766026e"),
            ("src/metal_renderer.rs", "052f4a7bf667740fa2f3eb05cec31ed49202b03848a0d59f754cddd3e104191c"),
            ("src/gpui_apple.rs", "8ca1035aabe88d7acda54db0bf5edda25bd75439f888f5626fbd276c6c5887cf"),
            ("vendor/gpui/src/scene.rs", "3c2e1598169001ed10a3b99df37718619558fe4c192f7a5423ced9ae1762ca9b"),
            ("src/shaders.metal", "0293a36fef5f5bbde11fcad44ac903c6d70bb6c83c9db86c26a831f6ae53da3c"),
            ("src/text_system.rs", "8be80d1891386c7724f8cff160277468a2bd947cbd43f31dc1a4f5ee8fc4b8b3"),
        ],
    };
    generator::generate(root, &spec)
}

/// 验证原包并生成全树后，才运行平台原有的 shader 构建。
fn main() {
    let _generated = generated_sources();
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    apple_build::run(&_generated);
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple_build {
    #[cfg(not(feature = "runtime_shaders"))]
    use std::ffi::OsString;
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

        println!("cargo:rerun-if-env-changed=SDKROOT");
        println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");

        // Build scripts run on the host, so the target platform must come from
        // Cargo's environment rather than `cfg!`. The target environment, not the
        // target name, identifies simulators: `x86_64-apple-ios` has no `-sim` suffix.
        let target_os = env::var("CARGO_CFG_TARGET_OS")
            .expect("Cargo sets CARGO_CFG_TARGET_OS for build scripts");
        let target_env = env::var("CARGO_CFG_TARGET_ENV")
            .expect("Cargo sets CARGO_CFG_TARGET_ENV for build scripts");
        let (sdk, minimum_version_argument) = match (target_os.as_str(), target_env.as_str()) {
            ("macos", _) => ("macosx", "-mmacosx-version-min=12.0"),
            ("ios", "sim") => ("iphonesimulator", "-mios-simulator-version-min=15.0"),
            ("ios", "") => ("iphoneos", "-mios-version-min=15.0"),
            _ => {
                println!(
                    "cargo::error=unsupported Metal shader target: target_os={target_os}, target_env={target_env}"
                );
                process::exit(1);
            }
        };

        let sdk = if target_os == "macos" {
            env::var_os("SDKROOT").unwrap_or_else(|| OsString::from(sdk))
        } else {
            OsString::from(sdk)
        };

        let shader_path = generated.join("src/shaders.metal");
        let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
        let air_output_path = out_dir.join("shaders.air");
        let metallib_output_path = out_dir.join("shaders.metallib");
        println!("cargo:rerun-if-changed={}", shader_path.display());

        // The metal compiler records the resolved absolute path of its input
        // unconditionally. Compile a copy staged in OUT_DIR so the recorded
        // location is the build's canonical output directory, never the
        // checkout (corgi rejects artifacts that embed the build path).
        let staged_shader_path = out_dir.join("shaders.metal");
        std::fs::copy(shader_path, &staged_shader_path).unwrap();

        // When targeting macOS 12+, both `metal` and `metallib` also record
        // their working directory in the metallib. Run them from OUT_DIR so
        // that path isn't the checkout either.
        let output = Command::new("xcrun")
            .current_dir(&out_dir)
            .arg("-sdk")
            .arg(&sdk)
            .args([
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
            .current_dir(&out_dir)
            .arg("-sdk")
            .arg(&sdk)
            .arg("metallib")
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

        // Nothing reads the .air intermediate after this point.
        std::fs::remove_file(&air_output_path).unwrap();
    }
}
