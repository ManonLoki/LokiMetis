impl SkinService {
    #[cfg(test)]
    /// 执行换皮宿主内部的 `prepare_import_batch` 步骤。
    pub async fn prepare_import_batch(
        &self,
        sources: Vec<PathBuf>,
    ) -> Result<PreparedSkinImportBatch, AppError> {
        self.prepare_import_batch_with_progress(sources, |_| {})
            .await
    }

    /// 执行换皮宿主内部的 `prepare_import_batch_with_progress` 步骤。
    pub async fn prepare_import_batch_with_progress<F>(
        &self,
        sources: Vec<PathBuf>,
        progress: F,
    ) -> Result<PreparedSkinImportBatch, AppError>
    where
        F: Fn(SkinImportPreparationEvent) + Send + Sync + 'static,
    {
        loki_metis_core::validate_skin_import_batch_size(sources.len()).map_err(|_| {
            AppError::new(
                "skin.import_batch_limit",
                format!("每次最多选择 {MAX_IMPORT_BATCH_FILES} 个皮肤包。"),
            )
        })?;
        let _operation = self.operation.lock().await;
        self.cancel_current_import()?;
        let token = next_import_token();
        let cancelled = Arc::new(AtomicBool::new(false));
        *self
            .import_preparation
            .lock()
            .map_err(|_| import_state_error())? = Some(ActiveImportPreparation {
            token: token.clone(),
            cancelled: Arc::clone(&cancelled),
        });
        let progress: Arc<dyn Fn(SkinImportPreparationEvent) + Send + Sync> = Arc::new(progress);
        progress(SkinImportPreparationEvent::Started {
            token: token.clone(),
            total_files: sources.len(),
        });
        let user_root = self.user_root.clone();
        let worker_token = token.clone();
        let worker_cancelled = Arc::clone(&cancelled);
        let worker_progress = Arc::clone(&progress);
        let worker_result = tokio::task::spawn_blocking(move || {
            prepare_import_batch_in_worker(
                &user_root,
                sources,
                worker_token,
                &worker_cancelled,
                &worker_progress,
            )
        })
        .await
        .map_err(|_| AppError::new("skin.import_failed", "皮肤包预检任务意外中断。"));
        let prepared = match worker_result {
            Ok(Ok(prepared)) => prepared,
            Ok(Err(error)) | Err(error) => {
                self.finish_import_preparation(&token)?;
                return Err(error);
            }
        };
        if cancelled.load(Ordering::Acquire) {
            let _ = std::fs::remove_dir_all(&prepared.staging_root);
            self.finish_import_preparation(&token)?;
            return Err(import_cancelled_error());
        }
        let response = prepared.response();
        let staging_root = prepared.staging_root.clone();
        let mut active = match self.import_preparation.lock() {
            Ok(active) => active,
            Err(_) => {
                let _ = std::fs::remove_dir_all(&staging_root);
                return Err(import_state_error());
            }
        };
        if !active.as_ref().is_some_and(|active| active.token == token)
            || cancelled.load(Ordering::Acquire)
        {
            let _ = std::fs::remove_dir_all(&staging_root);
            if active.as_ref().is_some_and(|active| active.token == token) {
                *active = None;
            }
            return Err(import_cancelled_error());
        }
        let mut state = match self.pending_import.lock() {
            Ok(state) => state,
            Err(_) => {
                let _ = std::fs::remove_dir_all(&staging_root);
                *active = None;
                return Err(import_state_error());
            }
        };
        if state.is_some() {
            let _ = std::fs::remove_dir_all(&staging_root);
            *active = None;
            return Err(import_state_error());
        }
        *state = Some(prepared);
        *active = None;
        Ok(response)
    }

    /// 执行换皮宿主内部的 `commit_import_batch` 步骤。
    pub async fn commit_import_batch(
        &self,
        token: &str,
        selected_item_ids: &[String],
        allow_third_party_code: bool,
    ) -> Result<BatchImportResult, AppError> {
        validate_import_token(token)?;
        if selected_item_ids.is_empty() || selected_item_ids.len() > MAX_IMPORT_BATCH_FILES {
            return Err(AppError::new(
                "skin.import_selection_invalid",
                "请至少选择一个有效皮肤后再导入。",
            ));
        }
        let selected = selected_item_ids.iter().cloned().collect::<HashSet<_>>();
        if selected.len() != selected_item_ids.len() {
            return Err(AppError::new(
                "skin.import_selection_invalid",
                "导入选择中包含重复项目，请重新审核。",
            ));
        }
        let _operation = self.operation.lock().await;
        {
            let pending = self
                .pending_import
                .lock()
                .map_err(|_| import_state_error())?;
            let pending = pending
                .as_ref()
                .filter(|pending| pending.token == token)
                .ok_or_else(|| {
                    AppError::new(
                        "skin.import_expired",
                        "待安装的皮肤包已失效，请重新选择 ZIP。",
                    )
                })?;
            if selected
                .iter()
                .any(|item_id| !pending.items.iter().any(|item| &item.item_id == item_id))
            {
                return Err(AppError::new(
                    "skin.import_selection_invalid",
                    "导入选择包含已失效的项目，请重新选择 ZIP。",
                ));
            }
            for item in pending
                .items
                .iter()
                .filter(|item| selected.contains(&item.item_id))
            {
                validate_code_execution_consent(
                    item.candidate.package_type,
                    allow_third_party_code,
                )?;
            }
        }
        let pending = self.take_pending_import(token)?;
        let mut installed = Vec::new();
        let mut failed = Vec::new();
        for item in pending.items {
            if !selected.contains(&item.item_id) {
                continue;
            }
            let destination = self.user_root.join(&item.candidate.id);
            let move_result = match path_is_occupied(&destination) {
                Ok(true) => Err(AppError::new(
                    "skin.import_conflict",
                    "安装位置刚刚被占用，未覆盖既有皮肤。",
                )),
                Ok(false) => std::fs::rename(&item.staging, &destination)
                    .map_err(|_| AppError::new("skin.import_failed", "无法保存已校验的用户皮肤。")),
                Err(error) => Err(error),
            };
            match move_result {
                Ok(()) => installed.push(item.candidate),
                Err(error) => failed.push(FailedSkinImport {
                    item_id: item.item_id,
                    archive_name: item.archive_name,
                    skin_name: item.candidate.name,
                    code: error.code,
                    message: error.message,
                    details: error.details,
                }),
            }
        }
        if pending.staging_root.exists() {
            let _ = std::fs::remove_dir_all(&pending.staging_root);
        }
        if !installed.is_empty() {
            self.invalidate_catalog()?;
        }
        Ok(BatchImportResult {
            installed,
            failed,
            skipped_count: pending.skipped.len(),
        })
    }

    /// 执行换皮宿主内部的 `cancel_import` 步骤。
    pub fn cancel_import(&self, token: &str) -> Result<(), AppError> {
        validate_import_token(token)?;
        if let Some(active) = self
            .import_preparation
            .lock()
            .map_err(|_| import_state_error())?
            .as_ref()
            .filter(|active| active.token == token)
        {
            active.cancelled.store(true, Ordering::Release);
        }
        let pending = {
            let mut state = self
                .pending_import
                .lock()
                .map_err(|_| import_state_error())?;
            if state.as_ref().is_some_and(|pending| pending.token == token) {
                state.take()
            } else {
                None
            }
        };
        if let Some(pending) = pending {
            let _ = std::fs::remove_dir_all(pending.staging_root);
        }
        Ok(())
    }

    /// 执行换皮宿主内部的 `finish_import_preparation` 步骤。
    fn finish_import_preparation(&self, token: &str) -> Result<(), AppError> {
        let mut active = self
            .import_preparation
            .lock()
            .map_err(|_| import_state_error())?;
        if active.as_ref().is_some_and(|active| active.token == token) {
            *active = None;
        }
        Ok(())
    }

    #[cfg(test)]
    /// 执行换皮宿主内部的 `prepare_test_import_batch` 步骤。
    fn prepare_test_import_batch(
        &self,
        source: &Path,
    ) -> Result<PreparedSkinImportBatch, AppError> {
        self.cancel_current_import()?;
        let progress: Arc<dyn Fn(SkinImportPreparationEvent) + Send + Sync> = Arc::new(|_| {});
        let pending = prepare_import_batch_in_worker(
            &self.user_root,
            vec![source.to_path_buf()],
            next_import_token(),
            &AtomicBool::new(false),
            &progress,
        )?;
        let response = pending.response();
        *self
            .pending_import
            .lock()
            .map_err(|_| import_state_error())? = Some(pending);
        Ok(response)
    }

    #[cfg(test)]
    /// 执行换皮宿主内部的 `commit_test_import_batch` 步骤。
    async fn commit_test_import_batch(&self, token: &str) -> Result<SkinDescriptor, AppError> {
        let item_id = self
            .pending_import
            .lock()
            .map_err(|_| import_state_error())?
            .as_ref()
            .and_then(|pending| pending.items.first())
            .map(|item| item.item_id.clone())
            .ok_or_else(import_state_error)?;
        let result = self.commit_import_batch(token, &[item_id], true).await?;
        result.installed.into_iter().next().ok_or_else(|| {
            let failure = result.failed.into_iter().next();
            AppError::with_details(
                failure
                    .as_ref()
                    .map_or("skin.import_failed", |item| item.code),
                failure
                    .as_ref()
                    .map_or("皮肤导入失败。".to_owned(), |item| {
                        item.message.clone()
                    }),
                failure.map_or_else(Vec::new, |item| item.details),
            )
        })
    }

    /// 执行换皮宿主内部的 `delete` 步骤。
    pub async fn delete(&self, skin: &SkinReference) -> Result<(), AppError> {
        let result = self.delete_many(std::slice::from_ref(skin)).await?;
        if result.deleted.len() == 1 {
            return Ok(());
        }
        let failure = result
            .failed
            .into_iter()
            .next()
            .ok_or_else(|| AppError::new("skin.delete_failed", "无法删除皮肤资源。"))?;
        Err(AppError::new(failure.code, failure.message))
    }

    /// 执行换皮宿主内部的 `delete_many` 步骤。
    pub async fn delete_many(
        &self,
        skins: &[SkinReference],
    ) -> Result<BatchDeleteResult, AppError> {
        validate_delete_batch(skins)?;
        let _operation = self.operation.lock().await;
        let runtime = self.runtime.lock().await;
        let active = runtime
            .instances
            .values()
            .filter_map(|instance| instance.active.as_ref())
            .map(|skin| (skin.source, skin.id.clone()))
            .collect::<HashSet<_>>();
        drop(runtime);

        let mut deleted = Vec::with_capacity(skins.len());
        let mut failed = Vec::new();
        for skin in skins {
            if active.contains(&(skin.source, skin.id.clone())) {
                failed.push(FailedSkinDelete {
                    skin: skin.clone(),
                    code: "skin.in_use",
                    message: "该皮肤正在使用，请先停用后再删除。".into(),
                });
                continue;
            }
            let result = self.skin_directory(skin).and_then(|directory| {
                std::fs::remove_dir_all(directory)
                    .map_err(|_| AppError::new("skin.delete_failed", "无法删除皮肤资源。"))
            });
            match result {
                Ok(()) => deleted.push(skin.clone()),
                Err(error) => failed.push(FailedSkinDelete {
                    skin: skin.clone(),
                    code: error.code,
                    message: error.message,
                }),
            }
        }
        if !deleted.is_empty() {
            self.invalidate_catalog()?;
        }
        Ok(BatchDeleteResult { deleted, failed })
    }

    /// 执行换皮宿主内部的 `open_directory` 步骤。
    pub async fn open_directory(&self, skin: &SkinReference) -> Result<(), AppError> {
        validate_skin_reference(skin)?;
        if skin.source == SkinSource::Builtin {
            return Err(AppError::new(
                "skin.builtin_read_only",
                "内置皮肤没有可打开的用户目录。",
            ));
        }
        let directory = self.skin_directory(skin)?;
        open_in_file_manager(&directory).await
    }

    /// 执行换皮宿主内部的 `take_pending_import` 步骤。
    fn take_pending_import(&self, token: &str) -> Result<PendingImportBatch, AppError> {
        let mut state = self
            .pending_import
            .lock()
            .map_err(|_| import_state_error())?;
        if state.as_ref().is_some_and(|pending| pending.token == token) {
            return state.take().ok_or_else(import_state_error);
        }
        Err(AppError::new(
            "skin.import_expired",
            "待安装的皮肤包已失效，请重新选择 ZIP。",
        ))
    }

    /// 执行换皮宿主内部的 `cancel_current_import` 步骤。
    fn cancel_current_import(&self) -> Result<(), AppError> {
        let pending = self
            .pending_import
            .lock()
            .map_err(|_| import_state_error())?
            .take();
        if let Some(pending) = pending {
            let _ = std::fs::remove_dir_all(pending.staging_root);
        }
        Ok(())
    }

    /// 执行换皮宿主内部的 `cleanup_transient_directories` 步骤。
    fn cleanup_transient_directories(&self) -> Result<(), AppError> {
        for entry in std::fs::read_dir(&self.user_root)
            .map_err(|_| AppError::new("skin.library_unavailable", "无法读取用户皮肤资源库。"))?
            .filter_map(Result::ok)
        {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name.starts_with(".import-") || name.starts_with(".create-") {
                let _ = std::fs::remove_dir_all(entry.path());
                continue;
            }
            if name.starts_with(".backup-") {
                let Ok(manifest) = read_manifest(&entry.path()) else {
                    let _ = std::fs::remove_dir_all(entry.path());
                    continue;
                };
                if !is_valid_skin_id(manifest.id()) {
                    let _ = std::fs::remove_dir_all(entry.path());
                    continue;
                }
                if load_descriptor(&entry.path(), manifest.id(), SkinSource::User).is_err() {
                    let _ = std::fs::remove_dir_all(entry.path());
                    continue;
                }
                let destination = self.user_root.join(manifest.id());
                if destination.exists() {
                    let _ = std::fs::remove_dir_all(entry.path());
                } else {
                    std::fs::rename(entry.path(), destination).map_err(|_| {
                        AppError::new("skin.library_unavailable", "无法恢复中断的用户皮肤替换。")
                    })?;
                }
            }
        }
        Ok(())
    }

    /// 执行换皮宿主内部的 `skin_directory` 步骤。
    fn skin_directory(&self, skin: &SkinReference) -> Result<PathBuf, AppError> {
        validate_skin_reference(skin)?;
        let directory = match skin.source {
            SkinSource::Builtin => self.builtin_root.join(&skin.id),
            SkinSource::User => self.user_root.join(&skin.id),
        };
        let metadata = std::fs::symlink_metadata(&directory)
            .map_err(|_| AppError::new("skin.not_found", "所选皮肤资源不存在。"))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(AppError::new("skin.not_found", "所选皮肤资源不存在。"));
        }
        load_descriptor(&directory, &skin.id, skin.source)?;
        Ok(directory)
    }
}
