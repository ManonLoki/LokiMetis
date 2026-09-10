// 以可信系统入口异步打开皮肤目录，并完整拥有 launcher 生命周期。

/// 文件管理器 launcher 的单次运行时限。
#[cfg(unix)]
const FILE_MANAGER_TIMEOUT: Duration = Duration::from_secs(5);
/// launcher 超时后用于终止和回收子进程的尾部预算。
#[cfg(unix)]
const FILE_MANAGER_REAP_TIMEOUT: Duration = Duration::from_secs(1);

/// 执行换皮宿主内部的 `open_in_file_manager` 步骤。
async fn open_in_file_manager(directory: &Path) -> Result<(), AppError> {
    let directory = tokio::fs::canonicalize(directory)
        .await
        .map_err(|_| AppError::new("skin.open_failed", "无法读取皮肤目录。"))?;
    let metadata = tokio::fs::metadata(&directory)
        .await
        .map_err(|_| AppError::new("skin.open_failed", "无法读取皮肤目录。"))?;
    if !metadata.is_dir() {
        return Err(AppError::new(
            "skin.open_failed",
            "皮肤目录不存在或不可访问。",
        ));
    }

    #[cfg(target_os = "macos")]
    {
        let mut command = tokio::process::Command::new("/usr/bin/open");
        command.arg(&directory);
        return run_file_manager_command(command).await;
    }
    #[cfg(target_os = "windows")]
    {
        return open_with_windows_shell(&directory);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let (program, prefix) = trusted_linux_file_manager()
            .ok_or_else(|| AppError::new("skin.open_failed", "未找到可信的系统文件管理器入口。"))?;
        let mut command = tokio::process::Command::new(program);
        command.args(prefix).arg(&directory);
        return run_file_manager_command(command).await;
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        let _ = directory;
        Err(AppError::new(
            "skin.open_failed",
            "当前平台不支持打开皮肤目录。",
        ))
    }
}

#[cfg(unix)]
/// 描述文件管理器 launcher 的启动、超时、回收或非零退出。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileManagerCommandError {
    Spawn,
    TimedOut,
    ReapTimedOut,
    Failed,
}

#[cfg(unix)]
/// 将共享进程 owner 错误映射到文件管理器的稳定错误合同。
fn file_manager_process_error(error: process_reaper::OwnedProcessError) -> FileManagerCommandError {
    match error {
        process_reaper::OwnedProcessError::Spawn => FileManagerCommandError::Spawn,
        process_reaper::OwnedProcessError::ReapTimedOut => FileManagerCommandError::ReapTimedOut,
        process_reaper::OwnedProcessError::Io
        | process_reaper::OwnedProcessError::ShuttingDown
        | process_reaper::OwnedProcessError::CapacityExceeded => FileManagerCommandError::Failed,
    }
}

#[cfg(unix)]
/// 在共享进程 owner 中运行 launcher；超时、取消与回收共用一次确定的总截止。
async fn run_owned_file_manager_command(
    mut command: tokio::process::Command,
    timeout: Duration,
) -> Result<(), FileManagerCommandError> {
    let started_at = tokio::time::Instant::now();
    let execution_deadline = started_at + timeout;
    let final_deadline = execution_deadline + FILE_MANAGER_REAP_TIMEOUT;
    process_reaper::prepare_process_spawn(execution_deadline)
        .await
        .map_err(file_manager_process_error)?;
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut owner = process_reaper::OwnedProcessChild::spawn(&mut command)
        .map_err(file_manager_process_error)?;
    let mut shutdown = owner.shutdown_receiver();
    let waited = tokio::select! {
        biased;
        _ = process_reaper::wait_for_process_shutdown(&mut shutdown) => None,
        waited = tokio::time::timeout_at(execution_deadline, owner.child_mut().wait()) => Some(waited),
    };
    match waited {
        Some(Ok(Ok(status))) if status.success() => Ok(()),
        Some(Ok(Ok(_))) | Some(Ok(Err(_))) => Err(FileManagerCommandError::Failed),
        Some(Err(_)) => {
            owner
                .terminate_and_reap_until(final_deadline)
                .await
                .map_err(file_manager_process_error)?;
            Err(FileManagerCommandError::TimedOut)
        }
        None => {
            let shutdown_deadline = process_reaper::process_shutdown_deadline()
                .unwrap_or(final_deadline)
                .min(final_deadline);
            owner
                .terminate_and_reap_until(shutdown_deadline)
                .await
                .map_err(file_manager_process_error)?;
            Err(FileManagerCommandError::Failed)
        }
    }
}

#[cfg(unix)]
/// 把 launcher 失败映射为可区分超时与普通故障的稳定错误。
async fn run_file_manager_command(command: tokio::process::Command) -> Result<(), AppError> {
    run_owned_file_manager_command(command, FILE_MANAGER_TIMEOUT)
        .await
        .map_err(|error| {
            if matches!(
                error,
                FileManagerCommandError::TimedOut | FileManagerCommandError::ReapTimedOut
            ) {
                AppError::new("skin.open_timeout", "系统文件管理器响应超时。")
            } else {
                AppError::new("skin.open_failed", "无法在文件管理器中打开皮肤目录。")
            }
        })
}

#[cfg(all(unix, not(target_os = "macos")))]
/// 从固定绝对路径中选择已安装且非符号链接的 Linux 文件管理器入口。
fn trusted_linux_file_manager() -> Option<(&'static str, &'static [&'static str])> {
    const CANDIDATES: [(&str, &[&str]); 4] = [
        ("/usr/bin/xdg-open", &[]),
        ("/usr/bin/gio", &["open"]),
        ("/bin/xdg-open", &[]),
        ("/bin/gio", &["open"]),
    ];
    CANDIDATES.into_iter().find(|(path, _)| {
        std::fs::symlink_metadata(path)
            .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
    })
}

#[cfg(target_os = "windows")]
/// 通过 Windows Shell API 打开目录，不解析 PATH，也不创建需由应用回收的子进程。
fn open_with_windows_shell(directory: &Path) -> Result<(), AppError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::PCWSTR;

    let operation = "open".encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let directory = directory
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: UTF-16 缓冲区均以 NUL 结尾且在 ShellExecuteW 调用期间保持有效；其余参数为空。
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(operation.as_ptr()),
            PCWSTR(directory.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        return Err(AppError::new(
            "skin.open_failed",
            "无法在文件管理器中打开皮肤目录。",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod file_manager_tests {
    #[cfg(unix)]
    use super::*;

    /// 永不退出的 launcher 必须被终止回收，并返回独立超时状态。
    #[cfg(unix)]
    #[tokio::test]
    async fn file_manager_launcher_timeout_is_observable() {
        let command = tokio::process::Command::new("/usr/bin/yes");
        assert_eq!(
            run_owned_file_manager_command(command, Duration::from_millis(30)).await,
            Err(FileManagerCommandError::TimedOut)
        );
    }

    /// 非零 launcher 退出不能被误报为目录已经打开。
    #[cfg(unix)]
    #[tokio::test]
    async fn file_manager_launcher_failure_is_observable() {
        let command = tokio::process::Command::new("/usr/bin/false");
        assert_eq!(
            run_owned_file_manager_command(command, FILE_MANAGER_TIMEOUT).await,
            Err(FileManagerCommandError::Failed)
        );
    }

    /// 进程 owner 容量耗尽必须稳定映射为 launcher 失败。
    #[cfg(unix)]
    #[test]
    fn file_manager_maps_process_capacity_to_failure() {
        assert_eq!(
            file_manager_process_error(process_reaper::OwnedProcessError::CapacityExceeded),
            FileManagerCommandError::Failed
        );
    }

    /// 文件管理器实现不得重新引入依赖 PATH 的 explorer 或 xdg-open 启动方式。
    #[test]
    fn file_manager_source_rejects_path_resolved_launchers() {
        let source = include_str!("file_manager.rs");
        assert!(!source.contains("Command::new(\"explorer\")"));
        assert!(!source.contains("Command::new(\"xdg-open\")"));
        assert!(source.contains("ShellExecuteW"));
        assert!(source.contains("/usr/bin/xdg-open"));
    }

    /// launcher 不得依赖 drop JoinHandle 的 detached task，必须交给共享子进程 owner。
    #[test]
    fn file_manager_launcher_uses_retained_process_owner() {
        let source = include_str!("file_manager.rs");
        let runner = source
            .split_once("async fn run_owned_file_manager_command")
            .expect("launcher runner must exist")
            .1
            .split_once("/// 把 launcher 失败映射")
            .expect("launcher runner boundary must exist")
            .0;
        assert!(runner.contains("process_reaper::OwnedProcessChild::spawn"));
        assert!(runner.contains("terminate_and_reap_until(final_deadline)"));
        assert!(!runner.contains("tokio::spawn(async move"));
    }

    /// Tauri 命令必须保持异步，防止 launcher 等待占用 IPC/UI 执行线程。
    #[test]
    fn open_skin_directory_command_is_async() {
        let source = include_str!("commands.rs");
        assert!(source.contains("pub async fn open_skin_directory"));
    }
}
