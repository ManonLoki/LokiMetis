// 本文件是 service_install_body.rs 的分片（第 2/3 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：安装皮肤（加载校验、宿主启动或恢复、外观预检、注入与持续监视）。

impl SkinService {
    /// 在串行操作锁内为宿主安装皮肤：加载并校验皮肤，必要时启动或恢复宿主，外观不一致且未获同意时返回待确认结果，否则注入页面并启动持续监视。
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
        let (_install_completion, install_cancel, mut cancel) = self.begin_install_completion()?;
        let _codex_operation = self.bind_install_codex_operation(install_cancel)?;
        let _runtime_mutation = self.begin_host_runtime_mutation(host);
        self.ensure_watch_runtime_open()?;
        self.reap_watch_tasks().await?;
        if *cancel.borrow() {
            return Err(operation_cancelled());
        }
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
                                "检测到多个 {} 实例，请在LokiMetris中选择应用目标。",
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
                                "检测到多个 {} 实例，请在LokiMetris中选择应用目标。",
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

        check_host_authority()?;
        let connection = connect_or_launch(
            host,
            &mut cancel,
            selected_instance.as_ref(),
            preferred_endpoint,
        )
        .await;
        let (mut browser, mut handler_task, connection_source, endpoint, verified_root_pid) =
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
        if let Err(error) = check_host_authority() {
            handler_task.abort();
            drop(browser);
            return Err(error);
        }
        let previous_tasks = {
            let mut runtime = self.runtime.lock().await;
            check_host_authority()?;
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
        let (mut browser, mut handler_task, report) = match injection {
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

        let Some(handler_abort) = handler_task.abort_handle() else {
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
        let initial_transaction = Some((transaction_id, report.transaction_targets.clone()));
        let join = tokio::spawn(async move {
            let _registration = registration;
            watch_pages(
                host,
                browser,
                handler_task,
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
}
