/// 执行换皮宿主内部的 `watch_pages` 步骤。
async fn watch_pages(
    mut browser: Browser,
    handler_task: JoinHandle<()>,
    payload: Arc<str>,
    skin: SkinReference,
    mut cancel: watch::Receiver<bool>,
) -> Result<usize, AppError> {
    let mut interval = tokio::time::interval_at(
        tokio::time::Instant::now() + SKIN_WATCH_INTERVAL,
        SKIN_WATCH_INTERVAL,
    );
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    'watch: loop {
        tokio::select! {
            biased;
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    break;
                }
            }
            _ = interval.tick() => {
                let refresh = async {
                    fetch_targets(&mut browser).await?;
                    inject_pages(&browser, &payload, &skin).await
                };
                tokio::select! {
                    biased;
                    changed = cancel.changed() => {
                        if changed.is_err() || *cancel.borrow() {
                            break 'watch;
                        }
                    }
                    result = refresh => match result {
                        Ok(report) if report.failed_pages > 0 => tracing::warn!(
                            "皮肤页面扫描部分失败，失败页面数={}", report.failed_pages
                        ),
                        Err(error) => {
                            tracing::warn!("皮肤页面扫描失败并停止维护，错误码={}", error.code);
                            break 'watch;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    let removed = tokio::time::timeout(CDP_CLEANUP_TIMEOUT, remove_from_browser(&browser))
        .await
        .unwrap_or_else(|_| {
            Err(cdp_timeout(
                "skin.cdp_cleanup_timeout",
                "Codex 皮肤清理超时。",
            ))
        });
    handler_task.abort();
    removed
}

/// 执行换皮宿主内部的 `inject_pages` 步骤。
async fn inject_pages(
    browser: &Browser,
    payload: &Arc<str>,
    skin: &SkinReference,
) -> Result<InjectionReport, AppError> {
    let pages = browser_pages(browser).await?;
    let results = join_all(pages.into_iter().map(|page| {
        let payload = Arc::clone(payload);
        let skin = skin.clone();
        async move {
            if !is_codex_page(&page).await? {
                return Ok((false, false, CompatibilityPageReport::default()));
            }
            let compatibility = apply_host_compatibility(&page).await;
            if has_current_skin(&page, &skin).await? {
                return Ok((true, false, compatibility));
            }
            tokio::time::timeout(
                CDP_REQUEST_TIMEOUT,
                page.evaluate_expression(payload.to_string()),
            )
            .await
            .map_err(|_| cdp_timeout("skin.cdp_request_timeout", "Codex 皮肤注入超时。"))?
            .map_err(cdp_error)?;
            Ok::<_, AppError>((true, true, compatibility))
        }
    }))
    .await;
    let mut report = InjectionReport::default();
    for result in results {
        match result {
            Ok((verified, injected, compatibility)) => {
                report.verified_pages += usize::from(verified);
                report.injected_pages += usize::from(injected);
                if verified {
                    report.include_compatibility(compatibility);
                }
            }
            Err(_) => report.failed_pages += 1,
        }
    }
    if report.failed_pages > 0 {
        tracing::warn!(
            "皮肤页面处理部分失败，已验证页面数={}，失败页面数={}",
            report.verified_pages,
            report.failed_pages
        );
    }
    Ok(report)
}

/// 执行换皮宿主内部的 `apply_host_compatibility` 步骤。
async fn apply_host_compatibility(page: &Page) -> CompatibilityPageReport {
    let result = tokio::time::timeout(
        CDP_REQUEST_TIMEOUT,
        page.evaluate_expression(HOST_COMPATIBILITY_SCRIPT),
    )
    .await;
    let parsed = match result {
        Ok(Ok(value)) => value.into_value::<CompatibilityPageReport>().ok(),
        Ok(Err(_)) | Err(_) => None,
    };
    match parsed {
        Some(report) if report.version == HOST_COMPATIBILITY_VERSION => report,
        _ => {
            tracing::warn!("Codex 宿主兼容规则执行失败，规则=adapter-runtime");
            CompatibilityPageReport {
                version: HOST_COMPATIBILITY_VERSION.into(),
                applied_rules: Vec::new(),
                skipped_rules: vec!["adapter-runtime".into()],
            }
        }
    }
}

/// 执行换皮宿主内部的 `is_codex_page` 步骤。
async fn is_codex_page(page: &Page) -> Result<bool, AppError> {
    let result = tokio::time::timeout(CDP_REQUEST_TIMEOUT, page.evaluate_expression(PROBE_SCRIPT))
        .await
        .map_err(|_| cdp_timeout("skin.cdp_request_timeout", "Codex 页面校验超时。"))?
        .map_err(cdp_error)?;
    let probe = result
        .into_value::<PageProbe>()
        .map_err(|_| cdp_response_error())?;
    Ok(probe.is_verified_codex())
}

/// 执行换皮宿主内部的 `has_current_skin` 步骤。
async fn has_current_skin(page: &Page, skin: &SkinReference) -> Result<bool, AppError> {
    let expression = current_skin_expression(skin);
    let result = tokio::time::timeout(CDP_REQUEST_TIMEOUT, page.evaluate_expression(expression))
        .await
        .map_err(|_| cdp_timeout("skin.cdp_request_timeout", "Codex 皮肤状态检查超时。"))?
        .map_err(cdp_error)?;
    result
        .into_value::<bool>()
        .map_err(|_| cdp_response_error())
}

/// 执行换皮宿主内部的 `current_skin_expression` 步骤。
fn current_skin_expression(skin: &SkinReference) -> String {
    let version = serde_json::to_string(SKIN_VERSION).unwrap_or_else(|_| "null".into());
    let source = serde_json::to_string(&skin.source).unwrap_or_else(|_| "null".into());
    let id = serde_json::to_string(&skin.id).unwrap_or_else(|_| "null".into());
    format!(
        "(() => {{ const state = window.__CODEX_DREAM_SKIN_STATE__; const skin = state?.skin; return state?.version === {version} && skin?.source === {source} && skin?.id === {id}; }})()"
    )
}

/// 执行换皮宿主内部的 `remove_from_browser` 步骤。
async fn remove_from_browser(browser: &Browser) -> Result<usize, AppError> {
    let pages = browser_pages(browser).await?;
    let results = join_all(pages.into_iter().map(|page| async move {
        if !is_codex_page(&page).await? {
            return Ok(false);
        }
        tokio::time::timeout(CDP_REQUEST_TIMEOUT, page.evaluate_expression(REMOVE_SCRIPT))
            .await
            .map_err(|_| cdp_timeout("skin.cdp_request_timeout", "Codex 皮肤页面清理超时。"))?
            .map_err(cdp_error)?;
        Ok::<_, AppError>(true)
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
    if removed == 0 && failed > 0 {
        Err(AppError::new(
            "skin.cdp_cleanup_failed",
            "未能完成 Codex 皮肤页面清理，请稍后重试。",
        ))
    } else {
        Ok(removed)
    }
}

/// 执行换皮宿主内部的 `remove_from_existing_endpoint` 步骤。
async fn remove_from_existing_endpoint() -> Result<usize, AppError> {
    let Ok((mut browser, handler_task, _)) = connect_existing_browser().await else {
        return Ok(0);
    };
    let cleanup = async {
        fetch_targets(&mut browser).await?;
        remove_from_browser(&browser).await
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

/// 执行换皮宿主内部的 `remove_from_endpoint` 步骤。
async fn remove_from_endpoint(endpoint: CdpEndpoint) -> Result<usize, AppError> {
    let Ok((mut browser, handler_task)) = connect_browser(endpoint).await else {
        return Ok(0);
    };
    let cleanup = async {
        fetch_targets(&mut browser).await?;
        remove_from_browser(&browser).await
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
