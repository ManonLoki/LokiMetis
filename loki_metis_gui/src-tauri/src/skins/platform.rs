/// 执行换皮宿主内部的 `fetch_targets` 步骤。
async fn fetch_targets(browser: &mut Browser) -> Result<(), AppError> {
    tokio::time::timeout(CDP_REQUEST_TIMEOUT, browser.fetch_targets())
        .await
        .map_err(|_| cdp_timeout("skin.cdp_request_timeout", "Codex 调试目标刷新超时。"))?
        .map_err(cdp_error)?;
    tokio::time::sleep(CDP_TARGET_SETTLE_DELAY).await;
    Ok(())
}

/// 执行换皮宿主内部的 `try_activate_codex_window` 步骤。
async fn try_activate_codex_window(endpoint: CdpEndpoint) {
    let processes = match platform_codex_processes().await {
        Ok(processes) => processes
            .into_iter()
            .map(resolved_instance)
            .collect::<Vec<_>>(),
        Err(_) => {
            tracing::warn!("code=skin.codex_window_activation_target_failed");
            return;
        }
    };
    let Some(pid) = unique_codex_pid_for_endpoint(&processes, endpoint) else {
        tracing::warn!("code=skin.codex_window_activation_target_missing");
        return;
    };
    if !activate_platform_codex_window(pid).await {
        tracing::warn!("code=skin.codex_window_activation_rejected");
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
    let executable = discover_codex_executable().await?;
    if codex_is_running(&executable).await {
        return Err(manual_close_required());
    }
    let bundle = executable
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or_else(|| AppError::new("skin.codex_not_found", "Codex 应用路径无效。"))?;
    let status = tokio::process::Command::new("/usr/bin/open")
        .args(["-na"])
        .arg(bundle)
        .args([
            "--args",
            "--remote-debugging-address=127.0.0.1",
            "--remote-debugging-port=9341",
        ])
        .status()
        .await
        .map_err(|_| AppError::new("skin.codex_launch_failed", "无法启动 Codex。"))?;
    if !status.success() {
        return Err(AppError::new(
            "skin.codex_launch_failed",
            "系统未能以调试参数启动 Codex。",
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
/// 执行换皮宿主内部的 `discover_codex_executable` 步骤。
async fn discover_codex_executable() -> Result<PathBuf, AppError> {
    let output = tokio::process::Command::new("/usr/bin/mdfind")
        .arg("kMDItemCFBundleIdentifier == \"com.openai.codex\"")
        .output()
        .await
        .map_err(|_| AppError::new("skin.codex_not_found", "无法定位 Codex 应用。"))?;
    let mut bundles = vec![PathBuf::from("/Applications/ChatGPT.app")];
    if let Some(home) = std::env::var_os("HOME") {
        bundles.push(PathBuf::from(home).join("Applications/ChatGPT.app"));
    }
    bundles.extend(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| !line.is_empty())
            .map(PathBuf::from),
    );
    for bundle in bundles {
        let info = bundle.join("Contents/Info.plist");
        if !info.is_file() {
            continue;
        }
        let identifier = plist_value(&info, "CFBundleIdentifier").await;
        let executable_name = plist_value(&info, "CFBundleExecutable").await;
        if identifier.as_deref() == Some("com.openai.codex") {
            if let Some(name) = executable_name {
                let executable = bundle.join("Contents/MacOS").join(name);
                if executable.is_file() {
                    return Ok(executable);
                }
            }
        }
    }
    Err(AppError::new(
        "skin.codex_not_found",
        "未找到官方 Codex 应用（com.openai.codex）。",
    ))
}

#[cfg(target_os = "macos")]
/// 执行换皮宿主内部的 `plist_value` 步骤。
async fn plist_value(info: &Path, key: &str) -> Option<String> {
    let output = tokio::process::Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(info)
        .output()
        .await
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(target_os = "macos")]
/// 执行换皮宿主内部的 `codex_is_running` 步骤。
async fn codex_is_running(executable: &Path) -> bool {
    let Ok(output) = tokio::process::Command::new("/bin/ps")
        .args(["-axo", "command="])
        .output()
        .await
    else {
        return false;
    };
    let executable = executable.to_string_lossy();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line == executable || line.starts_with(&format!("{executable} ")))
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
    let executable = discover_codex_executable().await?;
    codex_command_lines_for(&executable).await
}

#[cfg(target_os = "macos")]
/// 执行换皮宿主内部的 `codex_command_lines_for` 步骤。
async fn codex_command_lines_for(executable: &Path) -> Result<Vec<(u32, String)>, AppError> {
    let output = tokio::process::Command::new("/bin/ps")
        .args(["-axo", "pid=,command="])
        .output()
        .await
        .map_err(|_| {
            AppError::new(
                "skin.codex_process_inspection_failed",
                "无法读取 Codex 调试启动参数。",
            )
        })?;
    if !output.status.success() {
        return Err(AppError::new(
            "skin.codex_process_inspection_failed",
            "无法读取 Codex 调试启动参数。",
        ));
    }
    Ok(matching_process_command_lines(
        &String::from_utf8_lossy(&output.stdout),
        executable,
    ))
}

#[cfg(target_os = "macos")]
/// 执行换皮宿主内部的 `platform_codex_processes` 步骤。
async fn platform_codex_processes() -> Result<Vec<PlatformCodexProcess>, AppError> {
    let executable = discover_codex_executable().await?;
    Ok(codex_command_lines_for(&executable)
        .await?
        .into_iter()
        .filter(|(_, command_line)| is_primary_codex_command_line(command_line))
        .map(|(pid, command_line)| PlatformCodexProcess {
            pid,
            executable: executable.clone(),
            command_line,
        })
        .collect())
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
    let pid = selected.process.pid.to_string();
    let status = tokio::process::Command::new("/bin/kill")
        .args(["-TERM", pid.as_str()])
        .status()
        .await
        .map_err(|_| AppError::new("skin.codex_force_close_failed", "无法关闭所选 Codex 实例。"))?;
    if !status.success() {
        return Err(AppError::new(
            "skin.codex_force_close_failed",
            "无法关闭所选 Codex 实例。",
        ));
    }
    tokio::process::Command::new(&selected.process.executable)
        .args(&selected.arguments)
        .args([
            "--remote-debugging-address=127.0.0.1".to_owned(),
            format!("--remote-debugging-port={port}"),
        ])
        .spawn()
        .map_err(|_| {
            AppError::new(
                "skin.codex_launch_failed",
                "无法使用原启动参数重新打开所选 Codex 实例。",
            )
        })?;
    Ok(())
}

#[cfg(target_os = "macos")]
/// 执行换皮宿主内部的 `force_close_platform_codex` 步骤。
async fn force_close_platform_codex() -> Result<(), AppError> {
    let executable = discover_codex_executable().await?;
    let pids = codex_command_lines_for(&executable)
        .await
        .map_err(|_| {
            AppError::new(
                "skin.codex_force_close_failed",
                "无法检查当前 Codex/GPT 桌面应用进程。",
            )
        })?
        .into_iter()
        .map(|(pid, _)| pid.to_string())
        .collect::<Vec<_>>();
    for pid in &pids {
        let status = tokio::process::Command::new("/bin/kill")
            .args(["-TERM", pid])
            .status()
            .await
            .map_err(|_| {
                AppError::new(
                    "skin.codex_force_close_failed",
                    "无法关闭当前 Codex/GPT 桌面应用，请保存工作后手动退出。",
                )
            })?;
        if !status.success() {
            return Err(AppError::new(
                "skin.codex_force_close_failed",
                "无法关闭当前 Codex/GPT 桌面应用，请保存工作后手动退出。",
            ));
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
/// 执行换皮宿主内部的 `platform_codex_is_running` 步骤。
async fn platform_codex_is_running() -> Result<bool, AppError> {
    match discover_codex_executable().await {
        Ok(executable) => Ok(codex_is_running(&executable).await),
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
