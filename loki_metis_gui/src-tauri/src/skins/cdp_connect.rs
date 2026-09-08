/// 执行换皮宿主内部的 `connect_or_launch` 步骤。
async fn connect_or_launch(
    host: SkinHostKind,
    cancel: &mut watch::Receiver<bool>,
    selected: Option<&ResolvedCodexInstance>,
    preferred_endpoint: Option<CdpEndpoint>,
) -> Result<(Browser, JoinHandle<()>, ConnectionSource, CdpEndpoint), AppError> {
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
        let verified = match run_cancellable(
            cancel,
            browser_matches_host(host, &mut browser, endpoint),
        )
        .await
        {
            Ok(verified) => verified,
            Err(error) => {
                handler_task.abort();
                drop(browser);
                return Err(remap_workbuddy_connection_error(host, error).await);
            }
        };
        if !verified {
            handler_task.abort();
            drop(browser);
            return Err(
                remap_workbuddy_connection_error(
                    host,
                    AppError::new(
                        "skin.cdp_rejected",
                        format!("调试端点不是 {} 的唯一可信主页面。", host.display_name()),
                    ),
                )
                .await,
            );
        }
        return Ok((browser, handler_task, ConnectionSource::Existing, endpoint));
    }
    match connect_existing_browser_with_preferred(host, cancel, preferred_endpoint).await {
        Ok((browser, handler_task, endpoint)) => {
            return Ok((browser, handler_task, ConnectionSource::Existing, endpoint));
        }
        Err(error) if error.code == "skin.operation_cancelled" => return Err(error),
        Err(_) => {}
    }
    if run_cancellable(cancel, platform_host_is_running(host)).await? {
        return Err(manual_close_required_for(host));
    }
    let endpoint = available_launch_endpoint(host)?;
    run_cancellable(cancel, launch_platform_host(host, endpoint.port)).await?;
    let (browser, handler_task, endpoint) = poll_until_cdp_ready(host, cancel, endpoint).await?;
    Ok((browser, handler_task, ConnectionSource::Launched, endpoint))
}

/// 执行换皮宿主内部的 `poll_until_cdp_ready` 步骤。
async fn poll_until_cdp_ready(
    host: SkinHostKind,
    cancel: &mut watch::Receiver<bool>,
    endpoint: CdpEndpoint,
) -> Result<(Browser, JoinHandle<()>, CdpEndpoint), AppError> {
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
                let verified = run_cancellable(
                    cancel,
                    browser_matches_host(host, &mut browser, endpoint),
                )
                .await;
                match verified {
                    Ok(true) => return Ok((browser, handler_task, endpoint)),
                    Err(error) if error.code == "skin.operation_cancelled" => {
                        handler_task.abort();
                        drop(browser);
                        return Err(error);
                    }
                    Err(error)
                        if matches!(
                            error.code,
                            "skin.workbuddy_cdp_owner_inspection_failed"
                                | "skin.workbuddy_process_inspection_failed"
                        ) =>
                    {
                        handler_task.abort();
                        drop(browser);
                        return Err(error);
                    }
                    Ok(false) | Err(_) => {
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
            Err(_) => {}
        }
        cancellable_sleep(cancel, CODEX_PAGE_POLL_INTERVAL).await?;
    }
    Err(cdp_unavailable_for(host))
}

/// WorkBuddy 可避开被其它本机进程占用的默认端口；Codex 保持既有固定端点。
fn available_launch_endpoint(host: SkinHostKind) -> Result<CdpEndpoint, AppError> {
    available_launch_endpoint_excluding(host, &[])
}

fn available_launch_endpoint_excluding(
    host: SkinHostKind,
    excluded_ports: &[u16],
) -> Result<CdpEndpoint, AppError> {
    if host == SkinHostKind::WorkBuddy && cfg!(target_os = "windows") {
        return available_debug_port_for_excluding(host, &[], excluded_ports)
            .map(CdpEndpoint::new);
    }
    Ok(CdpEndpoint::default_for(host))
}

fn cdp_unavailable_for(host: SkinHostKind) -> AppError {
    AppError::new(
        "skin.cdp_unavailable",
        format!("{} 未在 15 秒内开放可验证的本机调试页面。", host.display_name()),
    )
}

async fn host_runtime_status(
    host: SkinHostKind,
    preferred_endpoint: Option<CdpEndpoint>,
) -> Result<(CodexRuntimeStatus, Option<CdpEndpoint>), AppError> {
    let (_cancel_tx, mut cancel_rx) = watch::channel(false);
    match connect_existing_browser_with_preferred(host, &mut cancel_rx, preferred_endpoint).await {
        Ok((browser, handler_task, endpoint)) => {
            handler_task.abort();
            drop(browser);
            return Ok((
                CodexRuntimeStatus::new(classify_codex_runtime(true, false)),
                Some(endpoint),
            ));
        }
        Err(error)
            if matches!(
                error.code,
                "skin.workbuddy_cdp_owner_inspection_failed"
                    | "skin.workbuddy_process_inspection_failed"
            ) =>
        {
            return Err(error);
        }
        Err(_) => {}
    }
    Ok((
        CodexRuntimeStatus::new(classify_codex_runtime(
            false,
            platform_host_is_running(host).await?,
        )),
        None,
    ))
}

/// 执行换皮宿主内部的 `classify_codex_runtime` 步骤。
fn classify_codex_runtime(cdp_ready: bool, gui_running: bool) -> CodexRuntimeState {
    if cdp_ready {
        CodexRuntimeState::Ready
    } else if gui_running {
        CodexRuntimeState::RunningWithoutCdp
    } else {
        CodexRuntimeState::Stopped
    }
}

/// 执行换皮宿主内部的 `launch_and_wait_for_cdp` 步骤。
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

/// 执行换皮宿主内部的 `force_launch_and_wait_for_cdp` 步骤。
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
        if host == SkinHostKind::WorkBuddy && cfg!(target_os = "windows") {
            if *cancel.borrow() {
                return Err(operation_cancelled());
            }
            force_close_platform_host(host).await?;
            if platform_host_is_running(host).await? {
                return Err(force_close_timeout_error(host));
            }
            if *cancel.borrow() {
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

/// 执行换皮宿主内部的 `force_close_timeout_error` 步骤。
fn force_close_timeout_error(host: SkinHostKind) -> AppError {
    AppError::new(
        match host {
            SkinHostKind::Codex => "skin.codex_force_close_timeout",
            SkinHostKind::WorkBuddy => "skin.workbuddy_force_close_timeout",
        },
        format!("{} 未能在 15 秒内关闭，请保存工作后手动退出。", host.display_name()),
    )
}

/// 执行换皮宿主内部的 `wait_for_cdp_ready` 步骤。
async fn wait_for_cdp_ready(
    host: SkinHostKind,
    cancel: &mut watch::Receiver<bool>,
    endpoint: CdpEndpoint,
) -> Result<(CodexRuntimeStatus, Option<CdpEndpoint>), AppError> {
    let (browser, handler_task, _) = poll_until_cdp_ready(host, cancel, endpoint).await?;
    handler_task.abort();
    drop(browser);
    Ok((
        CodexRuntimeStatus::new(CodexRuntimeState::Ready),
        Some(endpoint),
    ))
}

/// 执行换皮宿主内部的 `manual_close_required` 步骤。
fn manual_close_required() -> AppError {
    manual_close_required_for(SkinHostKind::Codex)
}

fn manual_close_required_for(host: SkinHostKind) -> AppError {
    match host {
        SkinHostKind::Codex => AppError::new(
            "skin.codex_manual_close_required",
            "Codex 正在运行但未开放皮肤所需的调试端口。请保存工作并手动完全退出，再由LokiMetis启动。",
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

fn is_workbuddy_connection_error(error_code: &str) -> bool {
    matches!(
        error_code,
        "skin.cdp_connect_timeout"
            | "skin.cdp_failed"
            | "skin.cdp_request_timeout"
            | "skin.cdp_response_invalid"
            | "skin.cdp_rejected"
            | "skin.cdp_unavailable"
            | "skin.workbuddy_page_not_found"
    )
}

/// 将 WorkBuddy 仍存活时的不可用 CDP 统一映射为前端可确认的恢复请求。
async fn remap_workbuddy_connection_error(host: SkinHostKind, error: AppError) -> AppError {
    if host != SkinHostKind::WorkBuddy || error.code == "skin.operation_cancelled" {
        return error;
    }
    let host_running = platform_host_is_running(host).await.unwrap_or(false);
    if should_request_workbuddy_recovery(host, error.code, host_running) {
        manual_close_required_for(host)
    } else if is_workbuddy_connection_error(error.code) {
        AppError {
            message: error.message.replace("Codex", "WorkBuddy"),
            ..error
        }
    } else {
        error
    }
}

/// 执行换皮宿主内部的 `connect_existing_browser` 步骤。
async fn connect_existing_browser(
    host: SkinHostKind,
    cancel: &mut watch::Receiver<bool>,
) -> Result<(Browser, JoinHandle<()>, CdpEndpoint), AppError> {
    connect_existing_browser_with_preferred(host, cancel, None).await
}

/// 连接宿主时首先复核服务刚刚验证过的动态端点，再尝试命令行与默认端口。
async fn connect_existing_browser_with_preferred(
    host: SkinHostKind,
    cancel: &mut watch::Receiver<bool>,
    preferred_endpoint: Option<CdpEndpoint>,
) -> Result<(Browser, JoinHandle<()>, CdpEndpoint), AppError> {
    let candidates = run_cancellable(cancel, async {
        Ok(cdp_endpoint_candidates_for(host, preferred_endpoint).await)
    })
    .await?;
    let mut last_error = None;
    for endpoint in candidates {
        match run_cancellable(cancel, connect_browser(endpoint)).await {
            Ok((mut browser, handler_task)) => {
                let verified = browser_matches_host(host, &mut browser, endpoint).await;
                match verified {
                    Ok(true) => return Ok((browser, handler_task, endpoint)),
                    Ok(false) => {
                        handler_task.abort();
                        drop(browser);
                        last_error = Some(AppError::new(
                            "skin.cdp_rejected",
                            format!("调试端点不是 {} 主页面。", host.display_name()),
                        ));
                    }
                    Err(error)
                        if matches!(
                            error.code,
                            "skin.workbuddy_cdp_owner_inspection_failed"
                                | "skin.workbuddy_process_inspection_failed"
                        ) =>
                    {
                        handler_task.abort();
                        drop(browser);
                        return Err(error);
                    }
                    Err(error) => {
                        handler_task.abort();
                        drop(browser);
                        last_error = Some(error);
                    }
                }
            }
            Err(error) if error.code == "skin.operation_cancelled" => return Err(error),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        AppError::new(
            "skin.cdp_unavailable",
            format!("没有可用的 {} 本机调试端点。", host.display_name()),
        )
    }))
}

async fn browser_matches_host(
    host: SkinHostKind,
    browser: &mut Browser,
    endpoint: CdpEndpoint,
) -> Result<bool, AppError> {
    fetch_targets(browser).await?;
    for page in browser_pages(browser).await? {
        if is_host_page(host, &page).await? {
            return if host == SkinHostKind::WorkBuddy {
                let processes = platform_host_processes(host).await?;
                let [root] = processes.as_slice() else {
                    return Ok(false);
                };
                platform_workbuddy_endpoint_owned_by_root(endpoint.port, root.pid).await
            } else {
                Ok(true)
            };
        }
    }
    Ok(false)
}

/// 执行换皮宿主内部的 `connect_browser` 步骤。
async fn connect_browser(endpoint: CdpEndpoint) -> Result<(Browser, JoinHandle<()>), AppError> {
    let config = HandlerConfig {
        request_timeout: CDP_REQUEST_TIMEOUT,
        ..HandlerConfig::default()
    };
    let (browser, mut handler) = tokio::time::timeout(
        CDP_CONNECT_TIMEOUT,
        Browser::connect_with_config(endpoint.url(), config),
    )
    .await
    .map_err(|_| cdp_timeout("skin.cdp_connect_timeout", "连接 Codex 调试端口超时。"))?
    .map_err(cdp_error)?;
    if !is_allowed_websocket(browser.websocket_address(), endpoint) {
        return Err(AppError::new(
            "skin.cdp_rejected",
            "调试端点返回了非本机 WebSocket 地址，连接已拒绝。",
        ));
    }
    let handler_task = tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            if event.is_err() {
                break;
            }
        }
    });
    Ok((browser, handler_task))
}

/// 执行换皮宿主内部的 `wait_for_initial_injection` 步骤。
async fn wait_for_initial_injection(
    host: SkinHostKind,
    browser: Browser,
    handler_task: JoinHandle<()>,
    payload: &Arc<str>,
    skin: &SkinReference,
    page_ready_timeout: Duration,
    endpoint: CdpEndpoint,
    transaction_id: &str,
    cancel: &mut watch::Receiver<bool>,
) -> Result<(Browser, JoinHandle<()>, InjectionReport), AppError> {
    let mut browser = browser;
    let mut handler_task = HandlerTaskGuard::new(handler_task);
    let transaction = InjectionTransaction::new(transaction_id);
    let result = {
        let wait = async {
            tokio::time::timeout(
                page_ready_timeout,
                wait_for_initial_injection_inner(
                    host,
                    &mut browser,
                    &mut handler_task,
                    payload,
                    skin,
                    page_ready_timeout,
                    endpoint,
                    &transaction,
                ),
            )
            .await
            .map_err(|_| host_page_not_found(host))?
        };
        monitor_host_operation(host, cancel, wait).await
    };
    match result {
        Ok(report) => match handler_task.take() {
            Some(handler_task) => Ok((browser, handler_task, report)),
            None => {
                let error = AppError::new("skin.cdp_failed", "宿主调试会话已意外结束。");
                let transaction_targets = transaction.tracked_targets();
                let rollback = rollback_initial_injection(
                    host,
                    &mut browser,
                    &mut handler_task,
                    endpoint,
                    transaction_id,
                    &transaction_targets,
                )
                .await;
                Err(match rollback {
                    Ok(_) => error,
                    Err(rollback) => rollback_error(&error, rollback),
                })
            }
        },
        Err(error) => {
            let transaction_targets = transaction.tracked_targets();
            let rollback = rollback_initial_injection(
                host,
                &mut browser,
                &mut handler_task,
                endpoint,
                transaction_id,
                &transaction_targets,
            )
            .await;
            handler_task.abort();
            Err(match rollback {
                Ok(_) => error,
                Err(rollback) => rollback_error(&error, rollback),
            })
        }
    }
}

/// 回滚优先复用当前会话；会话失效时仅对同一端点重连并重新验证宿主一次。
async fn rollback_initial_injection(
    host: SkinHostKind,
    browser: &mut Browser,
    handler_task: &mut HandlerTaskGuard,
    endpoint: CdpEndpoint,
    transaction_id: &str,
    transaction_targets: &BTreeSet<String>,
) -> Result<usize, AppError> {
    let first_error = match rollback_marked_injection_pages(
        browser,
        transaction_id,
        transaction_targets,
    )
    .await
    {
        Ok(removed) => return Ok(removed),
        Err(error) => error,
    };
    if matches!(platform_host_is_running(host).await, Ok(false)) {
        return Ok(0);
    }
    let (mut next_browser, next_handler_task) = match connect_browser(endpoint).await {
        Ok(connection) => connection,
        Err(_) => return Err(first_error),
    };
    match browser_matches_host(host, &mut next_browser, endpoint).await {
        Ok(true) => {
            handler_task.replace(next_handler_task);
            *browser = next_browser;
            rollback_marked_injection_pages(browser, transaction_id, transaction_targets).await
        }
        Ok(false) | Err(_) => {
            next_handler_task.abort();
            Err(first_error)
        }
    }
}

/// 执行换皮宿主内部的 `wait_for_initial_appearance` 步骤。
async fn wait_for_initial_appearance(
    host: SkinHostKind,
    browser: &mut Browser,
    page_ready_timeout: Duration,
    policy: &AppearancePolicy,
    cancel: &mut watch::Receiver<bool>,
) -> Result<SkinAppearanceCheck, AppError> {
    let wait = async {
        let deadline = tokio::time::Instant::now() + page_ready_timeout;
        loop {
            if tokio::time::Instant::now() >= deadline {
                return Err(host_page_not_found(host));
            }
            fetch_targets(browser).await?;
            for page in browser_pages(browser).await? {
                if is_host_page(host, &page).await? {
                    let probe = probe_appearance(
                        &page,
                        policy
                            .requirements
                            .as_ref()
                            .is_some_and(AppearanceRequirements::has_fields),
                    )
                    .await?;
                    return Ok(build_appearance_check(policy, &probe));
                }
            }
            tokio::time::sleep(CODEX_PAGE_POLL_INTERVAL).await;
        }
    };
    monitor_host_operation(host, cancel, wait).await
}

/// 执行换皮宿主内部的 `probe_appearance` 步骤。
async fn probe_appearance(page: &Page, include_details: bool) -> Result<AppearanceProbe, AppError> {
    let script = if include_details {
        APPEARANCE_PROBE_SCRIPT
    } else {
        APPEARANCE_MODE_SCRIPT
    };
    let result = tokio::time::timeout(CDP_REQUEST_TIMEOUT, page.evaluate_expression(script))
        .await
        .map_err(|_| cdp_timeout("skin.cdp_request_timeout", "Codex 外观检测超时。"))?
        .map_err(cdp_error)?;
    result
        .into_value::<AppearanceProbe>()
        .map_err(|_| cdp_response_error())
}
