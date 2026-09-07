//! 通过 Tauri 的 Resource 基目录定位随应用打包的只读资源。
//!
//! 正式包只接受 Tauri 的官方解析结果；开发构建在 Tauri 无法识别自定义
//! Cargo target 根时，才回退到 Tauri CLI 已复制到可执行文件旁的资源。

use std::path::{Component, Path, PathBuf};

use tauri::{Manager, Runtime, path::BaseDirectory};

/// 使用 Tauri 官方 Resource 基目录解析资源，并兼容自定义 target-dir 的开发输出。
pub(crate) fn resolve_bundled_resource<R: Runtime>(
    manager: &impl Manager<R>,
    relative_path: impl AsRef<Path>,
) -> tauri::Result<PathBuf> {
    let relative_path = relative_path.as_ref();
    match manager
        .path()
        .resolve(relative_path, BaseDirectory::Resource)
    {
        Ok(path) => Ok(path),
        Err(tauri::Error::UnknownPath) if cfg!(debug_assertions) => {
            development_resource_next_to_executable(relative_path).ok_or(tauri::Error::UnknownPath)
        }
        Err(error) => Err(error),
    }
}

/// 只接受普通相对组件，避免开发态回退越过 Tauri 已复制的资源根。
fn development_resource_next_to_executable(relative_path: &Path) -> Option<PathBuf> {
    if relative_path.as_os_str().is_empty()
        || relative_path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }
    let executable = std::env::current_exe().ok()?;
    development_resource_for_executable(&executable, relative_path)
}

/// 从可注入的开发二进制位置定位 Tauri CLI 已复制的相邻资源。
fn development_resource_for_executable(executable: &Path, relative_path: &Path) -> Option<PathBuf> {
    let candidate = executable.parent()?.join(relative_path);
    candidate.exists().then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// 自定义 Cargo target 根不叫 `target` 时仍能读取 Tauri CLI 复制的资源。
    #[test]
    fn custom_target_directory_uses_tauri_copied_adjacent_resource() {
        let root = tempdir().expect("temp");
        let executable = root.path().join("cargo-target/debug/loki_metis_gui");
        let resource = executable
            .parent()
            .expect("binary parent")
            .join("builtin-skins");
        std::fs::create_dir_all(&resource).expect("resource directory");

        assert_eq!(
            development_resource_for_executable(&executable, Path::new("builtin-skins")),
            Some(resource)
        );
    }

    /// 缺失的相邻目录不能伪装成已经打包的资源。
    #[test]
    fn missing_adjacent_resource_is_rejected() {
        let root = tempdir().expect("temp");
        let executable = root.path().join("cargo-target/debug/loki_metis_gui");

        assert_eq!(
            development_resource_for_executable(&executable, Path::new("builtin-skins")),
            None
        );
    }
}
