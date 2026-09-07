/// 执行换皮宿主内部的 `resolve_codex_instance` 步骤。
async fn resolve_codex_instance(id: &str) -> Result<ResolvedCodexInstance, AppError> {
    platform_codex_processes()
        .await?
        .into_iter()
        .map(resolved_instance)
        .find(|instance| instance.id == id)
        .ok_or_else(|| {
            AppError::new(
                "skin.codex_instance_changed",
                "所选 Codex 实例已退出或身份发生变化，请重新选择。",
            )
        })
}

/// 执行换皮宿主内部的 `discover_codex_process_instances` 步骤。
async fn discover_codex_process_instances() -> Result<Vec<CodexInstance>, AppError> {
    let mut instances = platform_codex_processes()
        .await?
        .into_iter()
        .map(resolved_instance)
        .map(scanned_codex_instance)
        .collect::<Vec<_>>();
    instances.sort_by_key(|instance| std::cmp::Reverse(instance.pid));
    Ok(instances)
}

/// 执行换皮宿主内部的 `scanned_codex_instance` 步骤。
fn scanned_codex_instance(resolved: ResolvedCodexInstance) -> CodexInstance {
    let state = if resolved.debug_port.is_some() {
        CodexRuntimeState::Ready
    } else {
        CodexRuntimeState::RunningWithoutCdp
    };
    codex_instance_from_resolved(resolved, state, None)
}

/// 执行换皮宿主内部的 `probe_resolved_account_profile` 步骤。
async fn probe_resolved_account_profile(
    resolved: &ResolvedCodexInstance,
) -> Result<Option<AccountProfile>, AppError> {
    let port = resolved.debug_port.ok_or_else(|| {
        AppError::new(
            "skin.cdp_unavailable",
            "所选 Codex 实例没有可用的本机调试端口。",
        )
    })?;
    let (mut browser, task) = connect_browser(CdpEndpoint::new(port)).await?;
    let result = async {
        fetch_targets(&mut browser).await?;
        discover_account_profile_until_ready(&browser).await
    }
    .await;
    task.abort();
    drop(browser);
    result
}

/// 执行换皮宿主内部的 `codex_instance_from_resolved` 步骤。
fn codex_instance_from_resolved(
    resolved: ResolvedCodexInstance,
    state: CodexRuntimeState,
    account_profile: Option<AccountProfile>,
) -> CodexInstance {
    let label = resolved
        .profile
        .as_ref()
        .map(|profile| format!("Codex · {profile}"))
        .unwrap_or_else(|| format!("Codex 进程 {}", resolved.process.pid));
    CodexInstance {
        id: resolved.id,
        pid: resolved.process.pid,
        label,
        profile: resolved.profile,
        state,
        debug_port: resolved.debug_port,
        active_skin_name: None,
        active_skin: None,
        account_label: account_profile
            .as_ref()
            .and_then(|profile| profile.label.clone()),
        avatar_data_url: account_profile.and_then(|profile| profile.avatar_data_url),
    }
}

/// 执行换皮宿主内部的 `discover_account_profile_until_ready` 步骤。
async fn discover_account_profile_until_ready(
    browser: &Browser,
) -> Result<Option<AccountProfile>, AppError> {
    let deadline = Instant::now() + ACCOUNT_PROFILE_READY_TIMEOUT;
    let mut fallback = None;
    loop {
        let current_error = match discover_account_profile(browser).await {
            Ok(Some(profile)) => {
                if profile.avatar_data_url.is_some() {
                    return Ok(Some(profile));
                }
                fallback = Some(profile);
                None
            }
            Ok(None) => None,
            Err(error) => Some(error),
        };
        if Instant::now() >= deadline {
            return match (fallback, current_error) {
                (Some(profile), _) => Ok(Some(profile)),
                (None, Some(error)) => Err(error),
                (None, None) => Ok(None),
            };
        }
        tokio::time::sleep(ACCOUNT_PROFILE_RETRY_INTERVAL).await;
    }
}

/// 执行换皮宿主内部的 `discover_account_profile` 步骤。
async fn discover_account_profile(browser: &Browser) -> Result<Option<AccountProfile>, AppError> {
    let pages = browser_pages(browser).await?;
    let mut labels = BTreeSet::new();
    let mut avatars = BTreeSet::new();
    let mut active_skins = Vec::new();
    let mut probe_succeeded = false;
    let mut probe_failed = false;
    for page in pages {
        match is_codex_page(&page).await {
            Ok(true) => {}
            Ok(false) => continue,
            Err(_) => {
                probe_failed = true;
                continue;
            }
        }
        let result = match tokio::time::timeout(
            CDP_REQUEST_TIMEOUT,
            page.evaluate_expression(ACCOUNT_PROFILE_PROBE_SCRIPT),
        )
        .await
        {
            Ok(Ok(result)) => result,
            _ => {
                probe_failed = true;
                continue;
            }
        };
        if let Ok(profile) = result.into_value::<AccountProfileProbe>() {
            probe_succeeded = true;
            let normalized = profile
                .label
                .unwrap_or_default()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let label = (!normalized.is_empty()
                && normalized.chars().count() <= 80
                && !normalized.chars().any(char::is_control))
            .then_some(normalized);
            if let Some(label) = label.as_ref() {
                labels.insert(label.clone());
            }
            if let Some(avatar) = validated_account_avatar(profile.avatar_data_url.as_deref()) {
                avatars.insert(avatar);
            }
            if let Some(active_skin) = recovered_skin_identity(profile.active_skin) {
                if !active_skins.contains(&active_skin) {
                    active_skins.push(active_skin);
                }
            }
        } else {
            probe_failed = true;
        }
    }
    if !probe_succeeded && probe_failed {
        return Err(AppError::new(
            "skin.account_profile_probe_failed",
            "无法读取 Codex 账户资料。",
        ));
    }
    let label = (labels.len() == 1)
        .then(|| labels.into_iter().next())
        .flatten();
    let avatar_data_url = (avatars.len() == 1)
        .then(|| avatars.into_iter().next())
        .flatten();
    let active_skin = (active_skins.len() == 1).then(|| active_skins.remove(0));
    if label.is_none() && avatar_data_url.is_none() && active_skin.is_none() {
        return Ok(None);
    }
    Ok(Some(AccountProfile {
        label,
        avatar_data_url,
        active_skin,
    }))
}

/// 执行换皮宿主内部的 `recovered_skin_identity` 步骤。
fn recovered_skin_identity(probe: Option<ActiveSkinProbe>) -> Option<RecoveredSkinIdentity> {
    let probe = probe?;
    let _version = probe.version.filter(|value| {
        !value.trim().is_empty()
            && value.chars().count() <= 80
            && !value.chars().any(char::is_control)
    })?;
    if let (Some(source), Some(id)) = (probe.source, probe.id) {
        if is_valid_skin_id(&id) {
            return Some(RecoveredSkinIdentity::Exact(SkinReference { source, id }));
        }
    }
    probe
        .legacy_theme_id
        .filter(|id| is_valid_skin_id(id))
        .map(RecoveredSkinIdentity::LegacyId)
        .or_else(|| {
            probe
                .legacy_style_text
                .filter(|style| {
                    !style.is_empty() && style.chars().count() <= MAX_RUNTIME_STYLE_PROBE_CHARS
                })
                .map(RecoveredSkinIdentity::LegacyThemeCss)
        })
}

/// 执行换皮宿主内部的 `validated_account_avatar` 步骤。
fn validated_account_avatar(value: Option<&str>) -> Option<String> {
    let value = value?;
    let (header, payload) = value.split_once(',')?;
    if !matches!(
        header,
        "data:image/png;base64" | "data:image/jpeg;base64" | "data:image/webp;base64"
    ) || payload.is_empty()
        || payload.chars().any(char::is_whitespace)
    {
        return None;
    }
    let decoded = STANDARD.decode(payload).ok()?;
    (!decoded.is_empty() && decoded.len() <= MAX_ACCOUNT_AVATAR_BYTES).then(|| value.to_owned())
}

/// 执行换皮宿主内部的 `displayed_active_skin_name` 步骤。
fn displayed_active_skin_name(active: Option<&SkinDescriptor>) -> Option<String> {
    active.map(|skin| skin.name.clone())
}

/// 执行换皮宿主内部的 `available_debug_port` 步骤。
fn available_debug_port(instances: &[ResolvedCodexInstance]) -> Result<u16, AppError> {
    let used = instances
        .iter()
        .filter_map(|instance| instance.debug_port)
        .collect::<HashSet<_>>();
    (DEFAULT_CDP_PORT..=DEFAULT_CDP_PORT + 99)
        .find(|port| {
            !used.contains(port) && std::net::TcpListener::bind(("127.0.0.1", *port)).is_ok()
        })
        .ok_or_else(|| {
            AppError::new(
                "skin.cdp_unavailable",
                "没有可用于重启所选 Codex 实例的本机调试端口。",
            )
        })
}

/// 执行换皮宿主内部的 `endpoint_candidates_from_commands` 步骤。
fn endpoint_candidates_from_commands(mut commands: Vec<(u32, String)>) -> Vec<CdpEndpoint> {
    commands.sort_by_key(|command| std::cmp::Reverse(command.0));
    let mut ports = HashSet::new();
    let mut endpoints = commands
        .into_iter()
        .filter_map(|(_, command)| debug_port_from_command_line(&command))
        .filter(|port| ports.insert(*port))
        .map(CdpEndpoint::new)
        .collect::<Vec<_>>();
    if ports.insert(DEFAULT_CDP_PORT) {
        endpoints.push(CdpEndpoint::default());
    }
    endpoints
}

/// 执行换皮宿主内部的 `cdp_endpoint_candidates` 步骤。
async fn cdp_endpoint_candidates() -> Vec<CdpEndpoint> {
    endpoint_candidates_from_commands(platform_codex_command_lines().await.unwrap_or_default())
}

/// 执行换皮宿主内部的 `operation_cancelled` 步骤。
fn operation_cancelled() -> AppError {
    AppError::new("skin.operation_cancelled", "已停止 Codex 启动或页面搜寻。")
}

/// 执行换皮宿主内部的 `codex_exited` 步骤。
fn codex_exited() -> AppError {
    AppError::new("skin.codex_exited", "Codex 已在启动或页面搜寻期间退出。")
}

/// 执行换皮宿主内部的 `ensure_codex_running` 步骤。
fn ensure_codex_running(running: bool) -> Result<(), AppError> {
    running.then_some(()).ok_or_else(codex_exited)
}

/// 执行换皮宿主内部的 `monitor_codex_operation` 步骤。
async fn monitor_codex_operation<T, F>(
    cancel: &mut watch::Receiver<bool>,
    future: F,
) -> Result<T, AppError>
where
    F: Future<Output = Result<T, AppError>>,
{
    if *cancel.borrow() {
        return Err(operation_cancelled());
    }
    let mut process_check = tokio::time::interval_at(
        tokio::time::Instant::now() + CODEX_PROCESS_POLL_INTERVAL,
        CODEX_PROCESS_POLL_INTERVAL,
    );
    process_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    tokio::pin!(future);
    loop {
        tokio::select! {
            biased;
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    return Err(operation_cancelled());
                }
            }
            result = &mut future => return result,
            _ = process_check.tick() => {
                let running = run_cancellable(cancel, async {
                    tokio::time::timeout(
                        Duration::from_secs(1),
                        platform_codex_is_running(),
                    )
                    .await
                    .map_err(|_| AppError::new(
                        "skin.codex_process_inspection_failed",
                        "检查 Codex 运行状态超时。",
                    ))?
                }).await?;
                ensure_codex_running(running)?;
            }
        }
    }
}

/// 执行换皮宿主内部的 `run_cancellable` 步骤。
async fn run_cancellable<T, F>(cancel: &mut watch::Receiver<bool>, future: F) -> Result<T, AppError>
where
    F: Future<Output = Result<T, AppError>>,
{
    if *cancel.borrow() {
        return Err(operation_cancelled());
    }
    tokio::pin!(future);
    tokio::select! {
        biased;
        changed = cancel.changed() => {
            if changed.is_err() || *cancel.borrow() {
                Err(operation_cancelled())
            } else {
                future.await
            }
        }
        result = &mut future => result,
    }
}

/// 执行换皮宿主内部的 `cancellable_sleep` 步骤。
async fn cancellable_sleep(
    cancel: &mut watch::Receiver<bool>,
    duration: Duration,
) -> Result<(), AppError> {
    if *cancel.borrow() {
        return Err(operation_cancelled());
    }
    tokio::select! {
        biased;
        changed = cancel.changed() => {
            if changed.is_err() || *cancel.borrow() {
                Err(operation_cancelled())
            } else {
                Ok(())
            }
        }
        _ = tokio::time::sleep(duration) => Ok(()),
    }
}
