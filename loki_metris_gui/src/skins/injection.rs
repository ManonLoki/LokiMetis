// 本文件收纳：页面持续监视与补注入、逐页注入与事务标记、宿主兼容规则执行及当前皮肤检查。
// （本文件由主文件 include! 并入，与主文件共用同一个模块作用域。）

/// 后台持续监视宿主：先提交首批注入事务，再按固定间隔为新出现的页面补注入，直到收到取消信号。
async fn watch_pages(
    host: SkinHostKind,
    mut browser: Browser,
    mut handler_task: HandlerTaskGuard,
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

/// 并发处理全部宿主页面：已是当前皮肤的只补兼容规则，否则清除旧注入后写入新载荷，并汇总验证、注入、失败数与兼容规则。
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
            if !is_host_page(host, &page)
                .await
                .map_err(|error| (false, error))?
            {
                return Ok((false, false, CompatibilityPageReport::default()));
            }
            ensure_exclusive_skin_owner(&page)
                .await
                .map_err(|error| (true, error))?;
            if has_current_skin(&page, &skin)
                .await
                .map_err(|error| (true, error))?
            {
                let compatibility = apply_host_compatibility(host, &page).await.map_err(|error| (true, error))?;
                return Ok((true, false, compatibility));
            }
            if let Some(transaction) = transaction.as_ref() {
                transaction.track(page.target_id().as_ref().to_owned());
            }
            remove_skin_from_page(&page)
                .await
                .map_err(|error| (true, error))?;
            if let Some(transaction) = transaction.as_ref() {
                mark_injection_transaction(&page, transaction.id())
                    .await
                    .map_err(|error| (true, error))?;
            }
            let compatibility = apply_host_compatibility(host, &page).await.map_err(|error| (true, error))?;
            let evaluated = tokio::time::timeout(
                CDP_REQUEST_TIMEOUT,
                page.evaluate_expression(payload.to_string()),
            )
            .await
            .map_err(|_| {
                (
                    true,
                    cdp_timeout("skin.cdp_request_timeout", "Codex 皮肤注入超时。"),
                )
            })?
            .map_err(|error| (true, cdp_error(error)))?;
            let value = evaluated.into_value::<serde_json::Value>().map_err(|_| (true, cdp_response_error()))?;
            skin_owner_result(value).map_err(|error| (true, error))?;
            if !has_current_skin(&page, &skin)
                .await
                .map_err(|error| (true, error))?
            {
                return Err((true, injection_failed_error()));
            }
            Ok::<_, (bool, AppError)>((true, true, compatibility))
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
            Err((verified, error)) => {
                report.verified_pages += usize::from(verified);
                report.failed_pages += 1;
                report.first_failure.get_or_insert(error);
            }
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

/// 在写入皮肤资源前给页面登记本次事务标记，供失败补偿精确回滚。
async fn mark_injection_transaction(page: &Page, transaction_id: &str) -> Result<(), AppError> {
    let transaction_id = encode_injection_transaction_id(transaction_id)?;
    let expression = guarded_skin_script(&format!(
        "window.__LOKI_METRIS_SKIN_TRANSACTION__ = {transaction_id}; return true;"
    ));
    let result = tokio::time::timeout(CDP_REQUEST_TIMEOUT, page.evaluate_expression(expression))
        .await
        .map_err(|_| cdp_timeout("skin.cdp_request_timeout", "皮肤注入事务标记超时。"))?
        .map_err(cdp_error)?;
    skin_owner_result(result.into_value::<serde_json::Value>().map_err(|_| cdp_response_error())?)?;
    Ok(())
}

/// 将事务 ID 编码为安全的 JavaScript 字符串字面量。
fn encode_injection_transaction_id(transaction_id: &str) -> Result<String, AppError> {
    serde_json::to_string(transaction_id)
        .map_err(|_| AppError::new("skin.assets_invalid", "无法编码皮肤注入事务标记。"))
}

/// 生成只清理当前事务所拥有页面状态的回滚表达式。
fn rollback_injection_transaction_expression(transaction_id: &str) -> Result<Arc<str>, AppError> {
    let transaction_id = encode_injection_transaction_id(transaction_id)?;
    Ok(Arc::<str>::from(format!(
        "(() => {{
          if (window.__LOKI_METRIS_SKIN_TRANSACTION__ !== {transaction_id}) return null;
          const removed = {REMOVE_SCRIPT};
          if (removed === true) delete window.__LOKI_METRIS_SKIN_TRANSACTION__;
          return removed === true;
        }})()"
    )))
}

/// 生成只移除当前事务标记而保留已应用皮肤的提交表达式。
fn commit_injection_transaction_expression(transaction_id: &str) -> Result<Arc<str>, AppError> {
    let transaction_id = encode_injection_transaction_id(transaction_id)?;
    Ok(Arc::<str>::from(format!(
        "(() => {{
          if (window.__LOKI_METRIS_SKIN_TRANSACTION__ !== {transaction_id}) return false;
          delete window.__LOKI_METRIS_SKIN_TRANSACTION__;
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
            .map_err(|_| cdp_timeout("skin.cdp_request_timeout", "皮肤注入事务回滚超时。"))?
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
        tracing::warn!("皮肤注入事务 marker 清理失败，错误码={}", error.code);
        return;
    }
    let pages = match browser_pages(browser).await {
        Ok(pages) => pages,
        Err(error) => {
            tracing::warn!("皮肤注入事务 marker 清理失败，错误码={}", error.code);
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

/// 合并原始失败与补偿失败代码，返回可操作且不泄露载荷的稳定错误。
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

/// 在页面执行宿主兼容规则脚本并读取报告；执行失败或版本不符时记为跳过 `adapter-runtime` 规则。
async fn apply_host_compatibility(host: SkinHostKind, page: &Page) -> Result<CompatibilityPageReport, AppError> {
    let script = match host {
        SkinHostKind::Codex => HOST_COMPATIBILITY_SCRIPT,
        SkinHostKind::WorkBuddy => WORKBUDDY_HOST_COMPATIBILITY_SCRIPT,
    };
    let expression = guarded_skin_script(&format!("return {script}"));
    let result = tokio::time::timeout(CDP_REQUEST_TIMEOUT, page.evaluate_expression(expression)).await;
    let parsed: Option<CompatibilityPageReport> = match result {
        Ok(Ok(value)) => {
            match value.into_value::<serde_json::Value>().ok() {
                Some(value) => skin_owner_result(value)?.and_then(|value| serde_json::from_value(value).ok()),
                None => None,
            }
        }
        Ok(Err(_)) | Err(_) => None,
    };
    match parsed {
        Some(report) if report.version == HOST_COMPATIBILITY_VERSION => Ok(report),
        _ => {
            tracing::warn!(?host, "宿主兼容规则执行失败，规则=adapter-runtime");
            Ok(CompatibilityPageReport {
                version: HOST_COMPATIBILITY_VERSION.into(),
                applied_rules: Vec::new(),
                skipped_rules: vec!["adapter-runtime".into()],
            })
        }
    }
}

/// 判断页面是否为可信的 Codex 主页面。
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

/// 在请求期限内检查页面是否已完整生效指定皮肤。
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

/// 生成检查页面皮肤状态的脚本：要求运行态版本与皮肤身份一致、未被停用或静默、清理函数与样式节点齐全，且没有其它换肤工具。
fn current_skin_expression(skin: &SkinReference) -> String {
    let version = serde_json::to_string(SKIN_VERSION).unwrap_or_else(|_| "null".into());
    let source = serde_json::to_string(&skin.source).unwrap_or_else(|_| "null".into());
    let id = serde_json::to_string(&skin.id).unwrap_or_else(|_| "null".into());
    format!(
        "(() => {{
          if (window.__CODEX_DREAM_SKIN_STATE__ || window.__BIFANG_CODEX_SKIN_COMPAT__) return false;
          const state = window.__LOKI_METRIS_DREAM_SKIN_STATE__;
          const skin = state?.skin;
          const root = document.documentElement;
          const style = document.getElementById('loki-metris-dream-skin-style');
          return window.__LOKI_METRIS_DREAM_SKIN_DISABLED__ !== true &&
            state?.quiesced !== true &&
            state?.version === {version} && skin?.source === {source} && skin?.id === {id} &&
            typeof state?.cleanup === 'function' &&
            root?.classList.contains('codex-dream-skin') === true &&
            style?.tagName === 'STYLE' && style.isConnected === true &&
            Boolean(style.textContent?.trim()) && Boolean(style.sheet?.cssRules.length);
        }})()"
    )
}

/// 运行状态或样式未生效时拒绝把 CDP 求值成功解释为皮肤安装成功。
fn injection_failed_error() -> AppError {
    AppError::new(
        "skin.injection_failed",
        "未能确认全部宿主页面的皮肤已生效，请刷新后重试。",
    )
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - injection_cleanup.rs：页面皮肤清理、清理结果汇总、经已有或指定端点清理，以及 CDP 错误与 WebSocket 地址校验。
include!("injection_cleanup.rs");
