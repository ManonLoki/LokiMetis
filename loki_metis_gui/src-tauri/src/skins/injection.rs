/// 执行换皮宿主内部的 `watch_pages` 步骤。
async fn watch_pages(
    host: SkinHostKind,
    mut browser: Browser,
    handler_task: JoinHandle<()>,
    payload: Arc<str>,
    skin: SkinReference,
    initial_transaction: Option<(String, BTreeSet<String>)>,
    mut cancel: watch::Receiver<bool>,
) -> Result<usize, AppError> {
    if let Some((transaction_id, transaction_targets)) = initial_transaction {
        commit_marked_injection_pages(&mut browser, &transaction_id, &transaction_targets).await;
    }
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
                    inject_pages(host, &browser, &payload, &skin, None).await
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
    let removed = tokio::time::timeout(CDP_CLEANUP_TIMEOUT, remove_from_browser(host, &browser))
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
    host: SkinHostKind,
    browser: &Browser,
    payload: &Arc<str>,
    skin: &SkinReference,
    transaction: Option<&InjectionTransaction>,
) -> Result<InjectionReport, AppError> {
    let pages = browser_pages(browser).await?;
    let results = join_all(pages.into_iter().map(|page| {
        let payload = Arc::clone(payload);
        let skin = skin.clone();
        let transaction = transaction.cloned();
        async move {
            if !is_host_page(host, &page).await? {
                return Ok((false, false, CompatibilityPageReport::default()));
            }
            if has_current_skin(&page, &skin).await? {
                let compatibility = apply_host_compatibility(host, &page).await;
                return Ok((true, false, compatibility));
            }
            if let Some(transaction) = transaction.as_ref() {
                transaction.track(page.target_id().as_ref().to_owned());
                mark_injection_transaction(&page, transaction.id()).await?;
            }
            let compatibility = apply_host_compatibility(host, &page).await;
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
    if let Some(transaction) = transaction {
        report.transaction_targets = transaction.tracked_targets();
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

async fn mark_injection_transaction(page: &Page, transaction_id: &str) -> Result<(), AppError> {
    let transaction_id = encode_injection_transaction_id(transaction_id)?;
    let expression = format!(
        "(() => {{ window.__LOKI_METIS_SKIN_TRANSACTION__ = {transaction_id}; return true; }})()"
    );
    tokio::time::timeout(CDP_REQUEST_TIMEOUT, page.evaluate_expression(expression))
        .await
        .map_err(|_| cdp_timeout("skin.cdp_request_timeout", "皮肤注入事务标记超时。"))?
        .map_err(cdp_error)?;
    Ok(())
}

fn encode_injection_transaction_id(transaction_id: &str) -> Result<String, AppError> {
    serde_json::to_string(transaction_id)
        .map_err(|_| AppError::new("skin.assets_invalid", "无法编码皮肤注入事务标记。"))
}

fn rollback_injection_transaction_expression(
    transaction_id: &str,
) -> Result<Arc<str>, AppError> {
    let transaction_id = encode_injection_transaction_id(transaction_id)?;
    Ok(Arc::<str>::from(format!(
        "(() => {{
          if (window.__LOKI_METIS_SKIN_TRANSACTION__ !== {transaction_id}) return null;
          const removed = {REMOVE_SCRIPT};
          if (removed === true) delete window.__LOKI_METIS_SKIN_TRANSACTION__;
          return removed === true;
        }})()"
    )))
}

fn commit_injection_transaction_expression(
    transaction_id: &str,
) -> Result<Arc<str>, AppError> {
    let transaction_id = encode_injection_transaction_id(transaction_id)?;
    Ok(Arc::<str>::from(format!(
        "(() => {{
          if (window.__LOKI_METIS_SKIN_TRANSACTION__ !== {transaction_id}) return false;
          delete window.__LOKI_METIS_SKIN_TRANSACTION__;
          return true;
        }})()"
    )))
}

/// 严格回滚本次安装实际标记过的页面，绝不清理由其它安装产生的皮肤状态。
async fn rollback_marked_injection_pages(
    browser: &mut Browser,
    transaction_id: &str,
    transaction_targets: &BTreeSet<String>,
) -> Result<usize, AppError> {
    if transaction_targets.is_empty() {
        return Ok(0);
    }
    fetch_targets(browser).await?;
    let expression = rollback_injection_transaction_expression(transaction_id)?;
    let pages = browser_pages(browser)
        .await?
        .into_iter()
        .filter(|page| transaction_targets.contains(page.target_id().as_ref()))
        .collect::<Vec<_>>();
    let results = join_all(pages.into_iter().map(|page| {
        let expression = Arc::clone(&expression);
        async move {
            let value = tokio::time::timeout(
                CDP_REQUEST_TIMEOUT,
                page.evaluate_expression(expression.to_string()),
            )
            .await
            .map_err(|_| {
                cdp_timeout(
                    "skin.cdp_request_timeout",
                    "皮肤注入事务回滚超时。",
                )
            })?
            .map_err(cdp_error)?;
            value
                .into_value::<Option<bool>>()
                .map_err(|_| cdp_response_error())
        }
    }))
    .await;
    let mut matched = 0;
    let mut failed = 0;
    for result in results {
        match result {
            Ok(Some(true)) => matched += 1,
            Ok(None) => {}
            Ok(Some(false)) | Err(_) => failed += 1,
        }
    }
    if failed > 0 {
        return Err(AppError::new(
            "skin.injection_rollback_failed",
            "无法确认已清理本次临时皮肤注入，请刷新或重启宿主。",
        ));
    }
    Ok(matched)
}

/// 成功安装后的 marker 仅用于收尾；清理失败不能把已由 watcher 接管的皮肤反转成失败。
async fn commit_marked_injection_pages(
    browser: &mut Browser,
    transaction_id: &str,
    transaction_targets: &BTreeSet<String>,
) {
    if transaction_targets.is_empty() {
        return;
    }
    let expression = match commit_injection_transaction_expression(transaction_id) {
        Ok(expression) => expression,
        Err(_) => {
            tracing::warn!("code=skin.injection_marker_cleanup_failed");
            return;
        }
    };
    if let Err(error) = fetch_targets(browser).await {
        tracing::warn!(
            "皮肤注入事务 marker 清理失败，错误码={}",
            error.code
        );
        return;
    }
    let pages = match browser_pages(browser).await {
        Ok(pages) => pages,
        Err(error) => {
            tracing::warn!(
                "皮肤注入事务 marker 清理失败，错误码={}",
                error.code
            );
            return;
        }
    };
    let results = join_all(
        pages
            .into_iter()
            .filter(|page| transaction_targets.contains(page.target_id().as_ref()))
            .map(|page| {
                let expression = Arc::clone(&expression);
                async move {
                    tokio::time::timeout(
                        CDP_REQUEST_TIMEOUT,
                        page.evaluate_expression(expression.to_string()),
                    )
                    .await
                }
            }),
    )
    .await;
    let failed = results
        .iter()
        .filter(|result| !matches!(result, Ok(Ok(_))))
        .count();
    if failed > 0 {
        tracing::warn!("皮肤注入事务 marker 部分清理失败，失败页面数={failed}");
    }
}

fn rollback_error(original: &AppError, rollback: AppError) -> AppError {
    AppError::with_details(
        "skin.injection_rollback_failed",
        "皮肤应用失败，且无法确认已清理本次临时注入；请刷新或重启宿主后重试。",
        vec![
            format!("original_code={}", original.code),
            format!("rollback_code={}", rollback.code),
        ],
    )
}

/// 执行换皮宿主内部的 `apply_host_compatibility` 步骤。
async fn apply_host_compatibility(
    host: SkinHostKind,
    page: &Page,
) -> CompatibilityPageReport {
    let script = match host {
        SkinHostKind::Codex => HOST_COMPATIBILITY_SCRIPT,
        SkinHostKind::WorkBuddy => WORKBUDDY_HOST_COMPATIBILITY_SCRIPT,
    };
    let result = tokio::time::timeout(
        CDP_REQUEST_TIMEOUT,
        page.evaluate_expression(script),
    )
    .await;
    let parsed = match result {
        Ok(Ok(value)) => value.into_value::<CompatibilityPageReport>().ok(),
        Ok(Err(_)) | Err(_) => None,
    };
    match parsed {
        Some(report) if report.version == HOST_COMPATIBILITY_VERSION => report,
        _ => {
            tracing::warn!(?host, "宿主兼容规则执行失败，规则=adapter-runtime");
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
    is_host_page(SkinHostKind::Codex, page).await
}

/// 按宿主选择独立页面探针，防止跨应用注入。
async fn is_host_page(host: SkinHostKind, page: &Page) -> Result<bool, AppError> {
    let script = match host {
        SkinHostKind::Codex => PROBE_SCRIPT,
        SkinHostKind::WorkBuddy => WORKBUDDY_PROBE_SCRIPT,
    };
    let result = tokio::time::timeout(CDP_REQUEST_TIMEOUT, page.evaluate_expression(script))
        .await
        .map_err(|_| {
            AppError::new(
                "skin.cdp_request_timeout",
                format!("{} 页面校验超时。", host.display_name()),
            )
        })?
        .map_err(cdp_error)?;
    let probe = result
        .into_value::<PageProbe>()
        .map_err(|_| cdp_response_error())?;
    Ok(match host {
        SkinHostKind::Codex => probe.is_verified_codex(),
        SkinHostKind::WorkBuddy => probe.is_verified_workbuddy(),
    })
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
async fn remove_from_browser(host: SkinHostKind, browser: &Browser) -> Result<usize, AppError> {
    let pages = browser_pages(browser).await?;
    let results = join_all(pages.into_iter().map(|page| async move {
        if !is_host_page(host, &page).await? {
            return Ok(false);
        }
        let value = tokio::time::timeout(
            CDP_REQUEST_TIMEOUT,
            page.evaluate_expression(REMOVE_SCRIPT),
        )
            .await
            .map_err(|_| cdp_timeout("skin.cdp_request_timeout", "Codex 皮肤页面清理超时。"))?
            .map_err(cdp_error)?;
        value
            .into_value::<bool>()
            .map_err(|_| cdp_response_error())
            .and_then(|removed| {
                removed.then_some(true).ok_or_else(|| {
                    AppError::new("skin.cdp_cleanup_failed", "宿主皮肤清理脚本未完成。")
                })
            })
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
    let connection = connect_browser(endpoint).await;
    let (mut browser, handler_task) = match connection {
        Ok(connection) => connection,
        Err(_error) if matches!(platform_host_is_running(host).await, Ok(false)) => return Ok(0),
        Err(error) => return Err(error),
    };
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
    handler_task: JoinHandle<()>,
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
