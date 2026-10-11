// 本文件是 injection.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：执行换皮宿主内部的 `remove_from_browser` 步骤。；新注入与停用复用同一清理确认，防止旧状态冒充新载荷的成功结果。；汇总跨页面清理结果；任一页面失败都拒绝伪报完整成功。

/// 执行换皮宿主内部的 `remove_from_browser` 步骤。
async fn remove_from_browser(host: SkinHostKind, browser: &Browser) -> Result<usize, AppError> {
    let pages = browser_pages(browser).await?;
    let results = join_all(pages.into_iter().map(|page| async move {
        if !is_host_page(host, &page).await? {
            return Ok(false);
        }
        remove_skin_from_page(&page).await
    }))
    .await;
    let mut removed = 0;
    let mut failed = 0;
    for result in results {
        match result {
            Ok(true) => removed += 1,
            Ok(false) => {}
            Err(_) => failed += 1,
        }
    }
    if failed > 0 {
        tracing::warn!("皮肤页面清理部分失败，已清理页面数={removed}，失败页面数={failed}");
    }
    finish_cleanup_report(removed, failed)
}

/// 新注入与停用复用同一清理确认，防止旧状态冒充新载荷的成功结果。
async fn remove_skin_from_page(page: &Page) -> Result<bool, AppError> {
    let value = tokio::time::timeout(CDP_REQUEST_TIMEOUT, page.evaluate_expression(REMOVE_SCRIPT))
        .await
        .map_err(|_| cdp_timeout("skin.cdp_request_timeout", "宿主皮肤页面清理超时。"))?
        .map_err(cdp_error)?;
    value
        .into_value::<bool>()
        .map_err(|_| cdp_response_error())
        .and_then(|removed| {
            removed
                .then_some(true)
                .ok_or_else(|| AppError::new("skin.cdp_cleanup_failed", "宿主皮肤清理脚本未完成。"))
        })
}

/// 汇总跨页面清理结果；任一页面失败都拒绝伪报完整成功。
fn finish_cleanup_report(removed: usize, failed: usize) -> Result<usize, AppError> {
    if failed > 0 {
        Err(AppError::with_details(
            "skin.cdp_cleanup_failed",
            "未能完成全部宿主皮肤页面清理，请稍后重试。",
            vec![
                format!("removed_pages={removed}"),
                format!("failed_pages={failed}"),
            ],
        ))
    } else {
        Ok(removed)
    }
}

/// 执行换皮宿主内部的 `remove_from_existing_endpoint` 步骤。
async fn remove_from_existing_endpoint(host: SkinHostKind) -> Result<usize, AppError> {
    let (_cancel_tx, mut cancel_rx) = watch::channel(false);
    let connection = connect_existing_browser(host, &mut cancel_rx).await;
    let (browser, handler_task) = match connection {
        Ok((browser, handler_task, _)) => (browser, handler_task),
        Err(_error) if matches!(platform_host_is_running(host).await, Ok(false)) => return Ok(0),
        Err(error) => return Err(error),
    };
    cleanup_via(host, browser, handler_task).await
}

/// 执行换皮宿主内部的 `remove_from_endpoint` 步骤。
async fn remove_from_endpoint(
    host: SkinHostKind,
    endpoint: CdpEndpoint,
) -> Result<usize, AppError> {
    let connection = connect_browser_for_purpose(endpoint, true).await;
    let (mut browser, handler_task) = match connection {
        Ok((browser, handler_task)) => (browser, handler_task),
        Err(_error) if matches!(platform_host_is_running(host).await, Ok(false)) => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut handler_task = handler_task;
    let verified = browser_matches_host(host, &mut browser, endpoint).await;
    match verified {
        Ok(true) => {}
        Ok(false) => {
            handler_task.abort();
            return Err(AppError::new(
                "skin.cdp_rejected",
                format!("调试端点不是 {} 的可信主页面。", host.display_name()),
            ));
        }
        Err(error) => {
            handler_task.abort();
            return Err(error);
        }
    }
    cleanup_via(host, browser, handler_task).await
}

/// 执行换皮宿主内部的 `cleanup_via` 步骤。
async fn cleanup_via(
    host: SkinHostKind,
    mut browser: Browser,
    mut handler_task: HandlerTaskGuard,
) -> Result<usize, AppError> {
    let cleanup = async {
        fetch_targets(&mut browser).await?;
        remove_from_browser(host, &browser).await
    };
    let result = tokio::time::timeout(CDP_CLEANUP_TIMEOUT, cleanup)
        .await
        .unwrap_or_else(|_| {
            Err(cdp_timeout(
                "skin.cdp_cleanup_timeout",
                "Codex 皮肤清理超时。",
            ))
        });
    handler_task.abort();
    result
}

/// 执行换皮宿主内部的 `cdp_timeout` 步骤。
fn cdp_timeout(code: &'static str, message: &'static str) -> AppError {
    AppError::new(code, message)
}

/// 执行换皮宿主内部的 `cdp_error` 步骤。
fn cdp_error(_: chromiumoxide::error::CdpError) -> AppError {
    AppError::new("skin.cdp_failed", "与 Codex 调试会话通信失败。")
}

/// 执行换皮宿主内部的 `cdp_response_error` 步骤。
fn cdp_response_error() -> AppError {
    AppError::new(
        "skin.cdp_response_invalid",
        "Codex 调试页面返回了无效响应。",
    )
}

/// 执行换皮宿主内部的 `is_allowed_websocket` 步骤。
fn is_allowed_websocket(address: &str, endpoint: CdpEndpoint) -> bool {
    [
        format!("ws://127.0.0.1:{}/", endpoint.port),
        format!("ws://localhost:{}/", endpoint.port),
        format!("ws://[::1]:{}/", endpoint.port),
    ]
    .iter()
    .any(|prefix| address.starts_with(prefix))
}

/// 其它应用的皮肤仍在运行时不接管共享CSS表面，也不执行其cleanup。
async fn ensure_exclusive_skin_owner(page: &Page) -> Result<(), AppError> {
    let value = tokio::time::timeout(
        CDP_REQUEST_TIMEOUT,
        page.evaluate_expression(
            "(() => !window.__CODEX_DREAM_SKIN_STATE__ && !window.__BIFANG_CODEX_SKIN_COMPAT__)()",
        ),
    )
    .await
    .map_err(|_| cdp_timeout("skin.cdp_request_timeout", "宿主皮肤归属检查超时。"))?
    .map_err(cdp_error)?;
    if value
        .into_value::<bool>()
        .map_err(|_| cdp_response_error())?
    {
        Ok(())
    } else {
        Err(foreign_skin_owner_error())
    }
}
