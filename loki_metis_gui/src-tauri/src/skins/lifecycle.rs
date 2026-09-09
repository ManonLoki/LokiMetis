impl Drop for SkinService {
    /// 执行换皮宿主内部的 `drop` 步骤。
    fn drop(&mut self) {
        if let Ok(active) = self.import_preparation.get_mut() {
            if let Some(active) = active.take() {
                active.cancelled.store(true, Ordering::Release);
            }
        }
        if let Ok(state) = self.pending_import.get_mut() {
            if let Some(pending) = state.take() {
                let _ = std::fs::remove_dir_all(pending.staging_root);
            }
        }
    }
}

/// 执行换皮宿主内部的 `stop_watch_task` 步骤。
async fn stop_watch_task(task: Option<WatchTask>) -> Result<usize, AppError> {
    let Some(mut task) = task else {
        return Ok(0);
    };
    let host = task.host;
    let endpoint = task.endpoint;
    let _ = task.cancel.send(true);
    let result = match tokio::time::timeout(WATCH_STOP_TIMEOUT, &mut task.join).await {
        Ok(Ok(Ok(affected_pages))) => Ok(affected_pages),
        Ok(Ok(Err(error))) => {
            tracing::warn!("皮肤后台任务已失败，尝试独立清理，错误码={}", error.code);
            task.handler_abort.abort();
            remove_from_endpoint(host, endpoint).await.or(Err(error))
        }
        Ok(Err(_)) => {
            task.handler_abort.abort();
            let error = AppError::new("skin.task_failed", "皮肤后台任务未能正常停止。");
            remove_from_endpoint(host, endpoint).await.or(Err(error))
        }
        Err(_) => {
            tracing::warn!("皮肤后台任务停止超时，执行有界强制清理");
            task.handler_abort.abort();
            task.join.abort();
            let _ = task.join.await;
            remove_from_endpoint(host, endpoint).await.map_err(|_| {
                AppError::new(
                    "skin.task_stop_timeout",
                    "皮肤后台任务停止超时，残留页面清理未完成，请稍后重试。",
                )
            })
        }
    };
    result
}

/// 停止一组已经从运行态摘除的监视任务；即使其中一个清理失败，也继续回收其余任务。
async fn stop_watch_tasks(tasks: Vec<WatchTask>) -> Result<usize, AppError> {
    let mut affected_pages = 0_usize;
    let mut first_error = None;
    for task in tasks {
        match stop_watch_task(Some(task)).await {
            Ok(affected) => affected_pages = affected_pages.saturating_add(affected),
            Err(error) if first_error.is_none() => first_error = Some(error),
            Err(_) => {}
        }
    }
    first_error.map_or(Ok(affected_pages), Err)
}

/// 执行换皮宿主内部的 `prompt_unavailable` 步骤。
fn prompt_unavailable(message: impl Into<String>) -> AppError {
    AppError::new("skin.prompt_unavailable", message)
}

/// 执行换皮宿主内部的 `validate_prompt_directory` 步骤。
fn validate_prompt_directory(path: &Path, message: &str) -> Result<(), AppError> {
    if !path.is_absolute() {
        return Err(prompt_unavailable(message));
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|_| prompt_unavailable(message))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(prompt_unavailable(message));
    }
    Ok(())
}

/// 执行换皮宿主内部的 `validate_bundled_skill_file` 步骤。
fn validate_bundled_skill_file(
    bundled_skill_root: &Path,
    relative: &Path,
) -> Result<PathBuf, AppError> {
    const MESSAGE: &str = "安装包中的皮肤生成 Skill 不完整，请重新安装LokiMetis。";
    validate_prompt_directory(bundled_skill_root, MESSAGE)?;
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(prompt_unavailable(MESSAGE));
    }

    let root = bundled_skill_root
        .canonicalize()
        .map_err(|_| prompt_unavailable(MESSAGE))?;
    let candidate = bundled_skill_root.join(relative);
    let mut current = bundled_skill_root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(segment) = component else {
            return Err(prompt_unavailable(MESSAGE));
        };
        current.push(segment);
        let metadata =
            std::fs::symlink_metadata(&current).map_err(|_| prompt_unavailable(MESSAGE))?;
        if metadata.file_type().is_symlink() {
            return Err(prompt_unavailable(MESSAGE));
        }
    }
    let metadata =
        std::fs::symlink_metadata(&candidate).map_err(|_| prompt_unavailable(MESSAGE))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(prompt_unavailable(MESSAGE));
    }
    let canonical = candidate
        .canonicalize()
        .map_err(|_| prompt_unavailable(MESSAGE))?;
    if !canonical.starts_with(root) {
        return Err(prompt_unavailable(MESSAGE));
    }
    Ok(candidate)
}

/// 执行换皮宿主内部的 `prompt_json_path` 步骤。
fn prompt_json_path(path: &Path) -> Result<String, AppError> {
    let value = path
        .to_str()
        .ok_or_else(|| prompt_unavailable("当前系统路径无法安全写入提示词，请联系应用维护者。"))?;
    if value.chars().any(char::is_control) {
        return Err(prompt_unavailable(
            "当前系统路径包含提示词无法安全表示的字符，请联系应用维护者。",
        ));
    }
    serde_json::to_string(value).map_err(|_| prompt_unavailable("暂时无法生成 Codex 主题提示词。"))
}

/// 执行换皮宿主内部的 `is_valid_skin_id` 步骤。
pub(crate) fn is_valid_skin_id(value: &str) -> bool {
    loki_metis_core::is_valid_skin_id(value)
}

/// 执行换皮宿主内部的 `validate_skin_id` 步骤。
fn validate_skin_id(value: &str) -> Result<(), AppError> {
    loki_metis_core::validate_skin_reference(&SkinReference {
        source: SkinSource::User,
        id: value.to_owned(),
    })
    .map_err(|_| {
        AppError::new(
            "skin.id_invalid",
            "皮肤标识只能包含小写字母、数字、连字符或下划线。",
        )
    })
}

/// 执行换皮宿主内部的 `validate_skin_reference` 步骤。
fn validate_skin_reference(skin: &SkinReference) -> Result<(), AppError> {
    loki_metis_core::validate_skin_reference(skin).map_err(|_| {
        AppError::new(
            "skin.id_invalid",
            "皮肤标识只能包含小写字母、数字、连字符或下划线。",
        )
    })
}

/// 把核心层的自由脚本信任缺失映射为稳定 GUI 错误，不暗示格式校验能证明安全。
fn third_party_code_consent_required_error() -> AppError {
    AppError::new(
        "skin.third_party_code_consent_required",
        "兼容皮肤包含 renderer-inject.js，并会在目标宿主中执行第三方代码。只有明确确认信任来源后才能继续；格式校验不能证明脚本安全。",
    )
}

/// 执行换皮宿主内部的 `validate_delete_batch` 步骤。
fn validate_delete_batch(skins: &[SkinReference]) -> Result<(), AppError> {
    loki_metis_core::validate_skin_delete_batch(skins).map_err(|error| match error {
        SkinRuleError::InvalidBatchSize => AppError::new(
            "skin.delete_selection_invalid",
            format!("每次必须选择 1 至 {MAX_DELETE_BATCH_ITEMS} 个用户皮肤。"),
        ),
        SkinRuleError::BuiltinReadOnly => {
            AppError::new(
                "skin.builtin_read_only",
                "内置皮肤不能删除。",
            )
        }
        SkinRuleError::DuplicateReference => AppError::new(
                "skin.delete_selection_duplicate",
                "批量删除列表包含重复皮肤。",
        ),
        SkinRuleError::InvalidId => AppError::new(
            "skin.id_invalid",
            "皮肤标识只能包含小写字母、数字、连字符或下划线。",
        ),
        SkinRuleError::InvalidCreatorText
        | SkinRuleError::InvalidThemeMetadata
        | SkinRuleError::InvalidColorModes
        | SkinRuleError::InvalidThemeImageName => {
            AppError::new("skin.delete_selection_invalid", "皮肤删除请求无效。")
        }
        SkinRuleError::ThirdPartyCodeConsentRequired => {
            third_party_code_consent_required_error()
        }
    })
}

/// 执行换皮宿主内部的 `validate_import_token` 步骤。
fn validate_import_token(token: &str) -> Result<(), AppError> {
    if !token.is_empty()
        && token.len() <= 64
        && token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'-')
    {
        Ok(())
    } else {
        Err(AppError::new(
            "skin.import_expired",
            "待安装的皮肤包已失效，请重新选择 ZIP。",
        ))
    }
}

/// 执行换皮宿主内部的 `unknown_author` 步骤。
fn unknown_author() -> String {
    "未知作者".into()
}
