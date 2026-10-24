//! 运行时资源只来自安装包；源码资源只能由开发入口或隔离测试明确注入。

use crate::skins::SkinError;
use std::path::{Path, PathBuf};

/// 正式二进制不依赖构建机器目录，也不接受开发环境变量覆盖包内资源。
pub fn resource_root() -> Result<PathBuf, SkinError> {
    let executable = std::env::current_exe().map_err(|_| missing_resources())?;
    // 测试与 GUI 测试（test-support）使用本 crate 源码资源；正式构建绝不读取构建机目录。
    #[cfg(any(test, feature = "test-support"))]
    let development = Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources"));
    #[cfg(not(any(test, feature = "test-support")))]
    let development = if cfg!(debug_assertions) {
        std::env::var_os("LOKI_METRIS_DEV_SKIN_RESOURCES").map(PathBuf::from)
    } else {
        None
    };
    locate(
        &executable,
        cfg!(target_os = "macos"),
        development.as_deref(),
    )
}

/// macOS与Windows安装布局有独立根，缺少必需目录返回可见错误。
fn locate(
    executable: &Path,
    macos: bool,
    development: Option<&Path>,
) -> Result<PathBuf, SkinError> {
    let parent = executable.parent().ok_or_else(missing_resources)?;
    let installed = parent.join(if macos {
        "../Resources/skins"
    } else {
        "resources/skins"
    });
    for candidate in std::iter::once(installed.as_path()).chain(development) {
        if candidate.join("builtin-skins").is_dir()
            && candidate.join("codex-skin-generator").is_dir()
        {
            return candidate.canonicalize().map_err(|_| missing_resources());
        }
    }
    Err(missing_resources())
}

/// 不将缺失安装资源解释为空皮肤库。
fn missing_resources() -> SkinError {
    SkinError::new(
        "skin.resources_missing",
        "应用皮肤资源缺失，请重新安装完整应用。",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 各平台只按安装布局定位，Linux便携包与Debian包同样不依赖源码目录。
    fn installed_resources_resolve_without_source_tree() {
        let root = tempfile::tempdir().unwrap();
        for (macos, executable, resources) in [
            (
                true,
                "LokiMetris.app/Contents/MacOS/loki_metris_gui",
                "LokiMetris.app/Contents/Resources/skins",
            ),
            (
                false,
                "windows/loki_metris_gui.exe",
                "windows/resources/skins",
            ),
            (
                false,
                "LokiMetris/loki_metris_gui",
                "LokiMetris/resources/skins",
            ),
            (
                false,
                "opt/com.loki-metris.app/loki_metris_gui",
                "opt/com.loki-metris.app/resources/skins",
            ),
        ] {
            let binary = root.path().join(executable);
            let expected = root.path().join(resources);
            std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
            for folder in ["builtin-skins", "codex-skin-generator"] {
                std::fs::create_dir_all(expected.join(folder)).unwrap();
            }
            assert_eq!(
                locate(&binary, macos, None).unwrap(),
                expected.canonicalize().unwrap()
            );
            std::fs::remove_dir_all(expected.join("builtin-skins")).unwrap();
            assert_eq!(
                locate(&binary, macos, None).unwrap_err().code,
                "skin.resources_missing"
            );
        }
    }

    #[test]
    /// 开发路径必须由调用方提供，普通安装缺失时不会静默回退源码。
    fn development_resource_root_requires_explicit_injection() {
        let root = tempfile::tempdir().unwrap();
        let development = root.path().join("source/resources");
        for folder in ["builtin-skins", "codex-skin-generator"] {
            std::fs::create_dir_all(development.join(folder)).unwrap();
        }
        let binary = root.path().join("bin/app");
        assert!(locate(&binary, false, None).is_err());
        assert_eq!(
            locate(&binary, false, Some(&development)).unwrap(),
            development.canonicalize().unwrap()
        );
    }
}
