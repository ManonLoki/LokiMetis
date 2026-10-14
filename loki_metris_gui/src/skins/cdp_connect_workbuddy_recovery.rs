// 本文件是 cdp_connect.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：判断错误是否属于可由 WorkBuddy 调试连接恢复流程处理的连接故障。；身份、进程或端口 owner 检查失败属于权威故障，禁止降级成破坏性恢复提示。；将 WorkBuddy 仍存活时的不可用 CDP 统一映射为界面可确认的恢复请求。

/// 判断错误是否属于可由 WorkBuddy 调试连接恢复流程处理的连接故障。
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

/// 身份、进程或端口 owner 检查失败属于权威故障，禁止降级成破坏性恢复提示。
fn is_host_inspection_error(error_code: &str) -> bool {
    matches!(
        error_code,
        "skin.codex_cdp_owner_inspection_failed"
            | "skin.codex_process_inspection_failed"
            | "skin.codex_process_inspection_timeout"
            | "skin.workbuddy_cdp_owner_inspection_failed"
            | "skin.workbuddy_process_inspection_failed"
            | "skin.workbuddy_process_inspection_timeout"
    )
}

/// 将 WorkBuddy 仍存活时的不可用 CDP 统一映射为界面可确认的恢复请求。
async fn remap_workbuddy_connection_error(host: SkinHostKind, error: AppError) -> AppError {
    if host != SkinHostKind::WorkBuddy || error.code == "skin.operation_cancelled" {
        return error;
    }
    let host_running = match platform_host_is_running(host).await {
        Ok(host_running) => host_running,
        Err(inspection_error) => return inspection_error,
    };
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

/// 连接已在运行的宿主（用于退出清理）；handler 已结束时视为会话失效。
async fn connect_existing_browser(
    host: SkinHostKind,
    cancel: &mut watch::Receiver<bool>,
) -> Result<(Browser, HandlerTaskGuard, CdpEndpoint), AppError> {
    let (browser, handler_task, endpoint, _) =
        connect_existing_browser_for_purpose(host, cancel, None, true).await?;
    if handler_task.is_finished() {
        drop(browser);
        return Err(AppError::new("skin.cdp_failed", "宿主调试会话已意外结束。"));
    }
    Ok((browser, handler_task, endpoint))
}

/// 连接宿主时首先复核服务刚刚验证过的动态端点，再尝试受信命令行声明的端口。
async fn connect_existing_browser_with_preferred(
    host: SkinHostKind,
    cancel: &mut watch::Receiver<bool>,
    preferred_endpoint: Option<CdpEndpoint>,
) -> Result<(Browser, HandlerTaskGuard, CdpEndpoint, u32), AppError> {
    connect_existing_browser_for_purpose(host, cancel, preferred_endpoint, false).await
}
/// 普通探针与退出清理使用相同宿主验证，但准入用途分别登记。
async fn connect_existing_browser_for_purpose(
    host: SkinHostKind,
    cancel: &mut watch::Receiver<bool>,
    preferred_endpoint: Option<CdpEndpoint>,
    cleanup: bool,
) -> Result<(Browser, HandlerTaskGuard, CdpEndpoint, u32), AppError> {
    let candidates = run_cancellable(
        cancel,
        cdp_endpoint_candidates_for(host, preferred_endpoint),
    )
    .await?;
    let mut last_error = None;
    for endpoint in candidates {
        match run_cancellable(cancel, connect_browser_for_purpose(endpoint, cleanup)).await {
            Ok((mut browser, handler_task)) => {
                let (mut handler_task, verified_root_pid) = hold_connected_handler_during(
                    handler_task,
                    browser_host_root_pid(host, &mut browser, endpoint, None),
                )
                .await;
                match verified_root_pid {
                    Ok(Some(root_pid)) => {
                        return Ok((browser, handler_task, endpoint, root_pid));
                    }
                    Ok(None) => {
                        handler_task.abort();
                        drop(browser);
                        last_error = Some(AppError::new(
                            "skin.cdp_rejected",
                            format!("调试端点不是 {} 主页面。", host.display_name()),
                        ));
                    }
                    Err(error) if is_host_inspection_error(error.code) => {
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

include!("cdp_identity.rs");

/// 连接指定端点的浏览器（普通用途）。
async fn connect_browser(endpoint: CdpEndpoint) -> Result<(Browser, HandlerTaskGuard), AppError> {
    connect_browser_for_purpose(endpoint, false).await
}
/// 只有退出后的页面清理可使用Draining阶段的连接容量。
async fn connect_browser_for_purpose(
    endpoint: CdpEndpoint,
    cleanup: bool,
) -> Result<(Browser, HandlerTaskGuard), AppError> {
    let reservation = reserve_handler_task_for_purpose(cleanup)?;
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
    Ok((browser, reservation.bind(handler_task)))
}

/// 先复核端点仍绑定预期宿主根进程，再在页面就绪期限内完成首批注入；期间响应取消，失败时由调用方回收 handler。
async fn wait_for_initial_injection(
    host: SkinHostKind,
    browser: Browser,
    handler_task: HandlerTaskGuard,
    payload: &Arc<str>,
    skin: &SkinReference,
    page_ready_timeout: Duration,
    endpoint: CdpEndpoint,
    expected_host_root_pid: u32,
    transaction_id: &str,
    cancel: &mut watch::Receiver<bool>,
) -> Result<(Browser, HandlerTaskGuard, InjectionReport), AppError> {
    let mut browser = browser;
    let mut handler_task = handler_task;
    let transaction = InjectionTransaction::new(transaction_id);
    let result = {
        let wait = async {
            verify_stable_endpoint_binding(host, &mut browser, endpoint, expected_host_root_pid)
                .await?;
            let report = tokio::time::timeout(
                page_ready_timeout,
                wait_for_initial_injection_inner(
                    host,
                    &mut browser,
                    &mut handler_task,
                    payload,
                    skin,
                    page_ready_timeout,
                    endpoint,
                    expected_host_root_pid,
                    &transaction,
                ),
            )
            .await
            .map_err(|_| host_page_not_found(host))??;
            verify_stable_endpoint_binding(host, &mut browser, endpoint, expected_host_root_pid)
                .await?;
            Ok(report)
        };
        monitor_host_operation(host, cancel, wait).await
    };
    match result {
        Ok(report) if !handler_task.is_finished() => Ok((browser, handler_task, report)),
        Ok(_) => {
            let error = AppError::new("skin.cdp_failed", "宿主调试会话已意外结束。");
            let transaction_targets = transaction.tracked_targets();
            let rollback = rollback_initial_injection(
                host,
                &mut browser,
                &mut handler_task,
                endpoint,
                expected_host_root_pid,
                transaction_id,
                &transaction_targets,
            )
            .await;
            Err(match rollback {
                Ok(_) => error,
                Err(rollback) => rollback_error(&error, rollback),
            })
        }
        Err(error) => {
            let transaction_targets = transaction.tracked_targets();
            let rollback = rollback_initial_injection(
                host,
                &mut browser,
                &mut handler_task,
                endpoint,
                expected_host_root_pid,
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
    expected_host_root_pid: u32,
    transaction_id: &str,
    transaction_targets: &BTreeSet<String>,
) -> Result<usize, AppError> {
    let first_error =
        match verify_stable_endpoint_binding(host, browser, endpoint, expected_host_root_pid).await
        {
            Ok(()) => {
                match rollback_marked_injection_pages(browser, transaction_id, transaction_targets)
                    .await
                {
                    Ok(removed) => return Ok(removed),
                    Err(error) => error,
                }
            }
            Err(error) => error,
        };
    if matches!(platform_host_is_running(host).await, Ok(false)) {
        return Ok(0);
    }
    let (mut next_browser, next_handler_task) =
        match connect_browser_for_purpose(endpoint, true).await {
            Ok(connection) => connection,
            Err(_) => return Err(first_error),
        };
    let (mut next_handler_task, verification) = hold_connected_handler_during(
        next_handler_task,
        browser_matches_host_for_root(
            host,
            &mut next_browser,
            endpoint,
            Some(expected_host_root_pid),
        ),
    )
    .await;
    match verification {
        Ok(true) => {
            if next_handler_task.is_finished() {
                return Err(first_error);
            }
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

/// 在页面就绪期限内找到第一个宿主页面并读取外观，生成安装前的外观预检结果。
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

/// 在页面执行外观探针脚本；需要比较逐项要求时读取完整外观，否则只读明暗模式。
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
