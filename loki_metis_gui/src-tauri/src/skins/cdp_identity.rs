/// 验证浏览器包含目标宿主页面，并把端口 owner 绑定到可信进程根。
async fn browser_matches_host(
    host: SkinHostKind,
    browser: &mut Browser,
    endpoint: CdpEndpoint,
) -> Result<bool, AppError> {
    browser_matches_host_for_root(host, browser, endpoint, None).await
}

/// 验证浏览器与可选的宿主根进程精确绑定。
async fn browser_matches_host_for_root(
    host: SkinHostKind,
    browser: &mut Browser,
    endpoint: CdpEndpoint,
    expected_host_root_pid: Option<u32>,
) -> Result<bool, AppError> {
    Ok(
        browser_host_root_pid(host, browser, endpoint, expected_host_root_pid)
            .await?
            .is_some(),
    )
}

/// 验证宿主页和 CDP listener owner，并返回本次绑定的可信根 PID。
async fn browser_host_root_pid(
    host: SkinHostKind,
    browser: &mut Browser,
    endpoint: CdpEndpoint,
    expected_host_root_pid: Option<u32>,
) -> Result<Option<u32>, AppError> {
    fetch_targets(browser).await?;
    for page in browser_pages(browser).await? {
        if !is_host_page(host, &page).await? {
            continue;
        }
        let processes = platform_host_processes(host).await?;
        let root_pid = if host == SkinHostKind::WorkBuddy {
            trusted_workbuddy_root_pid(&processes, expected_host_root_pid)
        } else {
            trusted_codex_root_pid(&processes, endpoint, expected_host_root_pid)
        };
        let Some(root_pid) = root_pid else {
            return Ok(None);
        };
        // 平台 owner 校验同时承担 mutation trust：macOS 会在此对根 PID
        // 重做 strict+nested+resources 签名验证，不把轻量枚举结果当成注入授权。
        let owned = if host == SkinHostKind::WorkBuddy {
            platform_workbuddy_endpoint_owned_by_root(endpoint.port, root_pid).await?
        } else {
            platform_codex_endpoint_owned_by_root(endpoint.port, root_pid).await?
        };
        return Ok(owned.then_some(root_pid));
    }
    Ok(None)
}

/// 选择端点必须绑定的 WorkBuddy 根 PID；指定实例时不得改用其它唯一实例。
fn trusted_workbuddy_root_pid(
    processes: &[PlatformCodexProcess],
    expected_workbuddy_root_pid: Option<u32>,
) -> Option<u32> {
    exactly_one(processes.iter().filter(|process| {
        expected_workbuddy_root_pid.is_none_or(|expected| process.pid == expected)
    }))
    .map(|process| process.pid)
}

/// 选择 Codex 端点必须绑定的根 PID；声明端口冲突或多实例均失败关闭。
fn trusted_codex_root_pid(
    processes: &[PlatformCodexProcess],
    endpoint: CdpEndpoint,
    expected_root_pid: Option<u32>,
) -> Option<u32> {
    if let Some(expected) = expected_root_pid {
        return exactly_one(processes.iter().filter(|process| process.pid == expected))
            .map(|process| process.pid);
    }
    if let Some(process) = exactly_one(processes.iter().filter(|process| {
        debug_port_from_command_line(&process.command_line) == Some(endpoint.port)
    })) {
        return Some(process.pid);
    }
    let [only] = processes else {
        return None;
    };
    debug_port_from_command_line(&only.command_line)
        .is_none()
        .then_some(only.pid)
}

/// 首次注入期间只接受同一个已验证根 PID，owner 消失或交接都失败关闭。
fn endpoint_owner_is_stable(expected_root_pid: u32, current_root_pid: Option<u32>) -> bool {
    current_root_pid == Some(expected_root_pid)
}

/// 在会对宿主页写入前复核页面、监听 owner 与根 PID 的同一绑定。
async fn verify_stable_endpoint_binding(
    host: SkinHostKind,
    browser: &mut Browser,
    endpoint: CdpEndpoint,
    expected_root_pid: u32,
) -> Result<(), AppError> {
    let current_root_pid =
        browser_host_root_pid(host, browser, endpoint, Some(expected_root_pid)).await?;
    endpoint_owner_is_stable(expected_root_pid, current_root_pid)
        .then_some(())
        .ok_or_else(|| {
            AppError::new(
                "skin.cdp_owner_changed",
                format!(
                    "{} 调试端口所属进程在皮肤应用期间发生变化，未提交皮肤。",
                    host.display_name()
                ),
            )
        })
}
