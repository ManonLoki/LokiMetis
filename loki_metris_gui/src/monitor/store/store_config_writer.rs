// 本文件是 mod.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：消费自动补写请求，直到应用生命周期明确关闭。；在具名 worker 中读取设置并完成显式写入，不占用 GPUI 异步执行线程。；串行化一次显式或后台写入；后台路径额外携带应用退出取消令牌。

/// 消费自动补写请求，直到应用生命周期明确关闭。
fn hook_config_writer_loop(
    shared: &Arc<(Mutex<HookWriterState>, Condvar)>,
    relay_executable: &Path,
    home_directory: &Path,
    repair_interval: Duration,
    cancellation: &Arc<AtomicBool>,
) {
    loop {
        let work = {
            let (state, wake) = &**shared;
            let Ok(mut state) = state.lock() else {
                tracing::warn!("automatic hook repair state was poisoned");
                return;
            };
            loop {
                if state.shutting_down {
                    return;
                }
                if let Some(request) = state.explicit_requests.pop_front() {
                    state.active_explicit_cancellation = Some(Arc::clone(&request.cancellation));
                    break HookWriterWork::Explicit(request);
                }
                if state.repair_requested {
                    state.repair_requested = false;
                    if let Some(settings) = state.settings.clone() {
                        break HookWriterWork::AutomaticRepair(settings);
                    }
                }
                let Ok((next, wait)) = wake.wait_timeout(state, repair_interval) else {
                    tracing::warn!("automatic hook repair wait state was poisoned");
                    return;
                };
                state = next;
                if wait.timed_out() && state.settings.is_some() {
                    state.repair_requested = true;
                }
            }
        };
        match work {
            HookWriterWork::Explicit(request) => {
                let request_cancellation = Arc::clone(&request.cancellation);
                let result =
                    execute_explicit_hook_write(&request, relay_executable, home_directory);
                let _ = request.response.send(result);
                let (state, _) = &**shared;
                let Ok(mut state) = state.lock() else {
                    tracing::warn!("automatic hook repair state was poisoned");
                    return;
                };
                if state
                    .active_explicit_cancellation
                    .as_ref()
                    .is_some_and(|active| Arc::ptr_eq(active, &request_cancellation))
                {
                    state.active_explicit_cancellation = None;
                }
            }
            HookWriterWork::AutomaticRepair(settings) => {
                repair_enabled_hook_configs_with_cancellation(
                    &settings,
                    relay_executable,
                    home_directory,
                    Arc::clone(cancellation),
                )
            }
        }
    }
}

/// 在具名 worker 中读取设置并完成显式写入，不占用 GPUI 异步执行线程。
fn execute_explicit_hook_write(
    request: &ExplicitHookWriteRequest,
    relay_executable: &Path,
    home_directory: &Path,
) -> Result<HookConfigWriteResult, HookError> {
    ensure_hook_write_not_cancelled(Some(&request.cancellation))?;
    let settings = super::settings::load_monitor_settings(&request.config_directory)?;
    write_hook_config_with_cancellation(
        &settings,
        request.tool,
        relay_executable,
        home_directory,
        Some(Arc::clone(&request.cancellation)),
    )
}

/// 显式请求允许首次注册；后台维护使用独立的已拥有条目门禁。
fn write_hook_config_with_cancellation(
    settings: &MonitorSettings,
    tool: AiTool,
    relay_executable: &Path,
    home_directory: &Path,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<HookConfigWriteResult, HookError> {
    write_hook_config_in_mode(settings, tool, relay_executable, home_directory, cancellation, HookRegistrationMode::RegisterOrRepair)
}

/// 串行化一次写入并把授权模式传入每个配置根；取消令牌贯穿后台及显式路径。
fn write_hook_config_in_mode(
    settings: &MonitorSettings,
    tool: AiTool,
    relay_executable: &Path,
    home_directory: &Path,
    cancellation: Option<Arc<AtomicBool>>,
    mode: HookRegistrationMode,
) -> Result<HookConfigWriteResult, HookError> {
    ensure_hook_write_not_cancelled(cancellation.as_deref())?;
    let _guard = lock_hook_config_writes(cancellation.as_deref())?;
    ensure_hook_write_not_cancelled(cancellation.as_deref())?;
    write_hook_config_unlocked(
        settings,
        tool,
        relay_executable,
        home_directory,
        cancellation,
        mode,
    )
}

/// 以可取消短轮询取得进程内写锁，避免排队请求在 35 秒后仍无法确认取消。
fn lock_hook_config_writes(
    cancellation: Option<&AtomicBool>,
) -> Result<std::sync::MutexGuard<'static, ()>, HookError> {
    let lock = HOOK_CONFIG_WRITE_LOCK.get_or_init(|| Mutex::new(()));
    loop {
        ensure_hook_write_not_cancelled(cancellation)?;
        match lock.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::WouldBlock) => thread::sleep(HOOK_WRITER_SHUTDOWN_POLL),
            Err(TryLockError::Poisoned(_)) => {
                return Err(HookError::new("error.hooks.writeFailed")
                    .param("detail", "hook config write lock was poisoned"));
            }
        }
    }
}

/// 在阻塞阶段边界观察调用或应用退出取消，未开始的写入必须失败关闭。
fn ensure_hook_write_not_cancelled(cancellation: Option<&AtomicBool>) -> Result<(), HookError> {
    if cancellation.is_some_and(|cancellation| cancellation.load(Ordering::Acquire)) {
        return Err(hook_write_lifecycle_error(HOOK_WRITE_CANCELLED_DETAIL));
    }
    Ok(())
}

/// 已持有配置写锁时执行单个工具的完整多文件写入。
///
/// 同一工具可能有多个固定配置根（WorkBuddy 国内版与国际版），每个根都要独立完成
/// 主配置与辅助文件的合并写入。取消是硬边界，一旦观察到就立即停止；单个根失败
/// 不会阻止其余根继续写入，最终以第一个失败结束，交由后台自愈重试收敛。
fn write_hook_config_unlocked(
    settings: &MonitorSettings,
    tool: AiTool,
    relay_executable: &Path,
    home_directory: &Path,
    cancellation: Option<Arc<AtomicBool>>,
    mode: HookRegistrationMode,
) -> Result<HookConfigWriteResult, HookError> {
    let custom_directory = settings.hook_directories.get(tool);
    if !custom_directory.trim().is_empty() {
        validate_hook_config_directory(custom_directory)?;
    }
    let location = location_for(tool, &settings.hook_directories, home_directory);
    let config_path = PathBuf::from(&location.config_path);
    let wsl_directory = WslDirectory::parse(&location.directory)
        .map(|directory| directory.with_cancellation(cancellation.clone()));
    if wsl_directory.is_some() && !hook_supports_wsl(tool) {
        return Err(HookError::new("error.hooks.wslUnsupportedByWindowsHost")
            .param("tool", ai_tool_name(tool)));
    }
    if mode == HookRegistrationMode::RepairOwnedOnly {
        if let Some(directory) = &wsl_directory {
            let primary = directory.join(hook_config_filename(tool)).read_optional()?;
            if !mode.permits_primary(tool, primary.as_deref())? {
                return Ok(hook_config_write_result(tool, config_path.to_string_lossy().into_owned(), false));
            }
        }
    }
    let generated = if let Some(wsl_directory) = &wsl_directory {
        let wsl_executable = wsl_directory.translate_windows_executable(relay_executable)?;
        generate_wsl_hook_config(tool, relay_executable, &wsl_executable)?
    } else {
        generate_hook_config(tool, relay_executable)?
    };
    ensure_hook_write_not_cancelled(cancellation.as_deref())?;

    if let Some(wsl_directory) = wsl_directory {
        return write_wsl_configs(
            tool,
            &config_path,
            &wsl_directory,
            generated,
            cancellation.as_deref(),
            mode,
        );
    }

    let mut config_changed = false;
    let mut first_error = None;
    for directory in
        hook_config_target_directories(tool, &settings.hook_directories, home_directory)
    {
        ensure_hook_write_not_cancelled(cancellation.as_deref())?;
        let result = write_local_configs(
            tool,
            Path::new(&hook_config_path_in(tool, &directory)),
            &directory,
            generated.clone(),
            cancellation.as_deref(),
            mode,
        );
        match result {
            Ok(result) => config_changed |= result.config_changed,
            Err(error) => {
                tracing::warn!(
                    tool = ai_tool_name(tool),
                    code = error.code,
                    "failed to write one of the fixed hook config roots"
                );
                first_error = Some(error);
            }
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    Ok(hook_config_write_result(
        tool,
        config_path.to_string_lossy().into_owned(),
        config_changed,
    ))
}

/// 合并并写入普通本机配置及工具声明的全部辅助文件。
fn write_local_configs(
    tool: AiTool,
    config_path: &Path,
    directory: &Path,
    generated: HookConfigPreview,
    cancellation: Option<&AtomicBool>,
    mode: HookRegistrationMode,
) -> Result<HookConfigWriteResult, HookError> {
    ensure_hook_write_not_cancelled(cancellation)?;
    let mut generated_files = vec![(config_path.to_owned(), generated)];
    generated_files.extend(
        generate_hook_auxiliary_configs(tool)
            .into_iter()
            .map(|preview| (directory.join(&preview.filename), preview)),
    );
    let config_changed = super::hook_config_io::reconcile_local_config_set_with_cancellation(
        directory,
        tool,
        generated_files,
        cancellation,
        mode,
    )?;
    ensure_hook_write_not_cancelled(cancellation)?;
    Ok(hook_config_write_result(
        tool,
        config_path.to_string_lossy().into_owned(),
        config_changed,
    ))
}

/// 合并并写入 WSL 主配置及工具声明的全部辅助文件。
fn write_wsl_configs(
    tool: AiTool,
    config_path: &Path,
    directory: &WslDirectory,
    generated: HookConfigPreview,
    cancellation: Option<&AtomicBool>,
    mode: HookRegistrationMode,
) -> Result<HookConfigWriteResult, HookError> {
    ensure_hook_write_not_cancelled(cancellation)?;
    let mut generated_files = vec![(directory.join(hook_config_filename(tool)), generated)];
    generated_files.extend(
        generate_hook_auxiliary_configs(tool)
            .into_iter()
            .map(|preview| (directory.join(&preview.filename), preview)),
    );
    let config_changed = super::hook_config_io::reconcile_wsl_config_set(tool, generated_files, mode)?;
    ensure_hook_write_not_cancelled(cancellation)?;
    Ok(hook_config_write_result(
        tool,
        config_path.to_string_lossy().into_owned(),
        config_changed,
    ))
}

/// 对已启用且已注册的工具运行完整合并，修复旧路径和缺失事件但不首次注册。
#[cfg(test)]
fn repair_enabled_hook_configs(
    settings: &MonitorSettings,
    relay_executable: &Path,
    home_directory: &Path,
) {
    repair_enabled_hook_configs_with_cancellation(
        settings,
        relay_executable,
        home_directory,
        Arc::new(AtomicBool::new(false)),
    );
}

/// 后台自愈在每个工具边界观察应用退出，并把同一令牌传入 WSL 子进程。
fn repair_enabled_hook_configs_with_cancellation(
    settings: &MonitorSettings,
    relay_executable: &Path,
    home_directory: &Path,
    cancellation: Arc<AtomicBool>,
) {
    for tool in normalize_enabled_ai_tools(&settings.enabled_ai_tools) {
        if cancellation.load(Ordering::Acquire) {
            return;
        }
        if let Err(error) = write_hook_config_in_mode(
            settings,
            tool,
            relay_executable,
            home_directory,
            Some(Arc::clone(&cancellation)),
            HookRegistrationMode::RepairOwnedOnly,
        ) {
            tracing::warn!(
                tool = ai_tool_name(tool),
                code = error.code,
                "failed to repair enabled hook config"
            );
        }
    }
}

#[cfg(test)]
mod tests;
