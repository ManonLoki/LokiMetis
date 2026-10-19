// 本文件是 lifecycle.rs 的分片（第 3/3 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：监视任务回收、服务释放时的取消与移交、皮肤提示词路径校验，以及皮肤 ID、删除选择与导入 token 的校验。

impl SkinService {
    /// 回收上一轮因最终期限耗尽而保存的任务；锁内只移交所有权，不跨 await 持锁。
    pub(super) async fn reap_retained_watch_tasks_until<Cleanup, CleanupFuture>(
        &self,
        _cooperative_deadline: tokio::time::Instant,
        final_deadline: tokio::time::Instant,
        abort_reap: Duration,
        cleanup: &mut Cleanup,
    ) -> Result<usize, AppError>
    where
        Cleanup: FnMut(SkinHostKind, CdpEndpoint) -> CleanupFuture,
        CleanupFuture: Future<Output = Result<usize, AppError>>,
    {
        let retained_count = self.lock_watch_task_reaper().retained.len();
        let mut affected_pages = 0_usize;
        let mut first_error = None;

        for _ in 0..retained_count {
            let Some(task) = self.lock_watch_task_reaper().retained.pop() else {
                break;
            };
            let mut task = RetainedWatchTaskGuard::new(task);
            if task.task_mut().join.is_some() {
                task.abort();
                let abort_deadline = (tokio::time::Instant::now() + abort_reap).min(final_deadline);
                match wait_for_watch_task_until(task.task_mut(), abort_deadline).await {
                    Some(Ok(Ok(affected))) => {
                        task.clear_finished_handles();
                        task.complete();
                        affected_pages = affected_pages.saturating_add(affected);
                        continue;
                    }
                    Some(result) => {
                        task.clear_finished_handles();
                        log_aborted_watch_join_result(result);
                    }
                    None => {
                        task.retain_in_service(self);
                        if first_error.is_none() {
                            first_error = Some(watch_stop_timeout_error());
                        }
                        continue;
                    }
                }
            }

            let host = task.task_mut().host;
            let endpoint = task.task_mut().endpoint;
            match cleanup_endpoint_until(cleanup, host, endpoint, final_deadline).await {
                EndpointCleanupOutcome::Completed(Ok(affected)) => {
                    task.complete();
                    affected_pages = affected_pages.saturating_add(affected);
                }
                EndpointCleanupOutcome::Completed(Err(error)) => {
                    task.retain_in_service(self);
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
                EndpointCleanupOutcome::DeadlineElapsed => {
                    task.retain_in_service(self);
                    if first_error.is_none() {
                        first_error = Some(watch_stop_timeout_error());
                    }
                }
            }
        }

        first_error.map_or(Ok(affected_pages), Err)
    }
}


impl Drop for SkinService {
    /// 服务释放时取消在途操作与全部页面监视任务，并把未确认终态的任务交给进程级 owner。
    fn drop(&mut self) {
        let _ = self.cancel_codex_operation();
        let retained = {
            let mut reaper = self
                .watch_task_reaper
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            reaper.shutting_down = true;
            reaper.service_dropped = true;
            for cancel in reaper.active.values() {
                let _ = cancel.send(true);
            }
            std::mem::take(&mut reaper.retained)
        };
        for task in retained {
            retain_process_watch_task(task);
        }
        for instance in self.runtime.get_mut().instances.values_mut() {
            if let Some(task) = instance.task.as_mut() {
                let _ = task.cancel.send(true);
                task.abort();
            }
        }
        if let Ok(active) = self.import_preparation.get_mut()
            && let Some(active) = active.take() {
                active.cancelled.store(true, Ordering::Release);
            }
        if let Ok(state) = self.pending_import.get_mut()
            && let Some(pending) = state.take() {
                let _ = std::fs::remove_dir_all(pending.staging_root);
            }
    }
}

/// 在共同截止前等待 watcher 终态；超时保留原 JoinHandle 给调用方继续持有。
async fn wait_for_watch_task_until(
    task: &mut RetainedWatchTask,
    deadline: tokio::time::Instant,
) -> Option<Result<Result<usize, AppError>, tokio::task::JoinError>> {
    let join = task.join.as_mut()?;
    if join.is_finished() {
        Some(join.await)
    } else {
        tokio::time::timeout_at(deadline, join).await.ok()
    }
}

/// endpoint cleanup 自身虽有 CDP 时限，外层仍以 watcher 的共同总截止为硬上界。
async fn cleanup_endpoint_until<Cleanup, CleanupFuture>(
    cleanup: &mut Cleanup,
    host: SkinHostKind,
    endpoint: CdpEndpoint,
    deadline: tokio::time::Instant,
) -> EndpointCleanupOutcome
where
    Cleanup: FnMut(SkinHostKind, CdpEndpoint) -> CleanupFuture,
    CleanupFuture: Future<Output = Result<usize, AppError>>,
{
    if tokio::time::Instant::now() >= deadline {
        return EndpointCleanupOutcome::DeadlineElapsed;
    }
    match tokio::time::timeout_at(deadline, cleanup(host, endpoint)).await {
        Ok(result) => EndpointCleanupOutcome::Completed(result),
        Err(_) => EndpointCleanupOutcome::DeadlineElapsed,
    }
}

/// 记录 abort 后 watcher 的真实终态，同时把正常取消视为已回收。
fn log_aborted_watch_join_result(result: Result<Result<usize, AppError>, tokio::task::JoinError>) {
    match result {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => tracing::warn!(
            error_code = error.code,
            "aborted skin watcher returned an error before termination"
        ),
        Err(error) if error.is_cancelled() => {}
        Err(error) => tracing::warn!(%error, "aborted skin watcher failed while terminating"),
    }
}

/// 返回 watcher 或端点清理未在共同截止内完成的稳定错误。
pub(super) fn watch_stop_timeout_error() -> AppError {
    AppError::new(
        "skin.task_stop_timeout",
        "皮肤后台任务停止超时，残留页面清理未完成，请稍后重试。",
    )
}

/// 生成皮肤提示词不可用的稳定错误。
pub(super) fn prompt_unavailable(message: impl Into<String>) -> AppError {
    AppError::new("skin.prompt_unavailable", message)
}

/// 要求提示词引用的目录是绝对路径下的真实目录（非链接）。
pub(super) fn validate_prompt_directory(path: &Path, message: &str) -> Result<(), AppError> {
    if !path.is_absolute() {
        return Err(prompt_unavailable(message));
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|_| prompt_unavailable(message))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(prompt_unavailable(message));
    }
    Ok(())
}

/// 校验安装包内置 Skill 中的文件：相对路径只含普通段，沿途不经过链接，且规范化后仍位于 Skill 根目录内。
pub(super) fn validate_bundled_skill_file(
    bundled_skill_root: &Path,
    relative: &Path,
) -> Result<PathBuf, AppError> {
    const MESSAGE: &str = "安装包中的皮肤生成 Skill 不完整，请重新安装LokiMetris。";
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

/// 把路径编码为 JSON 字符串供写入提示词；非 UTF-8 或含控制字符的路径拒绝。
pub(super) fn prompt_json_path(path: &Path) -> Result<String, AppError> {
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

/// 皮肤 ID 是否合法，规则由 core 决定。
pub(crate) fn is_valid_skin_id(value: &str) -> bool {
    loki_metris_core::is_valid_skin_id(value)
}

/// 校验用户皮肤 ID，规则由 core 决定。
pub(super) fn validate_skin_id(value: &str) -> Result<(), AppError> {
    loki_metris_core::validate_skin_reference(&SkinReference {
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

/// 校验皮肤引用，并把 core 规则错误映射为界面错误。
pub(super) fn validate_skin_reference(skin: &SkinReference) -> Result<(), AppError> {
    loki_metris_core::validate_skin_reference(skin).map_err(|_| {
        AppError::new(
            "skin.id_invalid",
            "皮肤标识只能包含小写字母、数字、连字符或下划线。",
        )
    })
}

/// 把核心层的自由脚本信任缺失映射为稳定 GUI 错误，不暗示格式校验能证明安全。
pub(super) fn third_party_code_consent_required_error() -> AppError {
    AppError::new(
        "skin.third_party_code_consent_required",
        "兼容皮肤包含 renderer-inject.js，并会在目标宿主中执行第三方代码。只有明确确认信任来源后才能继续；格式校验不能证明脚本安全。",
    )
}

/// 返回不暴露端口、PID 或路径的稳定 WorkBuddy 端点归属检查错误。
pub(super) fn workbuddy_owner_inspection_failed() -> AppError {
    AppError::new(
        "skin.workbuddy_cdp_owner_inspection_failed",
        "无法验证 WorkBuddy 调试端口所属进程，未应用皮肤。",
    )
}

/// 校验批量删除选择（数量、只读内置、重复与 ID），把 core 规则错误映射为界面错误。
pub(super) fn validate_delete_batch(skins: &[SkinReference]) -> Result<(), AppError> {
    loki_metris_core::validate_skin_delete_batch(skins).map_err(|error| match error {
        SkinRuleError::InvalidBatchSize => AppError::new(
            "skin.delete_selection_invalid",
            format!("每次必须选择 1 至 {MAX_DELETE_BATCH_ITEMS} 个用户皮肤。"),
        ),
        SkinRuleError::BuiltinReadOnly => {
            AppError::new("skin.builtin_read_only", "内置皮肤不能删除。")
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
        | SkinRuleError::InvalidThemeImageName
        | SkinRuleError::ThirdPartyCodeConsentRequired => {
            AppError::new("skin.delete_selection_invalid", "皮肤删除请求无效。")
        }
    })
}

/// 导入 token 必须是 64 字符以内的数字与连字符，否则视为已过期。
pub(super) fn validate_import_token(token: &str) -> Result<(), AppError> {
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

/// 旧版清单缺少作者时的默认值。
pub(super) fn unknown_author() -> String {
    "未知作者".into()
}
