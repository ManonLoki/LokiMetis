// 本文件收纳：安装前外观预检与逐项比较，以及首批注入的就绪等待与会话重连判定。
// （本文件由主文件 include! 并入，与主文件共用同一个模块作用域。）

/// 对照皮肤外观策略检查宿主当前外观：明暗模式不受支持时记一项差异，再按当前模式比较逐项要求。
fn build_appearance_check(
    policy: &AppearancePolicy,
    probe: &AppearanceProbe,
) -> SkinAppearanceCheck {
    let mut check = SkinAppearanceCheck {
        effective_mode: probe.effective_mode,
        supported_color_modes: policy.supported_color_modes.clone(),
        differences: Vec::new(),
        unreadable: Vec::new(),
    };
    if !policy.supported_color_modes.contains(&probe.effective_mode) {
        check.differences.push(AppearanceDifference {
            field: "colorMode".into(),
            label: "颜色模式".into(),
            current_value: Some(probe.effective_mode.as_str().into()),
            expected_value: policy
                .supported_color_modes
                .iter()
                .map(|mode| mode.as_str())
                .collect::<Vec<_>>()
                .join(" / "),
        });
    }
    let requirement =
        policy
            .requirements
            .as_ref()
            .and_then(|requirements| match probe.effective_mode {
                ColorMode::Light => requirements.light.as_ref(),
                ColorMode::Dark => requirements.dark.as_ref(),
            });
    if let Some(requirement) = requirement {
        compare_appearance_requirement(&mut check, requirement, probe);
    }
    check
}

/// 逐项比较当前模式下宿主主题、配色、字体等设置与皮肤要求。
fn compare_appearance_requirement(
    check: &mut SkinAppearanceCheck,
    requirement: &AppearanceRequirement,
    probe: &AppearanceProbe,
) {
    let mode = check.effective_mode.as_str();
    let theme = probe
        .appearance
        .get("themes")
        .and_then(|themes| themes.get(mode));
    let chrome = theme.and_then(|theme| theme.get("chromeTheme"));
    compare_optional_value(
        check,
        "codeThemeId",
        "代码主题",
        requirement.code_theme_id.as_deref(),
        theme.and_then(|value| value.get("codeThemeId")),
        probe.appearance_readable,
    );
    for (field, label, expected, private_key, effective_key) in [
        (
            "accent",
            "强调色",
            requirement.accent.as_deref(),
            "accent",
            "accent",
        ),
        (
            "surface",
            "背景色",
            requirement.surface.as_deref(),
            "surface",
            "surface",
        ),
        ("ink", "文字色", requirement.ink.as_deref(), "ink", "ink"),
    ] {
        let current = chrome
            .and_then(|value| value.get(private_key))
            .or_else(|| probe.effective.get(effective_key));
        compare_optional_value(check, field, label, expected, current, true);
    }
    let contrast = requirement.contrast.map(|value| value.to_string());
    compare_optional_value(
        check,
        "contrast",
        "对比度",
        contrast.as_deref(),
        chrome
            .and_then(|value| value.get("contrast"))
            .or_else(|| probe.effective.get("contrast")),
        true,
    );
    let opaque = requirement.opaque_windows.map(|value| value.to_string());
    compare_optional_value(
        check,
        "opaqueWindows",
        "透明窗口",
        opaque.as_deref(),
        chrome.and_then(|value| value.get("opaqueWindows")),
        probe.appearance_readable,
    );
    compare_optional_value(
        check,
        "uiFont",
        "界面字体",
        requirement.ui_font.as_deref(),
        chrome
            .and_then(|value| value.get("fonts"))
            .and_then(|value| value.get("ui")),
        probe.appearance_readable,
    );
    compare_optional_value(
        check,
        "codeFont",
        "代码字体",
        requirement.code_font.as_deref(),
        chrome
            .and_then(|value| value.get("fonts"))
            .and_then(|value| value.get("code")),
        probe.appearance_readable,
    );
    if let Some(semantic) = &requirement.semantic_colors {
        for (field, label, expected, key) in [
            (
                "semanticColors.diffAdded",
                "新增语义色",
                semantic.diff_added.as_deref(),
                "diffAdded",
            ),
            (
                "semanticColors.diffRemoved",
                "删除语义色",
                semantic.diff_removed.as_deref(),
                "diffRemoved",
            ),
            (
                "semanticColors.skill",
                "Skill 语义色",
                semantic.skill.as_deref(),
                "skill",
            ),
        ] {
            compare_optional_value(
                check,
                field,
                label,
                expected,
                chrome
                    .and_then(|value| value.get("semanticColors"))
                    .and_then(|value| value.get(key)),
                probe.appearance_readable,
            );
        }
    }
}

/// 比较单项设置：皮肤未要求时跳过，读不到当前值记为无法读取，规范化后不一致记为差异。
fn compare_optional_value(
    check: &mut SkinAppearanceCheck,
    field: &str,
    label: &str,
    expected: Option<&str>,
    current: Option<&serde_json::Value>,
    _source_readable: bool,
) {
    let Some(expected) = expected else {
        return;
    };
    let current = current
        .and_then(json_display_value)
        .filter(|value| !value.trim().is_empty());
    let difference = AppearanceDifference {
        field: field.into(),
        label: label.into(),
        current_value: current.clone(),
        expected_value: expected.into(),
    };
    match current {
        Some(current)
            if normalize_appearance_value(&current) != normalize_appearance_value(expected) =>
        {
            check.differences.push(difference);
        }
        None => check.unreadable.push(difference),
        _ => {}
    }
}

/// 把 JSON 标量转换为可展示文本，对象与数组返回空。
fn json_display_value(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

/// 外观设置比较前去除首尾空白并转为小写。
fn normalize_appearance_value(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

/// 在页面就绪期限内反复刷新目标并注入首批页面，直到至少一个页面验证成功；会话异常时重新连接同一宿主根进程。
async fn wait_for_initial_injection_inner(
    host: SkinHostKind,
    browser: &mut Browser,
    handler_task: &mut HandlerTaskGuard,
    payload: &Arc<str>,
    skin: &SkinReference,
    page_ready_timeout: Duration,
    endpoint: CdpEndpoint,
    expected_host_root_pid: u32,
    transaction: &InjectionTransaction,
) -> Result<InjectionReport, AppError> {
    let deadline = tokio::time::Instant::now() + page_ready_timeout;
    let mut last_session_error = None;

    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(last_session_error.unwrap_or_else(|| host_page_not_found(host)));
        }

        let scan = async {
            fetch_targets(browser).await?;
            inject_pages(host, browser, payload, skin, Some(transaction)).await
        }
        .await;

        let reconnect = match scan {
            Ok(mut report) if report.verified_pages > 0 => {
                validate_initial_injection_report(&mut report)?;
                return Ok(report);
            }
            Ok(report) if should_reconnect_initial_session(&report) => {
                last_session_error = Some(AppError::new(
                    "skin.cdp_failed",
                    "与 Codex 调试会话通信失败，请稍后重试。",
                ));
                true
            }
            Ok(_) => {
                last_session_error = None;
                false
            }
            Err(error) => {
                tracing::warn!("首次皮肤页面扫描失败，重建调试会话，错误码={}", error.code);
                last_session_error = Some(error);
                true
            }
        };

        if !reconnect {
            tokio::time::sleep(CODEX_PAGE_POLL_INTERVAL).await;
            continue;
        }

        tracing::warn!("首次皮肤页面扫描未能验证主页面，重建调试会话");
        loop {
            if tokio::time::Instant::now() >= deadline {
                return Err(last_session_error.unwrap_or_else(|| host_page_not_found(host)));
            }
            match connect_browser(endpoint).await {
                Ok((mut next_browser, next_handler_task)) => {
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
                    match reconnect_validation_decision(host, verification) {
                        ReconnectValidationDecision::Accept => {
                            if next_handler_task.is_finished() {
                                last_session_error = Some(AppError::new(
                                    "skin.cdp_failed",
                                    "宿主调试会话已意外结束。",
                                ));
                                continue;
                            }
                            handler_task.replace(next_handler_task);
                            *browser = next_browser;
                            break;
                        }
                        ReconnectValidationDecision::Retry(error) => {
                            next_handler_task.abort();
                            drop(next_browser);
                            last_session_error = Some(error);
                            tokio::time::sleep(CODEX_PAGE_POLL_INTERVAL).await;
                        }
                        ReconnectValidationDecision::Reject(error) => {
                            next_handler_task.abort();
                            drop(next_browser);
                            return Err(error);
                        }
                    }
                }
                Err(error) => {
                    last_session_error = Some(error);
                    tokio::time::sleep(CODEX_PAGE_POLL_INTERVAL).await;
                }
            }
        }
    }
}

/// 表示重连候选完成宿主页与进程归属复核后的处置方式。
enum ReconnectValidationDecision {
    /// 候选已通过全部校验，可以替换当前会话。
    Accept,
    /// 候选未通过临时校验，关闭后可在截止时间内重试。
    Retry(AppError),
    /// 无法可信判断端点归属，必须立即拒绝而不能降级重试。
    Reject(AppError),
}

/// 仅接受完整通过宿主校验的重连候选，并将归属检查故障设为失败关闭。
fn reconnect_validation_decision(
    host: SkinHostKind,
    verification: Result<bool, AppError>,
) -> ReconnectValidationDecision {
    match verification {
        Ok(true) => ReconnectValidationDecision::Accept,
        Ok(false) => ReconnectValidationDecision::Retry(AppError::new(
            "skin.cdp_rejected",
            format!("调试端点不是 {} 的唯一可信主页面。", host.display_name()),
        )),
        Err(error) if is_host_inspection_error(error.code) => {
            ReconnectValidationDecision::Reject(error)
        }
        Err(error) => ReconnectValidationDecision::Retry(error),
    }
}

/// 没有页面验证成功且有页面失败时，判定当前调试会话失效需要重连。
fn should_reconnect_initial_session(report: &InjectionReport) -> bool {
    report.verified_pages == 0 && report.failed_pages > 0
}

/// 已验证的页面必须全部应用成功，部分失败交由外层安装事务回滚。
fn validate_initial_injection_report(report: &mut InjectionReport) -> Result<(), AppError> {
    if report.failed_pages > 0 {
        Err(report.first_failure.take().unwrap_or_else(injection_failed_error))
    } else {
        Ok(())
    }
}

/// 返回与宿主一致的页面就绪失败，避免 WorkBuddy 错误沿用 Codex 文案。
fn host_page_not_found(host: SkinHostKind) -> AppError {
    match host {
        SkinHostKind::Codex => AppError::new(
            "skin.codex_page_not_found",
            "Codex 已启动，但主页面仍未准备就绪，请稍后重试；若持续失败再重启 Codex。",
        ),
        SkinHostKind::WorkBuddy => AppError::new(
            "skin.workbuddy_page_not_found",
            "WorkBuddy 已启动，但可验证主页面仍未准备就绪。",
        ),
    }
}
