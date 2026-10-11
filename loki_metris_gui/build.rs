//! 只消费候选工具已校验并生成的静态发布条目；普通开发构建使用真实空状态。

use std::{env, fs, path::PathBuf};

/// Windows 使用已有 SDK 的资源编译器，把产品 ICO 写入 GPUI 读取的资源 #1。
fn embed_windows_icon() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") { return; }
    let icon = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../packaging/icons/app.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    for variable in ["RC", "WindowsSdkDir", "ProgramFiles(x86)", "PATH"] {
        println!("cargo:rerun-if-env-changed={variable}");
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::copy(icon, output.join("app.ico")).expect("无法复制 Windows 应用图标");
    fs::write(output.join("app-icon.rc"), "1 ICON \"app.ico\"\n").unwrap();
    let resource = output.join("app-icon.res");
    let compiler = resource_compiler().expect("Windows SDK rc.exe 不可用，不能构建缺失正式图标的应用");
    let status = std::process::Command::new(compiler).current_dir(&output)
        .arg("/nologo").arg("/fo").arg(&resource).arg("app-icon.rc").status()
        .expect("无法运行 Windows SDK 资源编译器");
    assert!(status.success(), "Windows 应用图标编译失败");
    println!("cargo:rustc-link-arg-bin=loki_metris_gui={}", resource.display());
}

/// 优先尊重显式工具及 PATH，其后从现有 SDK 中按数值版本寻找本宿主的 rc.exe。
fn resource_compiler() -> Option<PathBuf> {
    if let Some(path) = env::var_os("RC") { return Some(path.into()); }
    if let Some(paths) = env::var_os("PATH") {
        if let Some(path) = env::split_paths(&paths).map(|path| path.join("rc.exe")).find(|path| path.is_file()) { return Some(path); }
    }
    let root = env::var_os("WindowsSdkDir").map(PathBuf::from).or_else(||
        env::var_os("ProgramFiles(x86)").map(|path| PathBuf::from(path).join("Windows Kits/10")))?;
    let architecture = if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" };
    let mut versions = fs::read_dir(root.join("bin")).ok()?.filter_map(Result::ok).filter_map(|entry| {
        let name = entry.file_name().into_string().ok()?;
        let version = name.split('.').map(str::parse::<u32>).collect::<Result<Vec<_>, _>>().ok()?;
        let path = entry.path().join(architecture).join("rc.exe");
        path.is_file().then_some((version, path))
    }).collect::<Vec<_>>();
    versions.sort_by(|a, b| b.0.cmp(&a.0));
    versions.into_iter().next().map(|(_, path)| path)
}

/// Cargo 在选择或内容变化时重新编译，更新日志不写回源码也不在窗口线程读取。
fn main() {
    embed_windows_icon();
    const INPUT: &str = "HARNESS_GPUI_RELEASE_NOTES_RS";
    println!("cargo:rerun-if-env-changed={INPUT}");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo 必须提供 OUT_DIR")).join("release_notes.rs");
    if let Some(input) = env::var_os(INPUT) {
        let input = PathBuf::from(input);
        assert!(input.is_absolute(), "候选静态更新日志必须使用绝对路径");
        let text = input.to_str().expect("候选静态更新日志路径必须是 UTF-8");
        assert!(!text.chars().any(char::is_control), "候选静态更新日志路径不得包含控制字符");
        let metadata = fs::symlink_metadata(&input).expect("候选静态更新日志必须存在");
        assert!(metadata.is_file() && metadata.len() <= 8 * 1024 * 1024, "候选静态更新日志必须是有界普通文件");
        println!("cargo:rerun-if-changed={text}");
        fs::copy(&input, output).expect("无法嵌入已验证的候选更新日志");
    } else {
        fs::write(output, "pub const RELEASE_NOTES_JSON: &[u8] = &[];\npub const RELEASE_NOTES: &[(&str, &str, &[(&str, &str)], &[(&str, &str)])] = &[];\n")
            .expect("无法建立空更新日志状态");
    }
}
