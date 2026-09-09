/// 执行换皮宿主内部的 `fetch_targets` 步骤。
async fn fetch_targets(browser: &mut Browser) -> Result<(), AppError> {
    tokio::time::timeout(CDP_REQUEST_TIMEOUT, browser.fetch_targets())
        .await
        .map_err(|_| cdp_timeout("skin.cdp_request_timeout", "Codex 调试目标刷新超时。"))?
        .map_err(cdp_error)?;
    tokio::time::sleep(CDP_TARGET_SETTLE_DELAY).await;
    Ok(())
}

/// 激活与已验证调试端点对应的宿主窗口。
async fn try_activate_host_window(host: SkinHostKind, endpoint: CdpEndpoint) {
    let processes = match platform_host_processes(host).await {
        Ok(processes) => processes
            .into_iter()
            .map(resolved_instance)
            .collect::<Vec<_>>(),
        Err(_) => {
            tracing::warn!(?host, "code=skin.host_window_activation_target_failed");
            return;
        }
    };
    let Some(pid) = unique_codex_pid_for_endpoint(&processes, endpoint).or_else(|| {
        (host == SkinHostKind::WorkBuddy && processes.len() == 1).then(|| processes[0].process.pid)
    }) else {
        tracing::warn!(?host, "code=skin.host_window_activation_target_missing");
        return;
    };
    if !activate_platform_codex_window(pid).await {
        tracing::warn!(?host, "code=skin.host_window_activation_rejected");
    }
}

/// 执行换皮宿主内部的 `exactly_one` 步骤：迭代器恰好只有一个匹配项时返回该项。
fn exactly_one<T>(mut iter: impl Iterator<Item = T>) -> Option<T> {
    let first = iter.next()?;
    iter.next().is_none().then_some(first)
}

/// 执行换皮宿主内部的 `unique_codex_pid_for_endpoint` 步骤。
fn unique_codex_pid_for_endpoint(
    instances: &[ResolvedCodexInstance],
    endpoint: CdpEndpoint,
) -> Option<u32> {
    exactly_one(
        instances
            .iter()
            .filter(|instance| instance.debug_port == Some(endpoint.port)),
    )
    .map(|instance| instance.process.pid)
}

/// 执行换皮宿主内部的 `restarted_instance_for_endpoint` 步骤。
fn restarted_instance_for_endpoint(
    instances: Vec<CodexInstance>,
    endpoint: CdpEndpoint,
) -> Option<CodexInstance> {
    exactly_one(
        instances
            .into_iter()
            .filter(|instance| instance.debug_port == Some(endpoint.port)),
    )
}

#[cfg(target_os = "windows")]
/// 执行换皮宿主内部的 `activate_platform_codex_window` 步骤。
async fn activate_platform_codex_window(pid: u32) -> bool {
    tokio::task::spawn_blocking(move || windows_codex::activate_gui_process(pid))
        .await
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
/// 执行换皮宿主内部的 `activate_platform_codex_window` 步骤。
async fn activate_platform_codex_window(pid: u32) -> bool {
    tokio::task::spawn_blocking(move || macos_codex::activate_gui_process(pid))
        .await
        .unwrap_or(false)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
/// 执行换皮宿主内部的 `activate_platform_codex_window` 步骤。
async fn activate_platform_codex_window(_pid: u32) -> bool {
    false
}

/// 执行换皮宿主内部的 `browser_pages` 步骤。
async fn browser_pages(browser: &Browser) -> Result<Vec<Page>, AppError> {
    tokio::time::timeout(CDP_REQUEST_TIMEOUT, browser.pages())
        .await
        .map_err(|_| cdp_timeout("skin.cdp_request_timeout", "Codex 调试页面列表响应超时。"))?
        .map_err(cdp_error)
}

#[cfg(target_os = "macos")]
/// 执行换皮宿主内部的 `launch_platform_codex` 步骤。
async fn launch_platform_codex() -> Result<(), AppError> {
    macos_process::launch_host(SkinHostKind::Codex, 9341).await
}

#[cfg(any(target_os = "macos", test))]
/// 执行换皮宿主内部的 `matching_process_command_lines` 步骤。
fn matching_process_command_lines(output: &str, executable: &Path) -> Vec<(u32, String)> {
    let executable = executable.to_string_lossy();
    let quoted_executable = format!("\"{executable}\"");
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let split = line.find(char::is_whitespace)?;
            let (pid, command) = line.split_at(split);
            let command = command.trim_start();
            let matches_executable = [executable.as_ref(), quoted_executable.as_str()]
                .iter()
                .any(|prefix| {
                    command
                        .strip_prefix(prefix)
                        .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with(" --"))
                });
            if !matches_executable {
                return None;
            }
            Some((pid.parse().ok()?, command.to_owned()))
        })
        .collect()
}

#[cfg(target_os = "macos")]
/// 执行换皮宿主内部的 `platform_codex_command_lines` 步骤。
async fn platform_codex_command_lines() -> Result<Vec<(u32, String)>, AppError> {
    macos_process::host_command_lines(SkinHostKind::Codex).await
}

#[cfg(target_os = "macos")]
/// 执行换皮宿主内部的 `platform_codex_processes` 步骤。
async fn platform_codex_processes() -> Result<Vec<PlatformCodexProcess>, AppError> {
    Ok(macos_process::host_processes(SkinHostKind::Codex)
        .await?
        .into_iter()
        .filter(|process| is_primary_codex_command_line(&process.command_line))
        .collect())
}

#[cfg(target_os = "macos")]
/// 列出经过官方可执行路径验证的 WorkBuddy GUI 主进程。
async fn platform_workbuddy_processes() -> Result<Vec<PlatformCodexProcess>, AppError> {
    Ok(macos_process::host_processes(SkinHostKind::WorkBuddy)
        .await?
        .into_iter()
        .filter(|process| is_primary_codex_command_line(&process.command_line))
        .collect())
}

#[cfg(target_os = "macos")]
/// 读取 macOS 官方 WorkBuddy 主程序对应的进程命令行。
async fn platform_workbuddy_command_lines() -> Result<Vec<(u32, String)>, AppError> {
    macos_process::host_command_lines(SkinHostKind::WorkBuddy).await
}

#[cfg(target_os = "macos")]
/// 判断 macOS 官方 WorkBuddy 是否运行，未安装按未运行处理。
async fn platform_workbuddy_is_running() -> Result<bool, AppError> {
    match macos_process::host_is_running(SkinHostKind::WorkBuddy).await {
        Ok(running) => Ok(running),
        Err(error) if error.code == "skin.workbuddy_not_found" => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "macos")]
/// 在 macOS 上启动官方 WorkBuddy，并通过环境变量绑定本机调试端口。
async fn launch_platform_workbuddy(port: u16) -> Result<(), AppError> {
    macos_process::launch_host(SkinHostKind::WorkBuddy, port).await
}

#[cfg(target_os = "macos")]
/// 仅向路径验证通过的 macOS WorkBuddy 进程发送终止信号。
async fn force_close_platform_workbuddy() -> Result<(), AppError> {
    macos_process::force_close_host(SkinHostKind::WorkBuddy).await
}

#[cfg(target_os = "macos")]
/// 复核 macOS WorkBuddy 实例身份后按原参数和新端口重启。
async fn restart_platform_workbuddy_instance(
    selected: &ResolvedCodexInstance,
    port: u16,
) -> Result<(), AppError> {
    let current = resolve_host_instance(SkinHostKind::WorkBuddy, &selected.id).await?;
    if current.process.executable != selected.process.executable {
        return Err(AppError::new(
            "skin.workbuddy_instance_changed",
            "所选 WorkBuddy 实例身份已变化，请重新选择。",
        ));
    }
    macos_process::restart_host_instance(SkinHostKind::WorkBuddy, selected, port).await
}

#[cfg(target_os = "macos")]
/// 执行换皮宿主内部的 `restart_platform_codex_instance` 步骤。
async fn restart_platform_codex_instance(
    selected: &ResolvedCodexInstance,
    port: u16,
) -> Result<(), AppError> {
    let current = resolve_codex_instance(&selected.id).await?;
    if current.process.executable != selected.process.executable {
        return Err(AppError::new(
            "skin.codex_instance_changed",
            "所选 Codex 实例身份已变化，请重新选择。",
        ));
    }
    let mut restarted = selected.clone();
    restarted
        .arguments
        .push("--remote-debugging-address=127.0.0.1".to_owned());
    restarted
        .arguments
        .push(format!("--remote-debugging-port={port}"));
    macos_process::restart_host_instance(SkinHostKind::Codex, &restarted, port).await
}

#[cfg(target_os = "macos")]
/// 执行换皮宿主内部的 `force_close_platform_codex` 步骤。
async fn force_close_platform_codex() -> Result<(), AppError> {
    macos_process::force_close_host(SkinHostKind::Codex).await
}

#[cfg(target_os = "macos")]
/// 执行换皮宿主内部的 `platform_codex_is_running` 步骤。
async fn platform_codex_is_running() -> Result<bool, AppError> {
    match macos_process::host_is_running(SkinHostKind::Codex).await {
        Ok(running) => Ok(running),
        Err(error) if error.code == "skin.codex_not_found" => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "windows")]
/// 执行换皮宿主内部的 `platform_codex_is_running` 步骤。
async fn platform_codex_is_running() -> Result<bool, AppError> {
    windows_codex::is_gui_running().await
}

#[cfg(target_os = "windows")]
/// 执行换皮宿主内部的 `platform_codex_command_lines` 步骤。
async fn platform_codex_command_lines() -> Result<Vec<(u32, String)>, AppError> {
    windows_codex::gui_process_command_lines().await
}

#[cfg(target_os = "windows")]
/// 执行换皮宿主内部的 `platform_codex_processes` 步骤。
async fn platform_codex_processes() -> Result<Vec<PlatformCodexProcess>, AppError> {
    Ok(windows_codex::gui_processes()
        .await?
        .into_iter()
        .filter(|(_, command_line)| is_primary_codex_command_line(command_line))
        .map(|(process, command_line)| PlatformCodexProcess {
            pid: process.pid(),
            executable: process.path().to_owned(),
            command_line,
        })
        .collect())
}

#[cfg(target_os = "windows")]
/// 执行换皮宿主内部的 `restart_platform_codex_instance` 步骤。
async fn restart_platform_codex_instance(
    selected: &ResolvedCodexInstance,
    port: u16,
) -> Result<(), AppError> {
    windows_codex::restart_gui_process(
        selected.process.pid,
        &selected.process.executable,
        &selected.arguments,
        port,
    )
    .await
}

#[cfg(target_os = "windows")]
/// 执行换皮宿主内部的 `launch_platform_codex` 步骤。
async fn launch_platform_codex() -> Result<(), AppError> {
    windows_codex::launch().await
}

#[cfg(target_os = "windows")]
/// 执行换皮宿主内部的 `force_close_platform_codex` 步骤。
async fn force_close_platform_codex() -> Result<(), AppError> {
    windows_codex::force_close_gui().await.map(|_| ())
}

#[cfg(target_os = "windows")]
/// 委托 Windows 适配器判断官方 WorkBuddy GUI 是否运行。
async fn platform_workbuddy_is_running() -> Result<bool, AppError> {
    windows_codex::workbuddy_is_gui_running().await
}

#[cfg(target_os = "windows")]
/// 委托 Windows 适配器读取可信 WorkBuddy GUI 命令行。
async fn platform_workbuddy_command_lines() -> Result<Vec<(u32, String)>, AppError> {
    windows_codex::workbuddy_gui_process_command_lines().await
}

#[cfg(target_os = "windows")]
/// 将 Windows 可信 WorkBuddy 进程转换为统一的平台进程结构。
async fn platform_workbuddy_processes() -> Result<Vec<PlatformCodexProcess>, AppError> {
    Ok(windows_codex::workbuddy_gui_processes()
        .await?
        .into_iter()
        .map(|(process, command_line)| PlatformCodexProcess {
            pid: process.pid(),
            executable: process.path().to_owned(),
            command_line,
        })
        .collect())
}

#[cfg(target_os = "windows")]
/// 委托 Windows 适配器复核并重启指定 WorkBuddy 实例。
async fn restart_platform_workbuddy_instance(
    selected: &ResolvedCodexInstance,
    port: u16,
) -> Result<(), AppError> {
    windows_codex::restart_workbuddy_gui_process(
        selected.process.pid,
        &selected.process.executable,
        &selected.arguments,
        port,
    )
    .await
}

#[cfg(target_os = "windows")]
/// 委托 Windows 适配器以指定调试端口启动 WorkBuddy。
async fn launch_platform_workbuddy(port: u16) -> Result<(), AppError> {
    windows_codex::launch_workbuddy(port).await
}

#[cfg(target_os = "windows")]
/// 委托 Windows 适配器关闭全部可信 WorkBuddy GUI 进程。
async fn force_close_platform_workbuddy() -> Result<(), AppError> {
    windows_codex::force_close_workbuddy_gui().await.map(|_| ())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
/// 执行换皮宿主内部的 `platform_codex_is_running` 步骤。
async fn platform_codex_is_running() -> Result<bool, AppError> {
    Ok(false)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
/// 执行换皮宿主内部的 `platform_codex_command_lines` 步骤。
async fn platform_codex_command_lines() -> Result<Vec<(u32, String)>, AppError> {
    Ok(Vec::new())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
/// 执行换皮宿主内部的 `platform_codex_processes` 步骤。
async fn platform_codex_processes() -> Result<Vec<PlatformCodexProcess>, AppError> {
    Ok(Vec::new())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
/// 执行换皮宿主内部的 `restart_platform_codex_instance` 步骤。
async fn restart_platform_codex_instance(
    _selected: &ResolvedCodexInstance,
    _port: u16,
) -> Result<(), AppError> {
    Err(AppError::new(
        "skin.platform_unsupported",
        "当前平台不支持重启 Codex 实例。",
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
/// 执行换皮宿主内部的 `launch_platform_codex` 步骤。
async fn launch_platform_codex() -> Result<(), AppError> {
    Err(AppError::new(
        "skin.platform_unsupported",
        "当前版本仅支持在 macOS 或 Windows 上启动 Codex 皮肤。",
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
/// 执行换皮宿主内部的 `force_close_platform_codex` 步骤。
async fn force_close_platform_codex() -> Result<(), AppError> {
    Err(AppError::new(
        "skin.platform_unsupported",
        "当前版本仅支持在 macOS 或 Windows 上强制启动 Codex 皮肤。",
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
/// 未支持平台始终报告 WorkBuddy 未运行。
async fn platform_workbuddy_is_running() -> Result<bool, AppError> {
    Ok(false)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
/// 未支持平台不提供 WorkBuddy 进程命令行。
async fn platform_workbuddy_command_lines() -> Result<Vec<(u32, String)>, AppError> {
    Ok(Vec::new())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
/// 未支持平台不提供 WorkBuddy 进程实例。
async fn platform_workbuddy_processes() -> Result<Vec<PlatformCodexProcess>, AppError> {
    Ok(Vec::new())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
/// 未支持平台拒绝重启 WorkBuddy 实例。
async fn restart_platform_workbuddy_instance(
    _selected: &ResolvedCodexInstance,
    _port: u16,
) -> Result<(), AppError> {
    Err(AppError::new(
        "skin.platform_unsupported",
        "当前平台不支持重启 WorkBuddy 实例。",
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
/// 未支持平台拒绝启动 WorkBuddy。
async fn launch_platform_workbuddy(_port: u16) -> Result<(), AppError> {
    Err(AppError::new(
        "skin.platform_unsupported",
        "当前版本仅支持在 macOS 或 Windows 上启动 WorkBuddy 皮肤。",
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
/// 未支持平台拒绝强制关闭 WorkBuddy。
async fn force_close_platform_workbuddy() -> Result<(), AppError> {
    Err(AppError::new(
        "skin.platform_unsupported",
        "当前版本仅支持在 macOS 或 Windows 上强制启动 WorkBuddy 皮肤。",
    ))
}

include!("host.rs");
