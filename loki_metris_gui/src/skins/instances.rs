// 本文件收纳：宿主实例解析与端点归属、账户资料探测、生效皮肤身份恢复与实例展示转换。
// （本文件由主文件 include! 并入，与主文件共用同一个模块作用域。）

/// 按实例 ID 重新解析 Codex 实例；实例已退出或身份变化时报错，防止操作错误进程。
#[cfg(target_os = "macos")]
async fn resolve_skin_host_instance(id: &str) -> Result<ResolvedSkinHostInstance, AppError> {
    resolved_host_instances(SkinHostKind::Codex)
        .await?
        .into_iter()
        .find(|instance| instance.id == id)
        .ok_or_else(|| {
            AppError::new(
                "skin.codex_instance_changed",
                "所选 Codex 实例已退出或身份发生变化，请重新选择。",
            )
        })
}

/// 解析指定宿主的实例，并只把经过页面与 owner 验证的端点关联到对应进程树根。
async fn resolved_host_instances(
    host: SkinHostKind,
) -> Result<Vec<ResolvedSkinHostInstance>, AppError> {
    resolved_host_instances_with_preferred(host, None).await
}

/// 使用服务刚验证过的端点作为首选候选，避免动态端口因命令行读取降级而短暂丢失。
async fn resolved_host_instances_with_preferred(
    host: SkinHostKind,
    preferred_endpoint: Option<CdpEndpoint>,
) -> Result<Vec<ResolvedSkinHostInstance>, AppError> {
    let mut instances = platform_host_processes(host)
        .await?
        .into_iter()
        .map(|process| resolved_instance_for_host(host, process))
        .collect::<Vec<_>>();
    let endpoint_candidates = cdp_endpoint_candidates_for(host, preferred_endpoint).await?;
    for instance in &mut instances {
        instance.debug_port = None;
        let root_pid = instance.process.pid;
        for endpoint in endpoint_candidates.iter().copied() {
            if endpoint_matches_host(host, endpoint, Some(root_pid)).await? {
                instance.debug_port = Some(endpoint.port);
                break;
            }
        }
    }
    Ok(instances)
}

/// 验证端点确属目标宿主，并绑定到预期的可信根进程树。
async fn endpoint_matches_host(
    host: SkinHostKind,
    endpoint: CdpEndpoint,
    expected_root_pid: Option<u32>,
) -> Result<bool, AppError> {
    let Ok((mut browser, task)) = connect_browser(endpoint).await else {
        return Ok(false);
    };
    let (mut task, matches) = hold_connected_handler_during(
        task,
        browser_matches_host_for_root(host, &mut browser, endpoint, expected_root_pid),
    )
    .await;
    task.abort();
    drop(browser);
    matches
}

/// 为运行态实例加宿主命名空间，防止不同宿主的相同实例 ID 冲突。
fn runtime_instance_key(host: SkinHostKind, instance_id: &str) -> String {
    let prefix = match host {
        SkinHostKind::Codex => "codex",
        SkinHostKind::WorkBuddy => "workBuddy",
    };
    format!("{prefix}:{instance_id}")
}

/// 判断运行态键是否属于指定宿主，避免 PID 变化后遗留的旧实例状态跨宿主泄漏。
fn runtime_instance_belongs_to_host(host: SkinHostKind, key: &str) -> bool {
    key.starts_with(match host {
        SkinHostKind::Codex => "codex:",
        SkinHostKind::WorkBuddy => "workBuddy:",
    })
}

/// 从宿主命名空间键中恢复原始实例 ID，并兼容旧的无前缀键。
fn runtime_instance_id(host: SkinHostKind, key: &str) -> &str {
    key.strip_prefix(match host {
        SkinHostKind::Codex => "codex:",
        SkinHostKind::WorkBuddy => "workBuddy:",
    })
    .unwrap_or(key)
}

/// 重新发现并按稳定 ID 解析目标宿主实例，拒绝已退出或身份变化的实例。
#[cfg(target_os = "macos")]
async fn resolve_host_instance(
    host: SkinHostKind,
    id: &str,
) -> Result<ResolvedSkinHostInstance, AppError> {
    resolve_host_instance_with_preferred(host, id, None).await
}

/// 解析实例时优先复核服务在同一生命周期内保存的已验证端点。
async fn resolve_host_instance_with_preferred(
    host: SkinHostKind,
    id: &str,
    preferred_endpoint: Option<CdpEndpoint>,
) -> Result<ResolvedSkinHostInstance, AppError> {
    resolved_host_instances_with_preferred(host, preferred_endpoint)
        .await?
        .into_iter()
        .find(|instance| instance.id == id)
        .ok_or_else(|| {
            AppError::new(
                "skin.host_instance_changed",
                format!(
                    "所选 {} 实例已退出或身份发生变化，请重新选择。",
                    host.display_name()
                ),
            )
        })
}

/// 扫描宿主主进程并生成按 PID 从新到旧排列的界面实例快照。
async fn discover_host_process_instances(
    host: SkinHostKind,
) -> Result<Vec<SkinHostInstance>, AppError> {
    let mut instances = resolved_host_instances(host)
        .await?
        .into_iter()
        .map(|resolved| scanned_host_instance(host, resolved))
        .collect::<Vec<_>>();
    instances.sort_by_key(|instance| std::cmp::Reverse(instance.pid));
    Ok(instances)
}

/// 把已解析宿主进程映射为扫描快照，并由调试端口决定运行状态。
fn scanned_host_instance(host: SkinHostKind, resolved: ResolvedSkinHostInstance) -> SkinHostInstance {
    let state = if resolved.debug_port.is_some() {
        CodexRuntimeState::Ready
    } else {
        CodexRuntimeState::RunningWithoutCdp
    };
    host_instance_from_resolved(host, resolved, state, None)
}

/// 把解析结果转换为界面实例，有调试端口即视为可换肤。
#[cfg(test)]
fn scanned_skin_host_instance(resolved: ResolvedSkinHostInstance) -> SkinHostInstance {
    let state = if resolved.debug_port.is_some() {
        CodexRuntimeState::Ready
    } else {
        CodexRuntimeState::RunningWithoutCdp
    };
    codex_instance_from_resolved(resolved, state, None)
}

/// 连接实例的调试端口，确认浏览器属于该进程后在就绪期限内读取账户资料。
async fn probe_resolved_account_profile(
    resolved: &ResolvedSkinHostInstance,
) -> Result<Option<AccountProfile>, AppError> {
    let port = resolved.debug_port.ok_or_else(|| {
        AppError::new(
            "skin.cdp_unavailable",
            "所选 Codex 实例没有可用的本机调试端口。",
        )
    })?;
    let (mut browser, task) = connect_browser(CdpEndpoint::new(port)).await?;
    let (mut task, result) = hold_connected_handler_during(task, async {
        if !browser_matches_host_for_root(
            SkinHostKind::Codex,
            &mut browser,
            CdpEndpoint::new(port),
            Some(resolved.process.pid),
        )
        .await?
        {
            return Err(AppError::new(
                "skin.cdp_rejected",
                "Codex 调试端点不再属于所选实例。",
            ));
        }
        discover_account_profile_until_ready(&browser).await
    })
    .await;
    task.abort();
    drop(browser);
    result
}

/// 两个宿主独立读取当前皮肤标记，不让账户缓存承担动态运行状态。
async fn probe_resolved_active_skin(
    host: SkinHostKind,
    resolved: &ResolvedSkinHostInstance,
) -> Result<Option<RecoveredSkinIdentity>, AppError> {
    let port = resolved.debug_port.ok_or_else(|| {
        AppError::new(
            "skin.cdp_unavailable",
            format!("所选 {} 实例没有可用的本机调试端口。", host.display_name()),
        )
    })?;
    let (mut browser, task) = connect_browser(CdpEndpoint::new(port)).await?;
    let (mut task, result) = hold_connected_handler_during(task, async {
        if !browser_matches_host_for_root(
            host,
            &mut browser,
            CdpEndpoint::new(port),
            Some(resolved.process.pid),
        )
        .await?
        {
            return Err(AppError::new(
                "skin.cdp_rejected",
                format!("{} 调试端点不再属于所选实例。", host.display_name()),
            ));
        }
        for page in browser_pages(&browser).await? {
            if !is_host_page(host, &page).await? {
                continue;
            }
            let value = tokio::time::timeout(
                CDP_REQUEST_TIMEOUT,
                page.evaluate_expression(ACTIVE_SKIN_PROBE_SCRIPT),
            )
            .await
            .map_err(|_| AppError::new("skin.cdp_request_timeout", "皮肤状态探测超时。"))?
            .map_err(cdp_error)?;
            let probe = value
                .into_value::<ActiveSkinOnlyProbe>()
                .map_err(|_| cdp_response_error())?;
            return Ok(recovered_skin_identity(probe.active_skin));
        }
        Ok(None)
    })
    .await;
    task.abort();
    drop(browser);
    result
}

/// 由解析结果生成界面实例：标签为“Codex · 配置档”，没有配置档时为“Codex 进程 PID”，并附上账户资料。
fn codex_instance_from_resolved(
    resolved: ResolvedSkinHostInstance,
    state: CodexRuntimeState,
    account_profile: Option<AccountProfile>,
) -> SkinHostInstance {
    let label = resolved
        .profile
        .as_ref()
        .map(|profile| format!("Codex · {profile}"))
        .unwrap_or_else(|| format!("Codex 进程 {}", resolved.process.pid));
    SkinHostInstance {
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

/// 将已验证进程映射为宿主实例，同时保留 Codex 专属账户信息边界。
fn host_instance_from_resolved(
    host: SkinHostKind,
    resolved: ResolvedSkinHostInstance,
    state: CodexRuntimeState,
    account_profile: Option<AccountProfile>,
) -> SkinHostInstance {
    if host == SkinHostKind::Codex {
        return codex_instance_from_resolved(resolved, state, account_profile);
    }
    let label = format!("{} 进程 {}", host.display_name(), resolved.process.pid);
    SkinHostInstance {
        id: resolved.id,
        pid: resolved.process.pid,
        label,
        profile: resolved.profile,
        state,
        debug_port: resolved.debug_port,
        active_skin_name: None,
        active_skin: None,
        account_label: None,
        avatar_data_url: None,
    }
}

/// 在就绪期限内反复读取账户资料，直到同时拿到头像；超时则返回已得到的部分资料或最后的错误。
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

/// 读取全部 Codex 页面的账户名称与头像；只有各页面结果一致时才采用，全部探测失败时报错。
async fn discover_account_profile(browser: &Browser) -> Result<Option<AccountProfile>, AppError> {
    let pages = browser_pages(browser).await?;
    let mut labels = BTreeSet::new();
    let mut avatars = BTreeSet::new();
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
    if label.is_none() && avatar_data_url.is_none() {
        return Ok(None);
    }
    Ok(Some(AccountProfile {
        label,
        avatar_data_url,
    }))
}

/// 从页面运行态标记恢复生效皮肤身份：优先精确引用，其次旧版主题 ID，最后旧版样式文本；版本标记无效时放弃。
fn recovered_skin_identity(probe: Option<ActiveSkinProbe>) -> Option<RecoveredSkinIdentity> {
    let probe = probe?;
    let _version = probe.version.filter(|value| {
        !value.trim().is_empty()
            && value.chars().count() <= 80
            && !value.chars().any(char::is_control)
    })?;
    if let (Some(source), Some(id)) = (probe.source, probe.id)
        && is_valid_skin_id(&id) {
            return Some(RecoveredSkinIdentity::Exact(SkinReference { source, id }));
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

/// 只接受 PNG、JPEG 或 WebP 的 base64 data URL，且解码后非空并不超过头像大小上限。
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

/// 返回生效皮肤的展示名称。
fn displayed_active_skin_name(active: Option<&SkinDescriptor>) -> Option<String> {
    active.map(|skin| skin.name.clone())
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - instances_debug_port.rs：调试端口选择与候选收集，以及宿主操作的取消、退出监视与可取消等待。
include!("instances_debug_port.rs");
