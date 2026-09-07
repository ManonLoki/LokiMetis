impl SkinService {
    /// 执行换皮宿主内部的 `install` 步骤。
    pub async fn install(
        &self,
        host: SkinHostKind,
        skin: &SkinReference,
        allow_appearance_mismatch: bool,
        instance_id: Option<&str>,
    ) -> Result<InstallSkinResult, AppError> {
        validate_skin_reference(skin)?;
        let loaded = load_skin(&self.builtin_root, &self.user_root, skin)?;
        let _operation = self.operation.lock().await;
        let (_guard, mut cancel) = self.begin_codex_operation()?;

        let selected_instance = match instance_id {
            Some(id) => Some(resolve_host_instance(host, id).await?),
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
        };
        let target_instance_id = selected_instance.as_ref().map(|instance| instance.id.clone());
        let target_key = target_instance_id
            .as_deref()
            .map(|instance_id| runtime_instance_key(host, instance_id));
        let already_installed = {
            let runtime = self.runtime.lock().await;
            let instance = target_key
                .as_ref()
                .and_then(|target| runtime.instances.get(target));
            let is_running = instance.is_some_and(|instance| {
                instance.active.as_ref().is_some_and(|active| {
                    active.source == skin.source && active.id == skin.id
                }) && instance
                    .task
                    .as_ref()
                    .is_some_and(|task| !task.join.is_finished())
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
            if let Some(endpoint) = endpoint {
                try_activate_host_window(host, endpoint).await;
            }
            return Ok(result);
        }

        let (mut browser, handler_task, connection_source, endpoint) =
            connect_or_launch(host, &mut cancel, selected_instance.as_ref()).await?;
        let target_instance_id =
            target_instance_id.unwrap_or_else(|| format!("endpoint:{}", endpoint.port));
        let target_key = target_key
            .unwrap_or_else(|| runtime_instance_key(host, &target_instance_id));
        let policy = AppearancePolicy {
            supported_color_modes: loaded.descriptor.supported_color_modes.clone(),
            requirements: loaded.appearance_requirements.clone(),
        };
        let appearance_check = wait_for_initial_appearance(
            host,
            &mut browser,
            connection_source.page_ready_timeout(),
            &policy,
            &mut cancel,
        )
        .await?;
        if !allow_appearance_mismatch
            && (!appearance_check.differences.is_empty() || !appearance_check.unreadable.is_empty())
        {
            handler_task.abort();
            return Ok(InstallSkinResult::NeedsConfirmation {
                check: appearance_check,
            });
        }

        let previous_task = {
            let mut runtime = self.runtime.lock().await;
            runtime
                .instances
                .remove(&target_key)
                .and_then(|instance| instance.task)
        };
        stop_watch_task(previous_task).await?;
        let (browser, handler_task, report) = wait_for_initial_injection(
            host,
            browser,
            handler_task,
            &loaded.payload,
            skin,
            connection_source.page_ready_timeout(),
            endpoint,
            &mut cancel,
        )
        .await?;

        let (cancel, cancel_rx) = watch::channel(false);
        let payload = Arc::clone(&loaded.payload);
        let watched_skin = skin.clone();
        let handler_abort = handler_task.abort_handle();
        let join = tokio::spawn(async move {
            watch_pages(host, browser, handler_task, payload, watched_skin, cancel_rx).await
        });
        let mut runtime = self.runtime.lock().await;
        let compatibility = report.compatibility_status();
        runtime.instances.insert(
            target_key.clone(),
            InstanceRuntime {
                active: Some(loaded.descriptor.clone()),
                compatibility: Some(compatibility.clone()),
                task: Some(WatchTask {
                    host,
                    cancel,
                    join,
                    handler_abort,
                    endpoint,
                }),
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
        let cleanup_endpoint = match instance_id {
            Some(id) => resolve_host_instance(host, id)
                .await?
                .debug_port
                .map(CdpEndpoint::new),
            None => None,
        };
        let task = {
            let mut runtime = self.runtime.lock().await;
            let target = instance_id
                .map(|id| runtime_instance_key(host, id))
                .or_else(|| runtime.last_targets.get(&host).cloned());
            let task = target
                .as_ref()
                .and_then(|target| runtime.instances.remove(target))
                .and_then(|instance| instance.task);
            runtime.last_targets.remove(&host);
            task
        };
        let affected_pages = if task.is_some() {
            stop_watch_task(task).await?
        } else if let Some(endpoint) = cleanup_endpoint {
            remove_from_endpoint(host, endpoint).await?
        } else {
            remove_from_existing_endpoint(host).await?
        };
        Ok(SkinStatus::stopped(affected_pages))
    }
}
