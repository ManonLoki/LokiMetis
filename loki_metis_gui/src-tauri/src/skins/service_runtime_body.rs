impl SkinService {
    /// 执行换皮宿主内部的 `status` 步骤。
    pub async fn status(&self) -> SkinStatus {
        let runtime = self.runtime.lock().await;
        runtime
            .last_target
            .as_ref()
            .and_then(|target| runtime.instances.get(target))
            .filter(|instance| {
                instance
                    .task
                    .as_ref()
                    .is_some_and(|task| !task.join.is_finished())
            })
            .and_then(|instance| instance.active.as_ref().map(|skin| (instance, skin)))
            .map(|(instance, skin)| {
                let target = runtime.last_target.as_deref().unwrap_or_default();
                SkinStatus::running(skin, 0, instance.compatibility.clone()).for_instance(target)
            })
            .unwrap_or_else(|| SkinStatus::stopped(0))
    }

    /// 执行换皮宿主内部的 `scanned_codex_instances` 步骤。
    pub async fn scanned_codex_instances(&self) -> Result<Vec<CodexInstance>, AppError> {
        let mut instances = discover_codex_process_instances().await?;
        self.retain_account_profile_probes(instances.iter().map(|instance| instance.id.as_str()));
        self.retain_recovered_instance_runtimes(
            instances.iter().map(|instance| instance.id.as_str()),
        )
        .await;
        self.annotate_codex_instances(&mut instances).await;
        Ok(instances)
    }

    /// 执行换皮宿主内部的 `probe_codex_instance` 步骤。
    pub async fn probe_codex_instance(&self, instance_id: &str) -> Result<CodexInstance, AppError> {
        let resolved = resolve_codex_instance(instance_id).await?;
        let mut instances = vec![self.cached_codex_instance(resolved).await?];
        self.annotate_codex_instances(&mut instances).await;
        instances.pop().ok_or_else(|| {
            AppError::new(
                "skin.codex_instance_changed",
                "所选 Codex 实例已退出或身份发生变化，请重新选择。",
            )
        })
    }

    /// 执行换皮宿主内部的 `account_profile_probe_cell` 步骤。
    fn account_profile_probe_cell(
        &self,
        instance_id: &str,
    ) -> Result<Arc<OnceCell<Option<AccountProfile>>>, AppError> {
        let mut probes = self.account_profile_probes.lock().map_err(|_| {
            AppError::new(
                "skin.account_profile_state_failed",
                "无法读取 Codex 账户资料探测状态，请重启LokiMetis后重试。",
            )
        })?;
        Ok(probes
            .entry(instance_id.to_owned())
            .or_insert_with(|| Arc::new(OnceCell::new()))
            .clone())
    }

    /// 执行换皮宿主内部的 `retain_account_profile_probes` 步骤。
    fn retain_account_profile_probes<'a>(&self, instance_ids: impl IntoIterator<Item = &'a str>) {
        let active = instance_ids.into_iter().collect::<HashSet<_>>();
        if let Ok(mut probes) = self.account_profile_probes.lock() {
            probes.retain(|instance_id, _| active.contains(instance_id.as_str()));
        }
    }

    /// 执行换皮宿主内部的 `retain_recovered_instance_runtimes` 步骤。
    async fn retain_recovered_instance_runtimes<'a>(
        &self,
        instance_ids: impl IntoIterator<Item = &'a str>,
    ) {
        let active = instance_ids
            .into_iter()
            .map(str::to_owned)
            .collect::<HashSet<_>>();
        let mut runtime = self.runtime.lock().await;
        runtime.instances.retain(|instance_id, instance| {
            instance.task.is_some() || active.contains(instance_id)
        });
    }

    /// 执行换皮宿主内部的 `cached_codex_instance` 步骤。
    async fn cached_codex_instance(
        &self,
        resolved: ResolvedCodexInstance,
    ) -> Result<CodexInstance, AppError> {
        if resolved.debug_port.is_none() {
            return Ok(codex_instance_from_resolved(
                resolved,
                CodexRuntimeState::RunningWithoutCdp,
                None,
            ));
        }
        let probe = self.account_profile_probe_cell(&resolved.id)?;
        match probe
            .get_or_try_init(|| probe_resolved_account_profile(&resolved))
            .await
        {
            Ok(account_profile) => {
                let recovered_skin = account_profile
                    .as_ref()
                    .and_then(|profile| profile.active_skin.as_ref())
                    .and_then(|identity| self.resolve_recovered_skin(identity));
                let mut instance = codex_instance_from_resolved(
                    resolved,
                    CodexRuntimeState::Ready,
                    account_profile.clone(),
                );
                if let Some(descriptor) = recovered_skin {
                    instance.active_skin_name = Some(descriptor.name.clone());
                    instance.active_skin = Some(SkinReference {
                        source: descriptor.source,
                        id: descriptor.id.clone(),
                    });
                    let mut runtime = self.runtime.lock().await;
                    runtime
                        .instances
                        .entry(instance.id.clone())
                        .or_insert(InstanceRuntime {
                            active: Some(descriptor),
                            compatibility: None,
                            task: None,
                        });
                }
                Ok(instance)
            }
            Err(_) => Ok(codex_instance_from_resolved(
                resolved,
                CodexRuntimeState::RunningWithoutCdp,
                None,
            )),
        }
    }

    /// 执行换皮宿主内部的 `annotate_codex_instances` 步骤。
    async fn annotate_codex_instances(&self, instances: &mut [CodexInstance]) {
        let runtime = self.runtime.lock().await;
        for instance in instances {
            let instance_runtime = runtime.instances.get(&instance.id);
            instance.active_skin_name = displayed_active_skin_name(
                instance_runtime.and_then(|value| value.active.as_ref()),
            );
            instance.active_skin = instance_runtime
                .and_then(|value| value.active.as_ref())
                .map(|skin| SkinReference {
                    source: skin.source,
                    id: skin.id.clone(),
                });
        }
    }

    /// 执行换皮宿主内部的 `resolve_recovered_skin` 步骤。
    fn resolve_recovered_skin(&self, identity: &RecoveredSkinIdentity) -> Option<SkinDescriptor> {
        match identity {
            RecoveredSkinIdentity::Exact(reference) => {
                self.skin_directory(reference).ok().and_then(|directory| {
                    load_descriptor(&directory, &reference.id, reference.source).ok()
                })
            }
            RecoveredSkinIdentity::LegacyId(id) => exactly_one(
                [SkinSource::User, SkinSource::Builtin]
                    .into_iter()
                    .filter_map(|source| {
                        let reference = SkinReference {
                            source,
                            id: id.clone(),
                        };
                        self.skin_directory(&reference)
                            .ok()
                            .and_then(|directory| load_descriptor(&directory, id, source).ok())
                    }),
            ),
            RecoveredSkinIdentity::LegacyThemeCss(style_text) => exactly_one(
                [
                    (SkinSource::User, &self.user_root),
                    (SkinSource::Builtin, &self.builtin_root),
                ]
                .into_iter()
                .flat_map(|(source, root)| {
                    std::fs::read_dir(root)
                        .into_iter()
                        .flatten()
                        .filter_map(Result::ok)
                        .filter_map(move |entry| {
                            let id = entry.file_name().to_str()?.to_owned();
                            if !is_valid_skin_id(&id) {
                                return None;
                            }
                            let directory = entry.path();
                            let manifest = read_manifest(&directory).ok()?;
                            let SkinManifest::ThemeCss(theme) = manifest else {
                                return None;
                            };
                            let config =
                                read_theme_css_config(&directory, theme.appearance.as_ref())
                                    .ok()?;
                            let expected = format!("{THEME_RUNTIME_CSS}\n{}", config.css_text);
                            if expected != *style_text {
                                return None;
                            }
                            load_descriptor(&directory, &id, source).ok()
                        })
                }),
            ),
        }
    }

    /// 执行换皮宿主内部的 `codex_runtime_status` 步骤。
    pub async fn codex_runtime_status(&self) -> Result<CodexRuntimeStatus, AppError> {
        codex_runtime_status().await
    }

    /// 执行换皮宿主内部的 `launch_codex` 步骤。
    pub async fn launch_codex(&self) -> Result<CodexRuntimeStatus, AppError> {
        let _operation = self.operation.lock().await;
        let (_guard, mut cancel) = self.begin_codex_operation()?;
        launch_and_wait_for_cdp(&mut cancel).await
    }

    /// 执行换皮宿主内部的 `force_launch_codex` 步骤。
    pub async fn force_launch_codex(&self) -> Result<CodexRuntimeStatus, AppError> {
        let _operation = self.operation.lock().await;
        let (_guard, mut cancel) = self.begin_codex_operation()?;
        force_launch_and_wait_for_cdp(&mut cancel).await
    }

    /// 执行换皮宿主内部的 `restart_codex_instance` 步骤。
    pub async fn restart_codex_instance(
        &self,
        instance_id: &str,
    ) -> Result<CodexInstance, AppError> {
        let _operation = self.operation.lock().await;
        let (_guard, mut cancel) = self.begin_codex_operation()?;
        let selected = resolve_codex_instance(instance_id).await?;
        let all = platform_codex_processes()
            .await?
            .into_iter()
            .map(resolved_instance)
            .collect::<Vec<_>>();
        let port = available_debug_port(&all)?;
        run_cancellable(
            &mut cancel,
            restart_platform_codex_instance(&selected, port),
        )
        .await?;
        let endpoint = CdpEndpoint::new(port);
        let deadline = tokio::time::Instant::now() + CODEX_LAUNCH_TIMEOUT;
        while tokio::time::Instant::now() < deadline {
            match run_cancellable(&mut cancel, connect_browser(endpoint)).await {
                Ok((browser, task)) => {
                    task.abort();
                    drop(browser);
                    let instances = self.scanned_codex_instances().await?;
                    if let Some(instance) = restarted_instance_for_endpoint(instances, endpoint) {
                        return Ok(instance);
                    }
                }
                Err(error) if error.code == "skin.operation_cancelled" => return Err(error),
                Err(_) => {}
            }
            cancellable_sleep(&mut cancel, CODEX_PAGE_POLL_INTERVAL).await?;
        }
        Err(AppError::new(
            "skin.cdp_unavailable",
            "所选 Codex 实例未在 15 秒内使用调试端口重新启动。",
        ))
    }

    /// 执行换皮宿主内部的 `cancel_codex_operation` 步骤。
    pub fn cancel_codex_operation(&self) -> bool {
        let Ok(active) = self.codex_operation.lock() else {
            tracing::warn!("Codex 操作取消状态锁已损坏");
            return false;
        };
        active
            .as_ref()
            .is_some_and(|operation| operation.cancel.send(true).is_ok())
    }

    /// 执行换皮宿主内部的 `begin_codex_operation` 步骤。
    fn begin_codex_operation(
        &self,
    ) -> Result<(CodexOperationGuard<'_>, watch::Receiver<bool>), AppError> {
        let id = self.next_codex_operation_id.fetch_add(1, Ordering::Relaxed);
        let (cancel, receiver) = watch::channel(false);
        let mut active = self.codex_operation.lock().map_err(|_| {
            AppError::new(
                "skin.operation_state_failed",
                "无法准备 Codex 操作状态，请重启应用后重试。",
            )
        })?;
        *active = Some(CodexOperation { id, cancel });
        Ok((
            CodexOperationGuard {
                id,
                active: &self.codex_operation,
            },
            receiver,
        ))
    }
}
