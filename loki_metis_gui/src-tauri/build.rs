use std::{env, path::PathBuf};

/// 为 Windows MSVC 可执行目标补上 Common Controls v6 清单，避免测试进程在入口点加载阶段失败。
fn emit_windows_executable_manifest_link_args() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }

    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"))
        .join("windows/app.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
}

/// 生成 Tauri 资源；Windows 清单由统一链接参数嵌入，资源库只保留图标与版本信息。
fn build_tauri_resources() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        let attributes = tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        tauri_build::try_build(attributes).expect("Tauri resource generation");
    } else {
        tauri_build::build();
    }
}

/// 生成 Tauri 资源，并为 Cargo 创建的 Windows 可执行目标补齐运行时清单。
fn main() {
    emit_windows_executable_manifest_link_args();
    build_tauri_resources();
}
