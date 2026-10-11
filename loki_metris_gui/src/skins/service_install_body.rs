/// 在完整安装事务退出前持续占用 shutdown completion 登记。
struct InstallCompletionGuard {
    _registration: ActiveWatchRegistration,
}

/// Windows WorkBuddy 的恢复会关闭全部旧进程，因此新 PID 必须接管该宿主的全部旧运行态。
fn replaces_all_host_runtimes(host: SkinHostKind) -> bool {
    cfg!(target_os = "windows") && host == SkinHostKind::WorkBuddy
}

/// 构造恢复期间实例或端点漂移的稳定错误，阻止继续提交皮肤。
fn workbuddy_recovery_unstable(message: impl Into<String>) -> AppError {
    AppError::new("skin.workbuddy_recovery_unstable", message)
}

/// 将用户已授权恢复后再次出现的确认请求改写为不可继续的漂移错误。
fn authorized_workbuddy_error(error: AppError) -> AppError {
    if error.code == "skin.workbuddy_recovery_required" {
        workbuddy_recovery_unstable(
            "WorkBuddy 在已确认的恢复操作中再次失去调试连接，未应用皮肤，请重试。",
        )
    } else {
        error
    }
}

/// 只接受绑定到指定端点的唯一 WorkBuddy 根，避免孤立页面或启动交接被误认成目标。
fn unique_workbuddy_target_for_endpoint(
    instances: &[ResolvedSkinHostInstance],
    endpoint: CdpEndpoint,
) -> Option<ResolvedSkinHostInstance> {
    let [instance] = instances else {
        return None;
    };
    (instance.debug_port == Some(endpoint.port)).then(|| instance.clone())
}

/// 注入事务提交前后必须仍是同一个根实例与端点。
fn workbuddy_target_binding_matches(
    expected: &ResolvedSkinHostInstance,
    current: &ResolvedSkinHostInstance,
    endpoint: CdpEndpoint,
) -> bool {
    expected.id == current.id && current.debug_port == Some(endpoint.port)
}

/// 重新发现并验证 Windows WorkBuddy 的唯一目标仍绑定指定端点。
async fn current_windows_workbuddy_target(
    endpoint: CdpEndpoint,
) -> Result<ResolvedSkinHostInstance, AppError> {
    let instances =
        resolved_host_instances_with_preferred(SkinHostKind::WorkBuddy, Some(endpoint)).await?;
    unique_workbuddy_target_for_endpoint(&instances, endpoint).ok_or_else(|| {
        workbuddy_recovery_unstable(
            "WorkBuddy 恢复期间实例或调试端点发生变化，未应用皮肤，请重试。",
        )
    })
}

/// 摘除新安装即将替代的监视任务，并清掉已失效 PID 对应的活动皮肤占位。
fn take_replaced_watch_tasks(
    runtime: &mut RuntimeState,
    host: SkinHostKind,
    target_key: &str,
) -> Vec<WatchTask> {
    if !replaces_all_host_runtimes(host) {
        return runtime
            .instances
            .remove(target_key)
            .and_then(|instance| instance.task)
            .into_iter()
            .collect();
    }
    let keys = runtime
        .instances
        .keys()
        .filter(|key| runtime_instance_belongs_to_host(host, key))
        .cloned()
        .collect::<Vec<_>>();
    keys.into_iter()
        .filter_map(|key| runtime.instances.remove(&key))
        .filter_map(|instance| instance.task)
        .collect()
}

/// 摘除停用目标。Windows WorkBuddy 使用宿主级回收，以覆盖恢复后 PID 已变化的场景。
fn take_uninstall_watch_tasks(
    runtime: &mut RuntimeState,
    host: SkinHostKind,
    target_key: Option<&str>,
) -> Vec<WatchTask> {
    let tasks = if replaces_all_host_runtimes(host) {
        let keys = runtime
            .instances
            .keys()
            .filter(|key| runtime_instance_belongs_to_host(host, key))
            .cloned()
            .collect::<Vec<_>>();
        keys.into_iter()
            .filter_map(|key| runtime.instances.remove(&key))
            .filter_map(|instance| instance.task)
            .collect()
    } else {
        target_key
            .map(str::to_owned)
            .or_else(|| runtime.last_targets.get(&host).cloned())
            .and_then(|target| runtime.instances.remove(&target))
            .and_then(|instance| instance.task)
            .into_iter()
            .collect()
    };
    runtime.last_targets.remove(&host);
    tasks
}

impl SkinService {
    /// 为完整安装事务建立退出 completion guard，并返回同一持久取消通道。
    fn begin_install_completion(
        &self,
    ) -> Result<
        (
            InstallCompletionGuard,
            watch::Sender<bool>,
            watch::Receiver<bool>,
        ),
        AppError,
    > {
        check_host_authority()?;
        let (cancel, receiver) = watch::channel(false);
        let registration = self.register_shutdown_participant(cancel.clone())?;
        Ok((
            InstallCompletionGuard {
                _registration: registration,
            },
            cancel,
            receiver,
        ))
    }

    /// 把安装 completion 的同一发送端发布为当前宿主操作取消入口。
    fn bind_install_codex_operation(
        &self,
        cancel: watch::Sender<bool>,
    ) -> Result<CodexOperationGuard<'_>, AppError> {
        let id = self.next_codex_operation_id.fetch_add(1, Ordering::Relaxed);
        let mut active = self.codex_operation.lock().map_err(|_| {
            AppError::new(
                "skin.operation_state_failed",
                "无法准备 Codex 操作状态，请重启应用后重试。",
            )
        })?;
        *active = Some(CodexOperation { id, cancel });
        Ok(CodexOperationGuard {
            id,
            active: &self.codex_operation,
        })
    }

    /// 在一次受确认的操作内收敛 WorkBuddy，并把已验证端点绑定到唯一官方进程树根。
    async fn recover_windows_workbuddy_target(
        &self,
        cancel: &mut watch::Receiver<bool>,
        mut preferred_endpoint: Option<CdpEndpoint>,
    ) -> Result<(ResolvedSkinHostInstance, CdpEndpoint), AppError> {
        let mut excluded_ports = Vec::new();
        let mut last_error = None;
        for attempt in 0..2 {
            let mut attempted_endpoint = None;
            let recovery = force_launch_and_wait_for_cdp(
                SkinHostKind::WorkBuddy,
                cancel,
                preferred_endpoint,
                &excluded_ports,
                &mut attempted_endpoint,
            )
            .await;
            let (_, endpoint) = match recovery {
                Ok(result) => result,
                Err(error)
                    if attempted_endpoint.is_some()
                        && (is_workbuddy_connection_error(error.code)
                            || error.code == "skin.workbuddy_recovery_required") =>
                {
                    let Some(attempted_endpoint) = attempted_endpoint else {
                        return Err(error);
                    };
                    excluded_ports.push(attempted_endpoint.port);
                    preferred_endpoint = None;
                    last_error = Some(error);
                    if attempt == 0 {
                        continue;
                    }
                    force_close_platform_host(SkinHostKind::WorkBuddy).await?;
                    return Err(workbuddy_recovery_unstable(
                        "WorkBuddy 已尝试两个本机调试端口但均未就绪，新实例已关闭，请重试。",
                    ));
                }
                Err(error) => return Err(error),
            };
            let endpoint = endpoint.ok_or_else(|| {
                AppError::new(
                    "skin.workbuddy_recovery_unstable",
                    "WorkBuddy 已启动，但没有返回可验证的调试端点。",
                )
            })?;
            self.remember_verified_endpoint(SkinHostKind::WorkBuddy, endpoint)?;
            preferred_endpoint = Some(endpoint);
            let instances =
                resolved_host_instances_with_preferred(SkinHostKind::WorkBuddy, preferred_endpoint)
                    .await?;
            if let Some(instance) = unique_workbuddy_target_for_endpoint(&instances, endpoint) {
                return Ok((instance, endpoint));
            }
            excluded_ports.push(endpoint.port);
            preferred_endpoint = None;
            last_error = Some(workbuddy_recovery_unstable(
                "WorkBuddy 已就绪，但实例与调试端点无法稳定绑定。",
            ));
            if attempt == 1 {
                force_close_platform_host(SkinHostKind::WorkBuddy).await?;
            }
        }
        Err(last_error.unwrap_or_else(|| {
            workbuddy_recovery_unstable("WorkBuddy 恢复失败，未应用皮肤，请重试。")
        }))
    }
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - service_install_body_install.rs：执行换皮宿主内部的 `install` 步骤。
// - service_install_body_uninstall.rs：执行换皮宿主内部的 `uninstall` 步骤。
include!("service_install_body_install.rs");
include!("service_install_body_uninstall.rs");
