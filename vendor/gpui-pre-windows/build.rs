#![allow(clippy::disallowed_methods, reason = "build scripts are exempt")]

#[path = "../gpui-patch-support/generator.rs"]
mod generator;
use generator::{Patch, Spec};
use std::path::{Path, PathBuf};

#[cfg(all(target_os = "windows", not(debug_assertions)))]
mod fxc_path;

/// 所有上游文件和补丁结果有固定 SHA，编译阶段仅生成独立 OUT_DIR 镜像。
fn generated_sources() -> PathBuf {
    let spec = Spec {
        entry: "src/gpui_windows.rs",
        root_prefix: include_str!("patches/root-prefix.before"),
        source_manifest_sha256: "67ceabff1707a47d93fb10d594fa27a8d6928f945cb839a9d958f971bdf7a9df",
        patches: &[
            Patch {
                name: "hlsl-clip-layout", path: "src/shaders.hlsl",
                before: include_str!("patches/hlsl-clip-layout.before"),
                after: include_str!("patches/hlsl-clip-layout.after"),
            },
            Patch {
                name: "hlsl-clip-distance", path: "src/shaders.hlsl",
                before: include_str!("patches/hlsl-clip-distance.before"),
                after: include_str!("patches/hlsl-clip-distance.after"),
            },
            Patch {
                name: "debug-generated-shader-path", path: "src/directx_renderer.rs",
                before: include_str!("patches/debug-generated-shader-path.before"),
                after: include_str!("patches/debug-generated-shader-path.after"),
            },
            Patch {
                name: "live-resize-state", path: "src/events.rs",
                before: include_str!("patches/live-resize-state.before"),
                after: include_str!("patches/live-resize-state.after"),
            },
            Patch {
                name: "live-resize-hit", path: "src/events.rs",
                before: include_str!("patches/live-resize-hit.before"),
                after: include_str!("patches/live-resize-hit.after"),
            },
            Patch {
                name: "live-maximize-hit", path: "src/events.rs",
                before: include_str!("patches/live-maximize-hit.before"),
                after: include_str!("patches/live-maximize-hit.after"),
            },
            Patch {
                name: "live-maximize-click", path: "src/events.rs",
                before: include_str!("patches/live-maximize-click.before"),
                after: include_str!("patches/live-maximize-click.after"),
            },
            Patch {
                name: "external-show-state", path: "src/window.rs",
                before: include_str!("patches/external-show-state.before"),
                after: include_str!("patches/external-show-state.after"),
            },
            Patch {
                name: "external-show-observe", path: "src/events.rs",
                before: include_str!("patches/external-show-observe.before"),
                after: include_str!("patches/external-show-observe.after"),
            },
        ],
        generated_hashes: &[
            ("src/window.rs", "5c7ead7e6f6d64909889c81553b20b39dd47ad301e4401788b22cb2a69f759e1"),
            ("src/events.rs", "df24c0504a0f16045fc9165bfc14d0894126399c3a15e124997dca6d80f77ebf"),
            ("src/gpui_windows.rs", "a4dbf8bbefd89e8ac5996a143e82c8e2eef65e1675c25ab6d27607329c041a67"),
            ("src/shaders.hlsl", "2bfb2dc989eec4762ee43966c296604ab718a9569fdcf917e8e772106b6c6764"),
            ("src/directx_renderer.rs", "79d4846367944a35a659a6e529245fb5a49fe860ec8fa51d7878b76a31ef89a9"),
        ],
    };
    generator::generate(Path::new(env!("CARGO_MANIFEST_DIR")), &spec)
}

/// 所有配置先验证源码；Windows release 再从生成树编译 HLSL。
fn main() {
    let _generated = generated_sources();
    #[cfg(target_os = "windows")]
    {
        // Compile HLSL shaders
        #[cfg(not(debug_assertions))]
        compile_shaders(&_generated);
    }
}

#[cfg(all(target_os = "windows", not(debug_assertions)))]
mod shader_compilation {
    use std::{
        fs,
        io::Write,
        path::{Path, PathBuf},
        process::{self, Command},
    };

    /// Windows SDK 编译器及绑定生成流程不变，输入改为已验证的生成 HLSL。
    pub fn compile_shaders(generated: &Path) {
        let shader_path = generated.join("src/shaders.hlsl");
        let out_dir = std::env::var("OUT_DIR").unwrap();

        println!("cargo:rerun-if-changed={}", shader_path.display());

        // Check if fxc.exe is available
        let fxc_path = find_fxc_compiler();

        // Define all modules
        let modules = [
            "quad",
            "shadow",
            "path_rasterization",
            "path_sprite",
            "underline",
            "monochrome_sprite",
            "subpixel_sprite",
            "polychrome_sprite",
        ];

        let rust_binding_path = format!("{}/shaders_bytes.rs", out_dir);
        if Path::new(&rust_binding_path).exists() {
            fs::remove_file(&rust_binding_path)
                .expect("Failed to remove existing Rust binding file");
        }
        for module in modules {
            compile_shader_for_module(
                module,
                &out_dir,
                &fxc_path,
                shader_path.to_str().unwrap(),
                &rust_binding_path,
            );
        }

        {
            let shader_path = generated.join("src/color_text_raster.hlsl");
            compile_shader_for_module(
                "emoji_rasterization",
                &out_dir,
                &fxc_path,
                shader_path.to_str().unwrap(),
                &rust_binding_path,
            );
        }
    }

    /// Locate `binary` in the newest installed Windows SDK.
    /// 按版本顺序查找当前架构的 SDK 工具，不依赖全局包管理器。
    pub fn find_latest_windows_sdk_binary(
        binary: &str,
    ) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
        let key = windows_registry::LOCAL_MACHINE
            .open("SOFTWARE\\WOW6432Node\\Microsoft\\Microsoft SDKs\\Windows\\v10.0")?;

        let install_folder: String = key.get_string("InstallationFolder")?; // "C:\Program Files (x86)\Windows Kits\10\"
        let install_folder_bin = Path::new(&install_folder).join("bin");

        let mut versions: Vec<_> = std::fs::read_dir(&install_folder_bin)?
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();

        versions.sort_by_key(|s| {
            s.split('.')
                .filter_map(|p| p.parse().ok())
                .collect::<Vec<u32>>()
        });

        let arch = match std::env::consts::ARCH {
            "x86_64" => "x64",
            "aarch64" => "arm64",
            _ => Err(format!(
                "Unsupported architecture: {}",
                std::env::consts::ARCH
            ))?,
        };

        if let Some(highest_version) = versions.last() {
            return Ok(Some(
                install_folder_bin
                    .join(highest_version)
                    .join(arch)
                    .join(binary),
            ));
        }

        Ok(None)
    }

    /// You can set the `GPUI_FXC_PATH` environment variable to specify the path to the fxc.exe compiler.
    /// 复用 PATH 或已安装 Windows SDK 的 fxc，缺失仍按上游报错。
    fn find_fxc_compiler() -> PathBuf {
        // Check environment variable
        if let Some(path) = std::env::var_os("GPUI_FXC_PATH")
            && Path::new(&path).is_file()
        {
            return path.into();
        }

        if let Some(search_path) = std::env::var_os("PATH")
            && let Some(path) = super::fxc_path::find_fxc_in_path(&search_path)
        {
            return path;
        }

        if let Ok(Some(path)) = find_latest_windows_sdk_binary("fxc.exe")
            && path.is_file()
        {
            return path;
        }

        panic!("Failed to find fxc.exe");
    }

    /// 每个模块生成对应顶点与片段 shader 常量。
    fn compile_shader_for_module(
        module: &str,
        out_dir: &str,
        fxc_path: &Path,
        shader_path: &str,
        rust_binding_path: &str,
    ) {
        // Compile vertex shader
        let output_file = format!("{}/{}_vs.h", out_dir, module);
        let const_name = format!("{}_VERTEX_BYTES", module.to_uppercase());
        compile_shader_impl(
            fxc_path,
            &format!("{module}_vertex"),
            &output_file,
            &const_name,
            shader_path,
            "vs_4_1",
        );
        generate_rust_binding(&const_name, &output_file, rust_binding_path);

        // Compile fragment shader
        let output_file = format!("{}/{}_ps.h", out_dir, module);
        let const_name = format!("{}_FRAGMENT_BYTES", module.to_uppercase());
        compile_shader_impl(
            fxc_path,
            &format!("{module}_fragment"),
            &output_file,
            &const_name,
            shader_path,
            "ps_4_1",
        );
        generate_rust_binding(&const_name, &output_file, rust_binding_path);
    }

    /// 只有编译器成功输出才接受 HLSL 字节，保留明确的失败诊断。
    fn compile_shader_impl(
        fxc_path: &Path,
        entry_point: &str,
        output_path: &str,
        var_name: &str,
        shader_path: &str,
        target: &str,
    ) {
        let output = Command::new(fxc_path)
            .args([
                "/T",
                target,
                "/E",
                entry_point,
                "/Fh",
                output_path,
                "/Vn",
                var_name,
                "/O3",
                shader_path,
            ])
            .output();

        match output {
            Ok(result) => {
                if result.status.success() {
                    return;
                }
                println!(
                    "cargo::error=Shader compilation failed for {}:\n{}",
                    entry_point,
                    String::from_utf8_lossy(&result.stderr)
                );
                process::exit(1);
            }
            Err(e) => {
                println!("cargo::error=Failed to run fxc for {}: {}", entry_point, e);
                process::exit(1);
            }
        }
    }

    /// 将 fxc 的 C 字节数组转换为 OUT_DIR 的 Rust 常量。
    fn generate_rust_binding(const_name: &str, head_file: &str, output_path: &str) {
        let header_content = fs::read_to_string(head_file).expect("Failed to read header file");
        let const_definition = {
            let global_var_start = header_content.find("const BYTE").unwrap();
            let global_var = &header_content[global_var_start..];
            let equal = global_var.find('=').unwrap();
            global_var[equal + 1..].trim()
        };
        let rust_binding = format!(
            "const {}: &[u8] = &{}\n",
            const_name,
            const_definition.replace('{', "[").replace('}', "]")
        );
        let mut options = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(output_path)
            .expect("Failed to open Rust binding file");
        options
            .write_all(rust_binding.as_bytes())
            .expect("Failed to write Rust binding file");
    }
}

#[cfg(all(target_os = "windows", not(debug_assertions)))]
use shader_compilation::compile_shaders;
