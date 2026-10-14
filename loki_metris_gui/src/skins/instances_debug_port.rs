// 本文件是 instances.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：调试端口选择与候选收集，以及宿主操作的取消、退出监视与可取消等待。

/// 从宿主默认端口区间选择未被已知实例占用的本机监听端口。
fn available_debug_port_for(
    host: SkinHostKind,
    instances: &[ResolvedSkinHostInstance],
) -> Result<u16, AppError> {
    available_debug_port_for_excluding(host, instances, &[])
}

/// 选择端口时同时排除已占用端口和本轮已失败端口。
fn available_debug_port_for_excluding(
    host: SkinHostKind,
    instances: &[ResolvedSkinHostInstance],
    excluded_ports: &[u16],
) -> Result<u16, AppError> {
    let used = instances
        .iter()
        .filter_map(|instance| instance.debug_port)
        .collect::<HashSet<_>>();
    let start = CdpEndpoint::default_for(host).port;
    (start..=start + 99)
        .find(|port| {
            !used.contains(port)
                && !excluded_ports.contains(port)
                && std::net::TcpListener::bind(("127.0.0.1", *port)).is_ok()
        })
        .ok_or_else(|| {
            AppError::new(
                "skin.cdp_unavailable",
                format!(
                    "没有可用于重启所选 {} 实例的本机调试端口。",
                    host.display_name()
                ),
            )
        })
}

#[cfg(test)]
/// 从宿主进程命令行收集显式声明的调试端点候选。
fn endpoint_candidates_from_commands(commands: Vec<(u32, String)>) -> Vec<CdpEndpoint> {
    explicit_endpoint_candidates_from_commands(commands)
}

/// 只提取命令行真实声明的端口，不混入任何宿主默认回退。
fn explicit_endpoint_candidates_from_commands(
    mut commands: Vec<(u32, String)>,
) -> Vec<CdpEndpoint> {
    commands.sort_by_key(|command| std::cmp::Reverse(command.0));
    let mut ports = HashSet::new();
    commands
        .into_iter()
        .filter_map(|(_, command)| debug_port_from_command_line(&command))
        .filter(|port| ports.insert(*port))
        .map(CdpEndpoint::new)
        .collect()
}

/// 合并宿主命令行与已验证首选值；仅 WorkBuddy 保留其兼容默认端口。
async fn cdp_endpoint_candidates_for(
    host: SkinHostKind,
    preferred_endpoint: Option<CdpEndpoint>,
) -> Result<Vec<CdpEndpoint>, AppError> {
    Ok(host_endpoint_candidates_from_commands(
        host,
        platform_host_command_lines(host).await?,
        preferred_endpoint,
    ))
}

/// 在通用命令行端口候选上替换宿主默认值，保留 WorkBuddy renderer 的动态端口。
fn host_endpoint_candidates_from_commands(
    host: SkinHostKind,
    commands: Vec<(u32, String)>,
    preferred_endpoint: Option<CdpEndpoint>,
) -> Vec<CdpEndpoint> {
    let mut endpoints = explicit_endpoint_candidates_from_commands(commands);
    if host == SkinHostKind::WorkBuddy {
        let default = CdpEndpoint::default_for(host);
        if !endpoints.contains(&default) {
            endpoints.push(default);
        }
    }
    if let Some(preferred) = preferred_endpoint {
        endpoints.retain(|endpoint| *endpoint != preferred);
        endpoints.insert(0, preferred);
    }
    endpoints
}

/// 用户停止启动或页面搜寻时的稳定错误。
/// 因为调用方既有 Codex 专属路径，也有 WorkBuddy 等其它宿主共用的
/// 通用取消/关闭门禁（如 [`crate::skins::lifecycle`] 的 shutdown 门禁），
/// 后者在调用时并不持有具体的 [`SkinHostKind`]。
fn operation_cancelled() -> AppError {
    AppError::new("skin.operation_cancelled", "已停止启动或页面搜寻。")
}

/// Codex 在操作期间退出时的稳定错误。
fn codex_exited() -> AppError {
    AppError::new("skin.codex_exited", "Codex 已在启动或页面搜寻期间退出。")
}

/// Codex 已不在运行时返回退出错误。
fn ensure_codex_running(running: bool) -> Result<(), AppError> {
    running.then_some(()).ok_or_else(codex_exited)
}

/// 运行 Codex 操作，同时响应取消信号并定期确认 Codex 仍在运行，任一失败立即结束操作。
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

/// Codex 操作额外监视宿主退出，WorkBuddy 操作仅响应统一取消信号。
async fn monitor_host_operation<T, F>(
    host: SkinHostKind,
    cancel: &mut watch::Receiver<bool>,
    future: F,
) -> Result<T, AppError>
where
    F: Future<Output = Result<T, AppError>>,
{
    if host == SkinHostKind::Codex {
        return monitor_codex_operation(cancel, future).await;
    }
    run_cancellable(cancel, future).await
}

/// 运行操作并在完成前收到取消信号时立即返回取消错误；信号通道关闭也视为取消。
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

/// 可被取消信号提前打断的等待。
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
