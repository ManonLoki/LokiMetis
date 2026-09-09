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
    instances: &[ResolvedCodexInstance],
    endpoint: CdpEndpoint,
) -> Option<ResolvedCodexInstance> {
    let [instance] = instances else {
        return None;
    };
    (instance.debug_port == Some(endpoint.port)).then(|| instance.clone())
}

/// 注入事务提交前后必须仍是同一个根实例与端点。
fn workbuddy_target_binding_matches(
    expected: &ResolvedCodexInstance,
    current: &ResolvedCodexInstance,
    endpoint: CdpEndpoint,
) -> bool {
    expected.id == current.id && current.debug_port == Some(endpoint.port)
}

/// 重新发现并验证 Windows WorkBuddy 的唯一目标仍绑定指定端点。
async fn current_windows_workbuddy_target(
    endpoint: CdpEndpoint,
) -> Result<ResolvedCodexInstance, AppError> {
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
    /// 在一次受确认的操作内收敛 WorkBuddy，并把已验证端点绑定到唯一官方进程树根。
    async fn recover_windows_workbuddy_target(
        &self,
        cancel: &mut watch::Receiver<bool>,
        mut preferred_endpoint: Option<CdpEndpoint>,
    ) -> Result<(ResolvedCodexInstance, CdpEndpoint), AppError> {
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

    /// 执行换皮宿主内部的 `install` 步骤。
    pub async fn install(
        &self,
        host: SkinHostKind,
        skin: &SkinReference,
        allow_appearance_mismatch: bool,
        instance_id: Option<&str>,
        allow_workbuddy_recovery: bool,
        allow_third_party_code: bool,
    ) -> Result<InstallSkinResult, AppError> {
        validate_skin_reference(skin)?;
        if allow_workbuddy_recovery && !replaces_all_host_runtimes(host) {
            return Err(AppError::new(
                "skin.workbuddy_close_all_unsupported",
                "关闭全部 WorkBuddy 的恢复流程仅在 Windows 上可用。",
            ));
        }
        let loaded = load_skin(
            &self.builtin_root,
            &self.user_root,
            skin,
            allow_third_party_code,
        )?;
        let _operation = self.operation.lock().await;
        let _runtime_mutation = self.begin_host_runtime_mutation(host);
        self.ensure_watch_runtime_open()?;
        self.reap_watch_tasks().await?;
        let (_guard, mut cancel) = self.begin_codex_operation()?;
        let mut preferred_endpoint = self.verified_endpoint_hint(host)?;

        let mut selected_instance = if allow_workbuddy_recovery {
            let (instance, endpoint) = self
                .recover_windows_workbuddy_target(&mut cancel, preferred_endpoint)
                .await?;
            preferred_endpoint = Some(endpoint);
            Some(instance)
        } else {
            match instance_id {
                Some(id) => {
                    Some(resolve_host_instance_with_preferred(host, id, preferred_endpoint).await?)
                }
                None if host == SkinHostKind::WorkBuddy => {
                    let mut instances =
                        resolved_host_instances_with_preferred(host, preferred_endpoint).await?;
                    if instances.len() > 1 {
                        return Err(AppError::new(
                            "skin.host_instance_selection_required",
                            format!(
                                "检测到多个 {} 实例，请在LokiMetis中选择应用目标。",
                                host.display_name()
                            ),
                        ));
                    }
                    instances.pop()
                }
                None => {
                    if platform_host_processes(host).await?.len() > 1 {
                        return Err(AppError::new(
                            "skin.host_instance_selection_required",
                            format!(
                                "检测到多个 {} 实例，请在LokiMetis中选择应用目标。",
                                host.display_name()
                            ),
                        ));
                    }
                    None
                }
            }
        };
        let target_instance_id = selected_instance
            .as_ref()
            .map(|instance| instance.id.clone());
        let target_key = target_instance_id
            .as_deref()
            .map(|instance_id| runtime_instance_key(host, instance_id));
        let already_installed = {
            let runtime = self.runtime.lock().await;
            let instance = target_key
                .as_ref()
                .and_then(|target| runtime.instances.get(target));
            let is_running = instance.is_some_and(|instance| {
                instance
                    .active
                    .as_ref()
                    .is_some_and(|active| active.source == skin.source && active.id == skin.id)
                    && instance.task.as_ref().is_some_and(WatchTask::is_running)
            });
            if is_running {
                let compatibility = instance.and_then(|value| value.compatibility.clone());
                let endpoint = instance
                    .and_then(|instance| instance.task.as_ref())
                    .map(|task| task.endpoint);
                Some((
                    InstallSkinResult::Installed {
                        status: SkinStatus::running(&loaded.descriptor, 0, compatibility)
                            .for_instance(target_instance_id.clone().unwrap_or_default()),
                    },
                    endpoint,
                ))
            } else {
                None
            }
        };
        if let Some((result, endpoint)) = already_installed {
            let binding_is_still_current = if replaces_all_host_runtimes(host) {
                match (selected_instance.as_ref(), endpoint) {
                    (Some(expected), Some(endpoint)) => current_windows_workbuddy_target(endpoint)
                        .await
                        .is_ok_and(|current| {
                            workbuddy_target_binding_matches(expected, &current, endpoint)
                        }),
                    _ => false,
                }
            } else {
                true
            };
            if binding_is_still_current {
                if let Some(endpoint) = endpoint {
                    try_activate_host_window(host, endpoint).await;
                }
                return Ok(result);
            }
        }

        let connection = connect_or_launch(
            host,
            &mut cancel,
            selected_instance.as_ref(),
            preferred_endpoint,
        )
        .await;
        let (mut browser, handler_task, connection_source, endpoint, verified_root_pid) =
            match connection {
                Ok(connection) => connection,
                Err(error)
                    if allow_workbuddy_recovery
                        && error.code == "skin.workbuddy_recovery_required" =>
                {
                    let (instance, recovered_endpoint) = self
                        .recover_windows_workbuddy_target(&mut cancel, preferred_endpoint)
                        .await?;
                    preferred_endpoint = Some(recovered_endpoint);
                    selected_instance = Some(instance);
                    connect_or_launch(
                        host,
                        &mut cancel,
                        selected_instance.as_ref(),
                        preferred_endpoint,
                    )
                    .await
                    .map_err(authorized_workbuddy_error)?
                }
                Err(error) => return Err(error),
            };
        self.remember_verified_endpoint(host, endpoint)?;
        if replaces_all_host_runtimes(host) {
            let current = current_windows_workbuddy_target(endpoint).await;
            let current = match current {
                Ok(current) => current,
                Err(error) => {
                    handler_task.abort();
                    drop(browser);
                    return Err(error);
                }
            };
            if !allow_workbuddy_recovery
                && selected_instance
                    .as_ref()
                    .is_some_and(|selected| selected.id != current.id)
            {
                handler_task.abort();
                drop(browser);
                return Err(AppError::new(
                    "skin.host_instance_changed",
                    "所选 WorkBuddy 实例已退出或身份发生变化，请重新选择。",
                ));
            }
            selected_instance = Some(current);
        }
        let target_instance_id = selected_instance
            .as_ref()
            .map(|instance| instance.id.clone());
        let target_key = target_instance_id
            .as_deref()
            .map(|instance_id| runtime_instance_key(host, instance_id));
        let target_instance_id =
            target_instance_id.unwrap_or_else(|| format!("endpoint:{}", endpoint.port));
        let target_key =
            target_key.unwrap_or_else(|| runtime_instance_key(host, &target_instance_id));
        let policy = AppearancePolicy {
            supported_color_modes: loaded.descriptor.supported_color_modes.clone(),
            requirements: loaded.appearance_requirements.clone(),
        };
        let appearance_check = match wait_for_initial_appearance(
            host,
            &mut browser,
            connection_source.page_ready_timeout(),
            &policy,
            &mut cancel,
        )
        .await
        {
            Ok(check) => check,
            Err(error) => {
                handler_task.abort();
                let error = remap_workbuddy_connection_error(host, error).await;
                return Err(if allow_workbuddy_recovery {
                    authorized_workbuddy_error(error)
                } else {
                    error
                });
            }
        };
        if !allow_appearance_mismatch
            && (!appearance_check.differences.is_empty() || !appearance_check.unreadable.is_empty())
        {
            handler_task.abort();
            return Ok(InstallSkinResult::NeedsConfirmation {
                check: appearance_check,
            });
        }

        if replaces_all_host_runtimes(host) {
            let current = current_windows_workbuddy_target(endpoint).await;
            let binding_is_stable = current.as_ref().is_ok_and(|current| {
                selected_instance.as_ref().is_some_and(|expected| {
                    workbuddy_target_binding_matches(expected, current, endpoint)
                })
            });
            if !binding_is_stable {
                handler_task.abort();
                drop(browser);
                return Err(current.err().unwrap_or_else(|| {
                    workbuddy_recovery_unstable(
                        "WorkBuddy 在皮肤应用前完成了进程交接，未应用皮肤，请重试。",
                    )
                }));
            }
        }
        let previous_tasks = {
            let mut runtime = self.runtime.lock().await;
            take_replaced_watch_tasks(&mut runtime, host, &target_key)
        };
        if let Err(error) = self.stop_watch_tasks(previous_tasks).await {
            handler_task.abort();
            drop(browser);
            return Err(error);
        }
        let transaction_id = uuid::Uuid::new_v4().to_string();
        let injection = wait_for_initial_injection(
            host,
            browser,
            handler_task,
            &loaded.payload,
            skin,
            connection_source.page_ready_timeout(),
            endpoint,
            verified_root_pid,
            &transaction_id,
            &mut cancel,
        )
        .await;
        let (mut browser, handler_task, report) = match injection {
            Ok(result) => result,
            Err(error) => {
                let error = remap_workbuddy_connection_error(host, error).await;
                return Err(if allow_workbuddy_recovery {
                    authorized_workbuddy_error(error)
                } else {
                    error
                });
            }
        };
        let mut handler_task = HandlerTaskGuard::new(handler_task);

        if *cancel.borrow() {
            let error = operation_cancelled();
            let rollback = rollback_initial_injection(
                host,
                &mut browser,
                &mut handler_task,
                endpoint,
                verified_root_pid,
                &transaction_id,
                &report.transaction_targets,
            )
            .await;
            handler_task.abort();
            drop(browser);
            return Err(match rollback {
                Ok(_) => error,
                Err(rollback) => rollback_error(&error, rollback),
            });
        }

        if replaces_all_host_runtimes(host) {
            let current = current_windows_workbuddy_target(endpoint).await;
            let binding_is_stable = current.as_ref().is_ok_and(|current| {
                selected_instance.as_ref().is_some_and(|expected| {
                    workbuddy_target_binding_matches(expected, current, endpoint)
                })
            });
            if !binding_is_stable {
                let error = current.err().unwrap_or_else(|| {
                    workbuddy_recovery_unstable(
                        "WorkBuddy 在皮肤注入期间完成了进程交接，运行状态未提交。",
                    )
                });
                let rollback = rollback_initial_injection(
                    host,
                    &mut browser,
                    &mut handler_task,
                    endpoint,
                    verified_root_pid,
                    &transaction_id,
                    &report.transaction_targets,
                )
                .await;
                handler_task.abort();
                drop(browser);
                return Err(match rollback {
                    Ok(_) => error,
                    Err(rollback) => rollback_error(&error, rollback),
                });
            }
        }

        if *cancel.borrow() {
            let error = operation_cancelled();
            let rollback = rollback_initial_injection(
                host,
                &mut browser,
                &mut handler_task,
                endpoint,
                verified_root_pid,
                &transaction_id,
                &report.transaction_targets,
            )
            .await;
            handler_task.abort();
            drop(browser);
            return Err(match rollback {
                Ok(_) => error,
                Err(rollback) => rollback_error(&error, rollback),
            });
        }

        let Some(handler_task_value) = handler_task.take() else {
            let error = AppError::new("skin.cdp_failed", "宿主调试会话已意外结束。");
            let rollback = rollback_initial_injection(
                host,
                &mut browser,
                &mut handler_task,
                endpoint,
                verified_root_pid,
                &transaction_id,
                &report.transaction_targets,
            )
            .await;
            handler_task.abort();
            drop(browser);
            return Err(match rollback {
                Ok(_) => error,
                Err(rollback) => rollback_error(&error, rollback),
            });
        };

        let (cancel, cancel_rx) = watch::channel(false);
        let registration = match self.register_active_watch_task(cancel.clone()) {
            Ok(registration) => registration,
            Err(error) => {
                let mut handler_task = HandlerTaskGuard::new(handler_task_value);
                let rollback = rollback_initial_injection(
                    host,
                    &mut browser,
                    &mut handler_task,
                    endpoint,
                    verified_root_pid,
                    &transaction_id,
                    &report.transaction_targets,
                )
                .await;
                handler_task.abort();
                drop(browser);
                return Err(match rollback {
                    Ok(_) => error,
                    Err(rollback) => rollback_error(&error, rollback),
                });
            }
        };
        let payload = Arc::clone(&loaded.payload);
        let watched_skin = skin.clone();
        let handler_abort = handler_task_value.abort_handle();
        let initial_transaction = Some((transaction_id, report.transaction_targets.clone()));
        let join = tokio::spawn(async move {
            let _registration = registration;
            watch_pages(
                host,
                browser,
                handler_task_value,
                payload,
                watched_skin,
                initial_transaction,
                cancel_rx,
            )
            .await
        });
        let watch_task = WatchTask::new(
            &self.watch_task_reaper,
            host,
            cancel,
            join,
            handler_abort,
            endpoint,
        );
        let mut runtime = self.runtime.lock().await;
        let shutting_down = self.lock_watch_task_reaper().shutting_down;
        if shutting_down {
            drop(runtime);
            let error = operation_cancelled();
            let cleanup = self.stop_watch_tasks(vec![watch_task]).await;
            return Err(match cleanup {
                Ok(_) => error,
                Err(cleanup) => rollback_error(&error, cleanup),
            });
        }
        let compatibility = report.compatibility_status();
        runtime.instances.insert(
            target_key.clone(),
            InstanceRuntime {
                active: Some(loaded.descriptor.clone()),
                compatibility: Some(compatibility.clone()),
                task: Some(watch_task),
            },
        );
        runtime.last_targets.insert(host, target_key.clone());
        let result = InstallSkinResult::Installed {
            status: SkinStatus::running(
                &loaded.descriptor,
                report.injected_pages,
                Some(compatibility),
            )
            .for_instance(target_instance_id),
        };
        drop(runtime);
        try_activate_host_window(host, endpoint).await;
        Ok(result)
    }

    /// 执行换皮宿主内部的 `uninstall` 步骤。
    pub async fn uninstall(
        &self,
        host: SkinHostKind,
        instance_id: Option<&str>,
    ) -> Result<SkinStatus, AppError> {
        let _operation = self.operation.lock().await;
        let _runtime_mutation = self.begin_host_runtime_mutation(host);
        self.reap_watch_tasks().await?;
        let preferred_endpoint = self.verified_endpoint_hint(host)?;
        let cleanup_endpoint = match instance_id {
            Some(id) => {
                match resolve_host_instance_with_preferred(host, id, preferred_endpoint).await {
                    Ok(instance) => instance.debug_port.map(CdpEndpoint::new),
                    Err(error)
                        if replaces_all_host_runtimes(host)
                            && error.code == "skin.host_instance_changed" =>
                    {
                        None
                    }
                    Err(error) => return Err(error),
                }
            }
            None => None,
        };
        let target_key = instance_id.map(|id| runtime_instance_key(host, id));
        let tasks = {
            let mut runtime = self.runtime.lock().await;
            take_uninstall_watch_tasks(&mut runtime, host, target_key.as_deref())
        };
        let had_tasks = !tasks.is_empty();
        let mut affected_pages = self.stop_watch_tasks(tasks).await?;
        if replaces_all_host_runtimes(host) {
            let endpoint = cleanup_endpoint.or(preferred_endpoint);
            let removed = if let Some(endpoint) = endpoint {
                remove_from_endpoint(host, endpoint).await?
            } else {
                0
            };
            affected_pages = affected_pages.saturating_add(removed);
            if removed == 0 {
                affected_pages =
                    affected_pages.saturating_add(remove_from_existing_endpoint(host).await?);
            }
        } else if !had_tasks {
            affected_pages = if let Some(endpoint) = cleanup_endpoint {
                remove_from_endpoint(host, endpoint).await?
            } else {
                remove_from_existing_endpoint(host).await?
            };
        }
        Ok(SkinStatus::stopped(affected_pages))
    }
}
