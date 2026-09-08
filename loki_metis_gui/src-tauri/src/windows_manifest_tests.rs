//! 锁定 Windows 测试可执行文件所需的 Common Controls v6 清单接线。

/// Windows MSVC 测试目标必须显式嵌入 Common Controls v6 清单。
#[cfg(all(target_os = "windows", target_env = "msvc"))]
#[test]
fn windows_test_targets_embed_common_controls_v6_manifest() {
    let build_script = include_str!("../build.rs");
    let manifest = include_str!("../windows/app.manifest");

    assert!(build_script.contains("cargo:rustc-link-arg=/MANIFEST:EMBED"));
    assert!(build_script.contains("cargo:rustc-link-arg=/MANIFESTINPUT:"));
    assert!(build_script.contains("WindowsAttributes::new_without_app_manifest()"));
    assert!(manifest.contains("name=\"com.manonloki.lokimetis\""));
    assert!(manifest.contains("version=\"1.0.0.0\""));
    assert!(manifest.contains("processorArchitecture=\"amd64\""));
    assert!(manifest.contains("Microsoft.Windows.Common-Controls"));
    assert!(manifest.contains("version=\"6.0.0.0\""));
}
