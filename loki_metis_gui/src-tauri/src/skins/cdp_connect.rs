/// 执行换皮宿主内部的 `connect_or_launch` 步骤。
async fn connect_or_launch(
    host: SkinHostKind,
    cancel: &mut watch::Receiver<bool>,
    selected: Option<&ResolvedCodexInstance>,
) -> Result<(Browser, JoinHandle<()>, ConnectionSource, CdpEndpoint), AppError> {
    if let Some(selected) = selected {
        let port = selected
            .debug_port
            .ok_or_else(|| manual_close_required_for(host))?;
        let endpoint = CdpEndpoint::new(port);
        let (browser, handler_task) = run_cancellable(cancel, connect_browser(endpoint)).await?;
        return Ok((browser, handler_task, ConnectionSource::Existing, endpoint));
    }
    match connect_existing_browser(host, cancel).await {
        Ok((browser, handler_task, endpoint)) => {
            return Ok((browser, handler_task, ConnectionSource::Existing, endpoint));
        }
        Err(error) if error.code == "skin.operation_cancelled" => return Err(error),
        Err(_) => {}
    }
    if run_cancellable(cancel, platform_host_is_running(host)).await? {
        return Err(manual_close_required_for(host));
    }
    run_cancellable(cancel, launch_platform_host(host)).await?;
    let (browser, handler_task, endpoint) = poll_until_cdp_ready(host, cancel).await?;
    Ok((browser, handler_task, ConnectionSource::Launched, endpoint))
}

/// 执行换皮宿主内部的 `poll_until_cdp_ready` 步骤。
async fn poll_until_cdp_ready(
    host: SkinHostKind,
    cancel: &mut watch::Receiver<bool>,
) -> Result<(Browser, JoinHandle<()>, CdpEndpoint), AppError> {
    let endpoint = CdpEndpoint::default_for(host);
    let deadline = tokio::time::Instant::now() + CODEX_LAUNCH_TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let connection = async {
            tokio::time::timeout(remaining, connect_browser(endpoint))
                .await
                .map_err(|_| {
                    cdp_timeout(
                        "skin.cdp_unavailable",
                        "Codex 未在 15 秒内开放本机调试端口。",
                    )
                })?
        };
        match monitor_host_operation(host, cancel, connection).await {
            Ok((browser, handler_task)) => {
                return Ok((browser, handler_task, endpoint));
            }
            Err(error)
                if matches!(error.code, "skin.operation_cancelled" | "skin.host_exited" | "skin.codex_exited") =>
            {
                return Err(error);
            }
            Err(_) => {}
        }
        cancellable_sleep(cancel, CODEX_PAGE_POLL_INTERVAL).await?;
    }
    Err(AppError::new(
        "skin.cdp_unavailable",
        "Codex 未在 15 秒内开放本机调试端口。",
    ))
}

async fn host_runtime_status(host: SkinHostKind) -> Result<CodexRuntimeStatus, AppError> {
    let (_cancel_tx, mut cancel_rx) = watch::channel(false);
    if let Ok((browser, handler_task, _)) = connect_existing_browser(host, &mut cancel_rx).await {
        handler_task.abort();
        drop(browser);
        return Ok(CodexRuntimeStatus::new(classify_codex_runtime(true, false)));
    }
    Ok(CodexRuntimeStatus::new(classify_codex_runtime(
        false,
        platform_host_is_running(host).await?,
    )))
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
) -> Result<CodexRuntimeStatus, AppError> {
    match run_cancellable(cancel, host_runtime_status(host)).await?.state {
        CodexRuntimeState::Ready => {
            return Ok(CodexRuntimeStatus::new(CodexRuntimeState::Ready));
        }
        CodexRuntimeState::RunningWithoutCdp => return Err(manual_close_required_for(host)),
        CodexRuntimeState::Stopped => {}
    }
    run_cancellable(cancel, launch_platform_host(host)).await?;
    wait_for_cdp_ready(host, cancel).await
}

/// 执行换皮宿主内部的 `force_launch_and_wait_for_cdp` 步骤。
async fn force_launch_and_wait_for_cdp(
    host: SkinHostKind,
    cancel: &mut watch::Receiver<bool>,
) -> Result<CodexRuntimeStatus, AppError> {
    match run_cancellable(cancel, host_runtime_status(host)).await?.state {
        CodexRuntimeState::Ready => {
            return Ok(CodexRuntimeStatus::new(CodexRuntimeState::Ready));
        }
        CodexRuntimeState::RunningWithoutCdp => {
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
        CodexRuntimeState::Stopped => {}
    }
    run_cancellable(cancel, launch_platform_host(host)).await?;
    wait_for_cdp_ready(host, cancel).await
}

/// 执行换皮宿主内部的 `force_close_timeout_error` 步骤。
fn force_close_timeout_error(host: SkinHostKind) -> AppError {
    AppError::new(
        "skin.codex_force_close_timeout",
        format!("{} 未能在 15 秒内关闭，请保存工作后手动退出。", host.display_name()),
    )
}

/// 执行换皮宿主内部的 `wait_for_cdp_ready` 步骤。
async fn wait_for_cdp_ready(
    host: SkinHostKind,
    cancel: &mut watch::Receiver<bool>,
) -> Result<CodexRuntimeStatus, AppError> {
    let (browser, handler_task, _) = poll_until_cdp_ready(host, cancel).await?;
    handler_task.abort();
    drop(browser);
    Ok(CodexRuntimeStatus::new(CodexRuntimeState::Ready))
}

/// 执行换皮宿主内部的 `manual_close_required` 步骤。
fn manual_close_required() -> AppError {
    manual_close_required_for(SkinHostKind::Codex)
}

fn manual_close_required_for(host: SkinHostKind) -> AppError {
    AppError::new(
        "skin.codex_manual_close_required",
        format!(
            "{} 正在运行但未开放皮肤所需的调试端口。请保存工作并手动完全退出，再由LokiMetis启动。",
            host.display_name()
        ),
    )
}

/// 执行换皮宿主内部的 `connect_existing_browser` 步骤。
async fn connect_existing_browser(
    host: SkinHostKind,
    cancel: &mut watch::Receiver<bool>,
) -> Result<(Browser, JoinHandle<()>, CdpEndpoint), AppError> {
    let candidates =
        run_cancellable(cancel, async { Ok(cdp_endpoint_candidates_for(host).await) }).await?;
    let mut last_error = None;
    for endpoint in candidates {
        match run_cancellable(cancel, connect_browser(endpoint)).await {
            Ok((mut browser, handler_task)) => {
                let verified = browser_matches_host(host, &mut browser).await.unwrap_or(false);
                if verified {
                    return Ok((browser, handler_task, endpoint));
                }
                handler_task.abort();
                drop(browser);
                last_error = Some(AppError::new(
                    "skin.cdp_rejected",
                    format!("调试端点不是 {} 主页面。", host.display_name()),
                ));
            }
            Err(error) if error.code == "skin.operation_cancelled" => return Err(error),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        AppError::new("skin.cdp_unavailable", "没有可用的 Codex 本机调试端点。")
    }))
}

async fn browser_matches_host(
    host: SkinHostKind,
    browser: &mut Browser,
) -> Result<bool, AppError> {
    fetch_targets(browser).await?;
    for page in browser_pages(browser).await? {
        if is_host_page(host, &page).await? {
            return Ok(true);
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
    cancel: &mut watch::Receiver<bool>,
) -> Result<(Browser, JoinHandle<()>, InjectionReport), AppError> {
    let wait = async {
        tokio::time::timeout(
            page_ready_timeout,
            wait_for_initial_injection_inner(
                host,
                browser,
                handler_task,
                payload,
                skin,
                page_ready_timeout,
                endpoint,
            ),
        )
        .await
        .map_err(|_| codex_page_not_found())?
    };
    monitor_host_operation(host, cancel, wait).await
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
                return Err(codex_page_not_found());
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
