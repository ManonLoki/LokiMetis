// 本文件收纳：连接 handler 的 owner 绑定，以及连接或启动宿主、等待 CDP 就绪、强制重启与运行状态归类。
// （本文件由主文件 include! 并入，与主文件共用同一个模块作用域。）

/// 把已由 guard 接管的连接绑定到后续操作；future 未 poll、取消或 panic 都不会丢失 owner。
async fn hold_connected_handler_during<T, Operation>(
    handler_task: HandlerTaskGuard,
    operation: Operation,
) -> (HandlerTaskGuard, T)
where
    Operation: Future<Output = T>,
{
    let result = operation.await;
    (handler_task, result)
}

/// 取得宿主的可信 CDP 连接：指定实例时只连接其调试端口并核对进程根；否则优先复用已运行宿主的端点，宿主运行却无调试端口时要求用户先退出，未运行时以空闲端口启动并等待就绪。
async fn connect_or_launch(
    host: SkinHostKind,
    cancel: &mut watch::Receiver<bool>,
    selected: Option<&ResolvedSkinHostInstance>,
    preferred_endpoint: Option<CdpEndpoint>,
) -> Result<
    (
        Browser,
        HandlerTaskGuard,
        ConnectionSource,
        CdpEndpoint,
        u32,
    ),
    AppError,
> {
    if let Some(selected) = selected {
        let port = selected
            .debug_port
            .ok_or_else(|| manual_close_required_for(host))?;
        let endpoint = CdpEndpoint::new(port);
        let connection = run_cancellable(cancel, connect_browser(endpoint)).await;
        let (mut browser, handler_task) = match connection {
            Ok(connection) => connection,
            Err(error) => return Err(remap_workbuddy_connection_error(host, error).await),
        };
        let (mut handler_task, verified_root_pid) = hold_connected_handler_during(
            handler_task,
            run_cancellable(
                cancel,
                browser_host_root_pid(host, &mut browser, endpoint, Some(selected.process.pid)),
            ),
        )
        .await;
        let verified_root_pid = match verified_root_pid {
            Ok(verified) => verified,
            Err(error) => {
                handler_task.abort();
                drop(browser);
                return Err(remap_workbuddy_connection_error(host, error).await);
            }
        };
        let Some(verified_root_pid) = verified_root_pid else {
            handler_task.abort();
            drop(browser);
            return Err(remap_workbuddy_connection_error(
                host,
                AppError::new(
                    "skin.cdp_rejected",
                    format!("调试端点不是 {} 的唯一可信主页面。", host.display_name()),
                ),
            )
            .await);
        };
        return Ok((
            browser,
            handler_task,
            ConnectionSource::Existing,
            endpoint,
            verified_root_pid,
        ));
    }
    match connect_existing_browser_with_preferred(host, cancel, preferred_endpoint).await {
        Ok((browser, handler_task, endpoint, root_pid)) => {
            return Ok((
                browser,
                handler_task,
                ConnectionSource::Existing,
                endpoint,
                root_pid,
            ));
        }
        Err(error) if error.code == "skin.operation_cancelled" => return Err(error),
        Err(error) if is_host_inspection_error(error.code) => return Err(error),
        Err(_) => {}
    }
    if run_cancellable(cancel, platform_host_is_running(host)).await? {
        return Err(manual_close_required_for(host));
    }
    let endpoint = available_launch_endpoint(host)?;
    run_cancellable(cancel, launch_platform_host(host, endpoint.port)).await?;
    let (browser, handler_task, endpoint, root_pid) =
        poll_until_cdp_ready(host, cancel, endpoint).await?;
    Ok((
        browser,
        handler_task,
        ConnectionSource::Launched,
        endpoint,
        root_pid,
    ))
}

/// 在启动期限内反复连接新启动宿主的调试端点，直到浏览器通过宿主身份核对；期间响应取消，Codex 还会监视进程退出。
async fn poll_until_cdp_ready(
    host: SkinHostKind,
    cancel: &mut watch::Receiver<bool>,
    endpoint: CdpEndpoint,
) -> Result<(Browser, HandlerTaskGuard, CdpEndpoint, u32), AppError> {
    let deadline = tokio::time::Instant::now() + CODEX_LAUNCH_TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let connection = async {
            tokio::time::timeout(remaining, connect_browser(endpoint))
                .await
                .map_err(|_| cdp_unavailable_for(host))?
        };
        let connection_result = if host == SkinHostKind::WorkBuddy {
            run_cancellable(cancel, connection).await
        } else {
            monitor_host_operation(host, cancel, connection).await
        };
        match connection_result {
            Ok((mut browser, handler_task)) => {
                let (mut handler_task, verified_root_pid) = hold_connected_handler_during(
                    handler_task,
                    run_cancellable(
                        cancel,
                        browser_host_root_pid(host, &mut browser, endpoint, None),
                    ),
                )
                .await;
                match verified_root_pid {
                    Ok(Some(root_pid)) => return Ok((browser, handler_task, endpoint, root_pid)),
                    Err(error) if error.code == "skin.operation_cancelled" => {
                        handler_task.abort();
                        drop(browser);
                        return Err(error);
                    }
                    Err(error) if is_host_inspection_error(error.code) => {
                        handler_task.abort();
                        drop(browser);
                        return Err(error);
                    }
                    Ok(None) => {
                        tracing::warn!("poll_until_cdp_ready: connected but no verified root pid");
                        handler_task.abort();
                        drop(browser);
                    }
                    Err(error) => {
                        tracing::warn!(
                            code = error.code,
                            message = error.message,
                            "poll_until_cdp_ready: root pid verification failed"
                        );
                        handler_task.abort();
                        drop(browser);
                    }
                }
            }
            Err(error)
                if matches!(error.code, "skin.operation_cancelled" | "skin.codex_exited") =>
            {
                return Err(error);
            }
            Err(error) => {
                tracing::warn!(
                    code = error.code,
                    message = error.message,
                    "poll_until_cdp_ready: connection attempt failed"
                );
            }
        }
        cancellable_sleep(cancel, CODEX_PAGE_POLL_INTERVAL).await?;
    }
    Err(cdp_unavailable_for(host))
}

/// WorkBuddy 可避开被其它本机进程占用的默认端口；Codex 保持既有固定端点。
fn available_launch_endpoint(host: SkinHostKind) -> Result<CdpEndpoint, AppError> {
    available_launch_endpoint_excluding(host, &[])
}

/// 为 WorkBuddy 选择未被占用且未在本轮失败过的端口，Codex 则保持固定端点。
fn available_launch_endpoint_excluding(
    host: SkinHostKind,
    excluded_ports: &[u16],
) -> Result<CdpEndpoint, AppError> {
    if host == SkinHostKind::WorkBuddy && cfg!(target_os = "windows") {
        return available_debug_port_for_excluding(host, &[], excluded_ports).map(CdpEndpoint::new);
    }
    let endpoint = CdpEndpoint::default_for(host);
    if excluded_ports.contains(&endpoint.port)
        || std::net::TcpListener::bind(("127.0.0.1", endpoint.port)).is_err()
    {
        return Err(AppError::new(
            "skin.cdp_unavailable",
            format!("{} 的本机调试端口不可用，未启动宿主。", host.display_name()),
        ));
    }
    Ok(endpoint)
}

/// 构造带宿主名称的 CDP 就绪超时错误，避免暴露端点细节。
fn cdp_unavailable_for(host: SkinHostKind) -> AppError {
    AppError::new(
        "skin.cdp_unavailable",
        format!(
            "{} 未在 15 秒内开放可验证的本机调试页面。",
            host.display_name()
        ),
    )
}

/// 已验证端点的既有连接偶发一次性 CDP 抖动（如目标枚举暂时为空），
/// 短暂重试几次再判定为「未开放调试端口」，避免误判成需要重启宿主。
const HOST_RUNTIME_STATUS_ATTEMPTS: u32 = 3;
const HOST_RUNTIME_STATUS_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(400);

/// 探测宿主当前运行与 CDP 可用状态，并返回本轮验证过的端点。
async fn host_runtime_status(
    host: SkinHostKind,
    preferred_endpoint: Option<CdpEndpoint>,
) -> Result<(CodexRuntimeStatus, Option<CdpEndpoint>), AppError> {
    // 抖动重试只对「已验证过的端点」有意义；没有 preferred_endpoint 时宿主大概率根本没在跑，
    // 重试只会白白重复一次进程枚举 + 连接尝试并叠加等待时间。
    let attempts = if preferred_endpoint.is_some() {
        HOST_RUNTIME_STATUS_ATTEMPTS
    } else {
        1
    };
    for attempt in 1..=attempts {
        let (_cancel_tx, mut cancel_rx) = watch::channel(false);
        match connect_existing_browser_with_preferred(host, &mut cancel_rx, preferred_endpoint)
            .await
        {
            Ok((browser, mut handler_task, endpoint, _)) => {
                handler_task.abort();
                drop(browser);
                return Ok((
                    CodexRuntimeStatus::new(classify_codex_runtime(true, false)),
                    Some(endpoint),
                ));
            }
            Err(error) if is_host_inspection_error(error.code) => {
                return Err(error);
            }
            Err(error) => {
                tracing::warn!(
                    attempt,
                    code = error.code,
                    message = error.message,
                    "host_runtime_status: existing connection check failed"
                );
                if attempt < attempts {
                    tokio::time::sleep(HOST_RUNTIME_STATUS_RETRY_DELAY).await;
                }
            }
        }
    }
    Ok((
        CodexRuntimeStatus::new(classify_codex_runtime(
            false,
            platform_host_is_running(host).await?,
        )),
        None,
    ))
}

/// 按 CDP 是否就绪与图形进程是否在运行归类宿主状态。
fn classify_codex_runtime(cdp_ready: bool, gui_running: bool) -> CodexRuntimeState {
    if cdp_ready {
        CodexRuntimeState::Ready
    } else if gui_running {
        CodexRuntimeState::RunningWithoutCdp
    } else {
        CodexRuntimeState::Stopped
    }
}

/// 确保宿主处于可换肤状态：已就绪直接返回，运行但无调试端口时要求用户先退出，未运行时以空闲端口启动并等待就绪。
async fn launch_and_wait_for_cdp(
    host: SkinHostKind,
    cancel: &mut watch::Receiver<bool>,
    preferred_endpoint: Option<CdpEndpoint>,
) -> Result<(CodexRuntimeStatus, Option<CdpEndpoint>), AppError> {
    let current = run_cancellable(cancel, host_runtime_status(host, preferred_endpoint)).await?;
    match current.0.state {
        CodexRuntimeState::Ready => {
            return Ok(current);
        }
        CodexRuntimeState::RunningWithoutCdp => return Err(manual_close_required_for(host)),
        CodexRuntimeState::Stopped => {}
    }
    let endpoint = available_launch_endpoint(host)?;
    run_cancellable(cancel, launch_platform_host(host, endpoint.port)).await?;
    match wait_for_cdp_ready(host, cancel, endpoint).await {
        Ok(result) => Ok(result),
        Err(error) => Err(remap_workbuddy_connection_error(host, error).await),
    }
}

/// 必要时强制关闭宿主后以未排除的端口重新启动并等待 CDP 就绪；可复用的就绪宿主直接返回。关闭前须持有宿主操作授权，关闭超时报错。
async fn force_launch_and_wait_for_cdp(
    host: SkinHostKind,
    cancel: &mut watch::Receiver<bool>,
    preferred_endpoint: Option<CdpEndpoint>,
    excluded_ports: &[u16],
    attempted_launch_endpoint: &mut Option<CdpEndpoint>,
) -> Result<(CodexRuntimeStatus, Option<CdpEndpoint>), AppError> {
    let current = run_cancellable(cancel, host_runtime_status(host, preferred_endpoint)).await?;
    let close_required = match current.0.state {
        CodexRuntimeState::Ready => {
            let process_roots = if host == SkinHostKind::WorkBuddy {
                run_cancellable(cancel, platform_host_processes(host))
                    .await?
                    .len()
            } else {
                0
            };
            if ready_runtime_can_be_reused(host, process_roots) {
                return Ok(current);
            }
            true
        }
        CodexRuntimeState::RunningWithoutCdp => true,
        CodexRuntimeState::Stopped => false,
    };
    if close_required {
        check_host_authority()?;
        if host == SkinHostKind::WorkBuddy && cfg!(target_os = "windows") {
            if *cancel.borrow() {
                return Err(operation_cancelled());
            }
            force_close_platform_host(host).await?;
            if platform_host_is_running(host).await? {
                return Err(force_close_timeout_error(host));
            }
            if *cancel.borrow() || check_host_authority().is_err() {
                return Err(AppError::new(
                    "skin.workbuddy_recovery_cancelled_after_close",
                    "已停止恢复；旧 WorkBuddy 已关闭，未启动新实例。",
                ));
            }
        } else {
            run_cancellable(cancel, force_close_platform_host(host)).await?;
            let deadline = tokio::time::Instant::now() + CODEX_FORCE_CLOSE_TIMEOUT;
            while tokio::time::Instant::now() < deadline {
                if *cancel.borrow() {
                    return Err(operation_cancelled());
                }
                if !run_cancellable(cancel, platform_host_is_running(host)).await? {
                    break;
                }
                cancellable_sleep(cancel, CODEX_PAGE_POLL_INTERVAL).await?;
            }
            if run_cancellable(cancel, platform_host_is_running(host)).await? {
                return Err(force_close_timeout_error(host));
            }
        }
    }
    let endpoint = available_launch_endpoint_excluding(host, excluded_ports)?;
    *attempted_launch_endpoint = Some(endpoint);
    run_cancellable(cancel, launch_platform_host(host, endpoint.port)).await?;
    wait_for_cdp_ready(host, cancel, endpoint).await
}

/// WorkBuddy 只有在恰好一个可信树根时才能复用 CDP；零根与多根都必须重新收敛。
fn ready_runtime_can_be_reused(host: SkinHostKind, process_roots: usize) -> bool {
    host != SkinHostKind::WorkBuddy || process_roots == 1
}

/// 宿主未能在强制关闭期限内退出时的错误。
fn force_close_timeout_error(host: SkinHostKind) -> AppError {
    AppError::new(
        match host {
            SkinHostKind::Codex => "skin.codex_force_close_timeout",
            SkinHostKind::WorkBuddy => "skin.workbuddy_force_close_timeout",
        },
        format!(
            "{} 未能在 15 秒内关闭，请保存工作后手动退出。",
            host.display_name()
        ),
    )
}

/// 等待新启动宿主的 CDP 就绪，确认后立即断开验证连接并返回就绪状态与端点。
async fn wait_for_cdp_ready(
    host: SkinHostKind,
    cancel: &mut watch::Receiver<bool>,
    endpoint: CdpEndpoint,
) -> Result<(CodexRuntimeStatus, Option<CdpEndpoint>), AppError> {
    let (browser, mut handler_task, _, _) = poll_until_cdp_ready(host, cancel, endpoint).await?;
    handler_task.abort();
    drop(browser);
    Ok((
        CodexRuntimeStatus::new(CodexRuntimeState::Ready),
        Some(endpoint),
    ))
}

/// 按宿主返回需要用户先保存工作并确认恢复的稳定错误。
fn manual_close_required_for(host: SkinHostKind) -> AppError {
    match host {
        SkinHostKind::Codex => AppError::new(
            "skin.codex_manual_close_required",
            "Codex 正在运行但未开放皮肤所需的调试端口。请保存工作并手动完全退出，再由LokiMetris启动。",
        ),
        SkinHostKind::WorkBuddy => AppError::new(
            "skin.workbuddy_recovery_required",
            "WorkBuddy 正在运行但未开放皮肤所需的调试端口，请保存工作并确认恢复调试连接。",
        ),
    }
}

/// 只有调试连接类故障且 WorkBuddy 仍在运行时，才请求用户确认全量恢复。
fn should_request_workbuddy_recovery(
    host: SkinHostKind,
    error_code: &str,
    host_running: bool,
) -> bool {
    host == SkinHostKind::WorkBuddy && host_running && is_workbuddy_connection_error(error_code)
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - cdp_connect_workbuddy_recovery.rs：判断错误是否属于可由 WorkBuddy 调试连接恢复流程处理的连接故障。；身份、进程或端口 owner 检查失败属于权威故障，禁止降级成破坏性恢复提示。；将 WorkBuddy 仍存活时的不可用 CDP 统一映射…
include!("cdp_connect_workbuddy_recovery.rs");
