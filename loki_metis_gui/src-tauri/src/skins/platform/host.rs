/// 按宿主类型路由到经过应用身份验证的平台进程适配器。
async fn platform_host_is_running(host: SkinHostKind) -> Result<bool, AppError> {
    match host {
        SkinHostKind::Codex => platform_codex_is_running().await,
        SkinHostKind::WorkBuddy => platform_workbuddy_is_running().await,
    }
}

/// 读取指定宿主已验证进程的命令行，用于发现其显式 CDP 端口。
async fn platform_host_command_lines(
    host: SkinHostKind,
) -> Result<Vec<(u32, String)>, AppError> {
    match host {
        SkinHostKind::Codex => platform_codex_command_lines().await,
        SkinHostKind::WorkBuddy => platform_workbuddy_command_lines().await,
    }
}

/// 列出指定宿主的可信主进程及其启动参数。
async fn platform_host_processes(
    host: SkinHostKind,
) -> Result<Vec<PlatformCodexProcess>, AppError> {
    match host {
        SkinHostKind::Codex => platform_codex_processes().await,
        SkinHostKind::WorkBuddy => platform_workbuddy_processes().await,
    }
}

/// 以原实例身份和参数重启指定宿主，并绑定新的本机调试端口。
async fn restart_platform_host_instance(
    host: SkinHostKind,
    selected: &ResolvedCodexInstance,
    port: u16,
) -> Result<(), AppError> {
    match host {
        SkinHostKind::Codex => restart_platform_codex_instance(selected, port).await,
        SkinHostKind::WorkBuddy => restart_platform_workbuddy_instance(selected, port).await,
    }
}

/// 按宿主类型启动官方桌面应用，并仅开放指定的本机调试端口。
async fn launch_platform_host(host: SkinHostKind, port: u16) -> Result<(), AppError> {
    match host {
        SkinHostKind::Codex => launch_platform_codex().await,
        SkinHostKind::WorkBuddy => launch_platform_workbuddy(port).await,
    }
}

/// 把 CDP listener 的唯一 owner 绑定到 Windows 或 macOS 的官方 WorkBuddy 进程树。
async fn platform_workbuddy_endpoint_owned_by_root(
    port: u16,
    root_pid: u32,
) -> Result<bool, AppError> {
    #[cfg(target_os = "windows")]
    {
        return tokio::task::spawn_blocking(move || {
            windows_codex::workbuddy_endpoint_owned_by_root(port, root_pid)
        })
        .await
        .map_err(|_| workbuddy_owner_inspection_failed())?;
    }
    #[cfg(target_os = "macos")]
    {
        return macos_process::workbuddy_endpoint_owned_by_root(port, root_pid).await;
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = (port, root_pid);
        Ok(false)
    }
}

/// 关闭指定宿主的全部已验证 GUI 进程，并传播不完整关闭错误。
async fn force_close_platform_host(host: SkinHostKind) -> Result<(), AppError> {
    match host {
        SkinHostKind::Codex => force_close_platform_codex().await,
        SkinHostKind::WorkBuddy => force_close_platform_workbuddy().await,
    }
}
