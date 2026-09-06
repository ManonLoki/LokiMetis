//! Windows 宿主访问 WSL Hook 配置的窄适配层；不承担普通本机文件读写。

use std::path::Path;

#[cfg(any(target_os = "windows", test))]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(target_os = "windows")]
use std::{
    io::Write,
    process::{Command, Stdio},
};

use loki_metis_core::HookError;

/// 同一进程并发生成 WSL 临时文件时的唯一序号。
#[cfg(any(target_os = "windows", test))]
static WSL_TEMPORARY_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 从 Windows WSL UNC 路径解析出的发行版与 Linux 目录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WslDirectory {
    distribution: String,
    linux_path: String,
}

/// WSL 发行版内一个可读写的确定文件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WslFile {
    distribution: String,
    linux_path: String,
}

impl WslDirectory {
    /// 识别 `\\wsl.localhost\发行版\...` 与 `\\wsl$\发行版\...`，并拒绝路径穿越。
    pub(super) fn parse(directory: &str) -> Option<Self> {
        let normalized = directory.replace('/', "\\");
        let lowercase = normalized.to_ascii_lowercase();
        let prefix_length = ["\\\\wsl.localhost\\", "\\\\wsl$\\"]
            .into_iter()
            .find(|prefix| lowercase.starts_with(prefix))?
            .len();
        let mut components = normalized[prefix_length..]
            .split('\\')
            .filter(|component| !component.is_empty());
        let distribution = components.next()?.to_owned();
        let remainder = components.collect::<Vec<_>>();
        if distribution == "."
            || distribution == ".."
            || remainder
                .iter()
                .any(|component| matches!(*component, "." | ".."))
        {
            return None;
        }
        Some(Self {
            distribution,
            linux_path: format!("/{}", remainder.join("/")),
        })
    }

    /// 在已解析目录下拼接由受信任协议提供的相对配置文件名。
    pub(super) fn join(&self, relative_path: &str) -> WslFile {
        let relative_path = relative_path.replace('\\', "/");
        WslFile {
            distribution: self.distribution.clone(),
            linux_path: format!(
                "{}/{}",
                self.linux_path.trim_end_matches('/'),
                relative_path.trim_start_matches('/')
            ),
        }
    }

    /// 通过目标发行版的 `wslpath` 把 Windows relay 路径转换成 Linux 可执行路径。
    #[cfg(target_os = "windows")]
    pub(super) fn translate_windows_executable(
        &self,
        executable: &Path,
    ) -> Result<String, HookError> {
        let executable = executable.to_str().ok_or_else(|| {
            HookError::new("error.hooks.writeFailed")
                .param("detail", "relay executable path is not Unicode")
        })?;
        let normalized = wslpath_input(executable);
        let output = run_wsl(&self.distribution, ["wslpath", "-u", normalized.as_str()])?;
        let translated = String::from_utf8(output).map_err(|error| {
            HookError::new("error.hooks.writeFailed").param("detail", error.to_string())
        })?;
        let translated = translated.trim();
        if translated.is_empty() || !translated.starts_with('/') {
            return Err(HookError::new("error.hooks.writeFailed")
                .param("detail", "wslpath returned a non-absolute path"));
        }
        Ok(translated.to_owned())
    }

    /// 非 Windows 构建保留同一接口，但明确拒绝真正访问 WSL。
    #[cfg(not(target_os = "windows"))]
    pub(super) fn translate_windows_executable(
        &self,
        _executable: &Path,
    ) -> Result<String, HookError> {
        let _ = self;
        Err(HookError::new("error.hooks.wslWindowsHostOnly"))
    }
}

/// 归一化传给 `wslpath` 的 Windows 路径，避免反斜杠被第二层命令解析吞掉。
#[cfg(any(target_os = "windows", test))]
fn wslpath_input(executable: &str) -> String {
    executable.replace('\\', "/")
}

impl WslFile {
    /// 返回不含 Windows 用户目录的 Linux 侧路径，供结构化错误定位目标。
    pub(super) fn display(&self) -> &str {
        &self.linux_path
    }

    /// 读取 WSL 内配置；文件不存在是正常的首次写入状态。
    #[cfg(target_os = "windows")]
    pub(super) fn read_optional(&self) -> Result<Option<String>, HookError> {
        let output = Command::new("wsl.exe")
            .args([
                "-d",
                &self.distribution,
                "--",
                "cat",
                "--",
                &self.linux_path,
            ])
            .output()
            .map_err(|error| wsl_launch_error(&self.distribution, &error))?;
        if output.status.success() {
            return String::from_utf8(output.stdout).map(Some).map_err(|error| {
                HookError::new("error.hooks.existingReadFailed").param("detail", error.to_string())
            });
        }

        let existence = Command::new("wsl.exe")
            .args([
                "-d",
                &self.distribution,
                "--",
                "test",
                "-e",
                &self.linux_path,
            ])
            .output()
            .map_err(|error| wsl_launch_error(&self.distribution, &error))?;
        if wsl_test_reports_missing(existence.status.code()) {
            return Ok(None);
        }
        Err(wsl_command_error(
            &self.distribution,
            "error.hooks.existingReadFailed",
            &output.stderr,
        ))
    }

    /// 非 Windows 构建不会尝试把 UNC 当作本机路径访问。
    #[cfg(not(target_os = "windows"))]
    pub(super) fn read_optional(&self) -> Result<Option<String>, HookError> {
        let _ = self;
        Err(HookError::new("error.hooks.wslWindowsHostOnly"))
    }

    /// 在 WSL 内用同目录临时文件和 `mv` 原子替换目标配置。
    #[cfg(target_os = "windows")]
    pub(super) fn write_atomic(&self, content: &str) -> Result<(), HookError> {
        let parent = self
            .linux_path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .filter(|parent| !parent.is_empty())
            .ok_or_else(|| {
                HookError::new("error.hooks.writeFailed")
                    .param("detail", "cannot determine WSL parent directory")
            })?;
        run_wsl(&self.distribution, ["mkdir", "-p", "--", parent])?;

        let temporary_path = unique_temporary_path(&self.linux_path);
        let mut child = Command::new("wsl.exe")
            .args(["-d", &self.distribution, "--", "tee", &temporary_path])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| wsl_launch_error(&self.distribution, &error))?;
        child
            .stdin
            .take()
            .ok_or_else(|| {
                HookError::new("error.hooks.writeFailed")
                    .param("detail", "cannot open WSL write channel")
            })?
            .write_all(content.as_bytes())
            .map_err(|error| {
                HookError::new("error.hooks.writeFailed").param("detail", error.to_string())
            })?;
        let output = child.wait_with_output().map_err(|error| {
            HookError::new("error.hooks.writeFailed").param("detail", error.to_string())
        })?;
        if !output.status.success() {
            return Err(wsl_command_error(
                &self.distribution,
                "error.hooks.writeFailed",
                &output.stderr,
            ));
        }

        if let Err(error) = run_wsl(
            &self.distribution,
            ["mv", "-f", "--", &temporary_path, &self.linux_path],
        ) {
            let _ = run_wsl(&self.distribution, ["rm", "-f", "--", &temporary_path]);
            return Err(error);
        }
        Ok(())
    }

    /// 非 Windows 构建不会启动 WSL 子进程。
    #[cfg(not(target_os = "windows"))]
    pub(super) fn write_atomic(&self, _content: &str) -> Result<(), HookError> {
        let _ = self;
        Err(HookError::new("error.hooks.wslWindowsHostOnly"))
    }
}

/// 为 WSL 同目录原子替换生成 PID 与进程内序号都唯一的临时路径。
#[cfg(any(target_os = "windows", test))]
fn unique_temporary_path(target: &str) -> String {
    format!(
        "{target}.lokimetis.{}.{}.tmp",
        std::process::id(),
        WSL_TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// 判断 POSIX `test -e` 的退出码是否明确表示目标不存在。
#[cfg(any(target_os = "windows", test))]
fn wsl_test_reports_missing(status_code: Option<i32>) -> bool {
    status_code == Some(1)
}

/// 在指定发行版内运行一个不经 shell 插值的文件操作。
#[cfg(target_os = "windows")]
fn run_wsl<const N: usize>(distribution: &str, command: [&str; N]) -> Result<Vec<u8>, HookError> {
    let output = Command::new("wsl.exe")
        .args(["-d", distribution, "--"])
        .args(command)
        .output()
        .map_err(|error| wsl_launch_error(distribution, &error))?;
    if !output.status.success() {
        return Err(wsl_command_error(
            distribution,
            "error.hooks.writeFailed",
            &output.stderr,
        ));
    }
    Ok(output.stdout)
}

/// 把无法启动 `wsl.exe` 映射为稳定 Hook 错误。
#[cfg(target_os = "windows")]
fn wsl_launch_error(distribution: &str, error: &std::io::Error) -> HookError {
    HookError::new("error.hooks.writeFailed")
        .param("distribution", distribution)
        .param("detail", error.to_string())
}

/// 把 WSL 命令失败映射为稳定 Hook 错误，并保留足够的本机诊断。
#[cfg(target_os = "windows")]
fn wsl_command_error(distribution: &str, code: &'static str, stderr: &[u8]) -> HookError {
    HookError::new(code)
        .param("distribution", distribution)
        .param("detail", String::from_utf8_lossy(stderr).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 两种 WSL UNC 前缀都能解析并安全拼接嵌套辅助文件。
    fn recognizes_both_wsl_unc_prefixes_and_nested_files() {
        let modern = WslDirectory::parse(r"\\wsl.localhost\Ubuntu-24.04\home\user\.openclaw")
            .expect("modern WSL UNC");
        assert_eq!(
            modern.join("extensions/lokimetis/package.json").display(),
            "/home/user/.openclaw/extensions/lokimetis/package.json"
        );
        let legacy = WslDirectory::parse(r"\\wsl$\Arch\home\user\.codex").expect("legacy WSL UNC");
        assert_eq!(
            legacy.join("hooks.json").display(),
            "/home/user/.codex/hooks.json"
        );
    }

    #[test]
    /// 不完整、穿越或普通网络 UNC 路径不会被误判为 WSL 目录。
    fn rejects_incomplete_or_traversing_wsl_unc_paths() {
        assert!(WslDirectory::parse(r"\\wsl.localhost\").is_none());
        assert!(WslDirectory::parse(r"\\wsl$\Ubuntu\home\..\.codex").is_none());
        assert!(WslDirectory::parse(r"\\server\share\.codex").is_none());
    }

    #[test]
    /// Windows relay 路径会在进入 wslpath 前统一为正斜杠。
    fn normalizes_windows_executable_for_wslpath() {
        assert_eq!(
            wslpath_input(r"C:\Program Files\LokiMetis\loki_metis_gui.exe"),
            "C:/Program Files/LokiMetis/loki_metis_gui.exe"
        );
        assert!(wsl_test_reports_missing(Some(1)));
        assert!(!wsl_test_reports_missing(Some(2)));
    }

    #[test]
    /// WSL 原子写入的临时名包含 PID 且每次不同，两个进程或线程不会共用旧固定文件。
    fn atomic_write_temporary_paths_are_process_scoped_and_unique() {
        let first = unique_temporary_path("/home/user/.grok/hooks/lokimetis.json");
        let second = unique_temporary_path("/home/user/.grok/hooks/lokimetis.json");
        let prefix = format!(
            "/home/user/.grok/hooks/lokimetis.json.lokimetis.{}.",
            std::process::id()
        );
        assert!(first.starts_with(&prefix));
        assert!(first.ends_with(".tmp"));
        assert_ne!(first, second);
    }
}
