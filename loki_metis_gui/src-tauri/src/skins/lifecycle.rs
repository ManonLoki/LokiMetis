/// 强制中止 watcher 后留给 Tokio 确认 future 已到终态的预算；端点清理继续受同一总截止约束。
const WATCH_ABORT_REAP_TIMEOUT: Duration = Duration::from_secs(1);

/// 描述 watcher 合作停止、总截止和 abort 终态确认的预算。
#[derive(Clone, Copy)]
struct WatchStopBudget {
    cooperative: Duration,
    total: Duration,
    abort_reap: Duration,
}

impl WatchStopBudget {
    /// 从产品常量构造生产环境预算。
    fn production() -> Self {
        Self {
            cooperative: WATCH_STOP_TIMEOUT,
            // 保留原有合作停止与 CDP 清理预算，但二者现在共享一个不可重置的总截止。
            total: WATCH_STOP_TIMEOUT.saturating_add(CDP_CLEANUP_TIMEOUT),
            abort_reap: WATCH_ABORT_REAP_TIMEOUT,
        }
    }

    /// 由同一开始时间派生不可重置的合作截止与最终截止。
    fn deadlines(
        self,
        started_at: tokio::time::Instant,
    ) -> (tokio::time::Instant, tokio::time::Instant) {
        let final_deadline = started_at + self.total;
        let cooperative_deadline = started_at + self.cooperative.min(self.total);
        (cooperative_deadline, final_deadline)
    }
}

/// 区分端点清理已返回（含显式错误）与外层共同截止耗尽。
enum EndpointCleanupOutcome {
    Completed(Result<usize, AppError>),
    DeadlineElapsed,
}

impl SkinService {
    /// 取得 watcher 回收器短临界区；即使测试污染锁，也不能遗失真实任务句柄。
    fn lock_watch_task_reaper(&self) -> std::sync::MutexGuard<'_, WatchTaskReaper> {
        self.watch_task_reaper
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// 保存未到终态的原 JoinHandle，或保存已到终态但尚未执行的端点清理。
    fn retain_watch_task(&self, task: RetainedWatchTask) {
        self.lock_watch_task_reaper().retained.push(task);
    }

    /// 在同一短锁中复核 shutdown 门禁并登记 watcher 取消发送端。
    fn register_active_watch_task(
        &self,
        cancel: watch::Sender<bool>,
    ) -> Result<ActiveWatchRegistration, AppError> {
        let mut state = self.lock_watch_task_reaper();
        if state.shutting_down || state.service_dropped {
            return Err(operation_cancelled());
        }
        let registration_id = state.next_registration_id;
        state.next_registration_id = state.next_registration_id.wrapping_add(1);
        state.active.insert(registration_id, cancel);
        Ok(ActiveWatchRegistration {
            registration_id: Some(registration_id),
            owner: Arc::clone(&self.watch_task_reaper),
        })
    }

    /// 等待全部已登记 watcher 到终态；所有调用共享首次 shutdown 的最终截止。
    async fn wait_for_active_watch_tasks_until(
        &self,
        deadline: tokio::time::Instant,
    ) -> Result<(), AppError> {
        loop {
            if self.lock_watch_task_reaper().active.is_empty() {
                return Ok(());
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(watch_stop_timeout_error());
            }
            tokio::time::sleep(Duration::from_millis(10).min(remaining)).await;
        }
    }

    /// 应用退出一旦开始，不允许在退出任务完成后又登记新的 watcher。
    fn ensure_watch_runtime_open(&self) -> Result<(), AppError> {
        if self.lock_watch_task_reaper().shutting_down {
            Err(operation_cancelled())
        } else {
            Ok(())
        }
    }

    /// 返回后续安装/停用操作需要优先回收的任务数。
    #[cfg(test)]
    fn retained_watch_task_count(&self) -> usize {
        self.lock_watch_task_reaper().retained.len()
    }

    /// 为正常操作建立一次完整而有界的 watcher 回收轮次。
    async fn reap_watch_tasks(&self) -> Result<usize, AppError> {
        let budget = WatchStopBudget::production();
        let (cooperative_deadline, final_deadline) = budget.deadlines(tokio::time::Instant::now());
        let mut cleanup = remove_from_endpoint;
        self.reap_retained_watch_tasks_until(
            cooperative_deadline,
            final_deadline,
            budget.abort_reap,
            &mut cleanup,
        )
        .await
    }

    /// 停止一组已从运行态摘除的 watcher，并优先回收上一轮保留的句柄。
    async fn stop_watch_tasks(&self, tasks: Vec<WatchTask>) -> Result<usize, AppError> {
        let budget = WatchStopBudget::production();
        let (cooperative_deadline, final_deadline) = budget.deadlines(tokio::time::Instant::now());
        let mut cleanup = remove_from_endpoint;
        self.stop_watch_tasks_until(
            tasks,
            cooperative_deadline,
            final_deadline,
            budget.abort_reap,
            &mut cleanup,
        )
        .await
    }

    /// 应用退出时关闭 watcher 登记、取消在途安装，并在首次确定的共同截止内回收。
    pub(crate) async fn shutdown(&self) {
        let mut cleanup = remove_from_endpoint;
        if let Err(error) = self
            .shutdown_with_budget_and_cleanup(WatchStopBudget::production(), &mut cleanup)
            .await
        {
            tracing::error!(
                error_code = error.code,
                "skin watcher shutdown did not fully complete before its deadline"
            );
        }
        #[cfg(unix)]
        if let Err(error) =
            process_reaper::shutdown_process_reaper(process_reaper::PROCESS_REAPER_SHUTDOWN_TIMEOUT)
                .await
        {
            tracing::error!(
                ?error,
                "skin launcher process reaper did not complete before its deadline"
            );
        }
    }

    /// 使用可注入预算和清理器关闭服务，供生产路径与生命周期回归共用。
    async fn shutdown_with_budget_and_cleanup<Cleanup, CleanupFuture>(
        &self,
        budget: WatchStopBudget,
        cleanup: &mut Cleanup,
    ) -> Result<usize, AppError>
    where
        Cleanup: FnMut(SkinHostKind, CdpEndpoint) -> CleanupFuture,
        CleanupFuture: Future<Output = Result<usize, AppError>>,
    {
        let (cooperative_deadline, final_deadline, active_cancellations) = {
            let now = tokio::time::Instant::now();
            let mut reaper = self.lock_watch_task_reaper();
            reaper.shutting_down = true;
            let final_deadline = *reaper.shutdown_deadline.get_or_insert(now + budget.total);
            let cleanup_tail = budget.total.saturating_sub(budget.cooperative);
            let cooperative_deadline = final_deadline
                .checked_sub(cleanup_tail)
                .unwrap_or(final_deadline);
            let active_cancellations = reaper.active.values().cloned().collect::<Vec<_>>();
            (cooperative_deadline, final_deadline, active_cancellations)
        };
        for cancel in active_cancellations {
            let _ = cancel.send(true);
        }
        let _ = self.cancel_codex_operation();

        // 退出门禁已在 reaper 内原子关闭；登记方持同一短锁复核，因此无需等待可能
        // 正在执行 CDP I/O 的 operation 锁，也不会在排空之后重新提交 watcher。
        let tasks = {
            let mut runtime = self.runtime.lock().await;
            runtime.last_targets.clear();
            runtime
                .instances
                .drain()
                .filter_map(|(_, instance)| instance.task)
                .collect::<Vec<_>>()
        };
        let mut affected_pages = 0_usize;
        let mut first_error = None;
        match self
            .stop_watch_tasks_until(
                tasks,
                cooperative_deadline,
                final_deadline,
                budget.abort_reap,
                cleanup,
            )
            .await
        {
            Ok(affected) => affected_pages = affected_pages.saturating_add(affected),
            Err(error) => first_error = Some(error),
        }
        if let Err(error) = self.wait_for_active_watch_tasks_until(final_deadline).await
            && first_error.is_none()
        {
            first_error = Some(error);
        }
        match self
            .reap_retained_watch_tasks_until(
                cooperative_deadline,
                final_deadline,
                budget.abort_reap,
                cleanup,
            )
            .await
        {
            Ok(affected) => affected_pages = affected_pages.saturating_add(affected),
            Err(error) if first_error.is_none() => first_error = Some(error),
            Err(_) => {}
        }
        first_error.map_or(Ok(affected_pages), Err)
    }

    /// 同一轮所有任务先收到合作取消，再共享合作、abort 确认与 endpoint cleanup 总截止。
    async fn stop_watch_tasks_until<Cleanup, CleanupFuture>(
        &self,
        tasks: Vec<WatchTask>,
        cooperative_deadline: tokio::time::Instant,
        final_deadline: tokio::time::Instant,
        abort_reap: Duration,
        cleanup: &mut Cleanup,
    ) -> Result<usize, AppError>
    where
        Cleanup: FnMut(SkinHostKind, CdpEndpoint) -> CleanupFuture,
        CleanupFuture: Future<Output = Result<usize, AppError>>,
    {
        for task in &tasks {
            let _ = task.cancel.send(true);
        }

        let mut affected_pages = 0_usize;
        let mut first_error = None;
        match self
            .reap_retained_watch_tasks_until(
                cooperative_deadline,
                final_deadline,
                abort_reap,
                cleanup,
            )
            .await
        {
            Ok(affected) => affected_pages = affected_pages.saturating_add(affected),
            Err(error) => first_error = Some(error),
        }
        for task in tasks {
            match self
                .stop_watch_task(
                    task,
                    cooperative_deadline,
                    final_deadline,
                    abort_reap,
                    cleanup,
                )
                .await
            {
                Ok(affected) => affected_pages = affected_pages.saturating_add(affected),
                Err(error) if first_error.is_none() => first_error = Some(error),
                Err(_) => {}
            }
        }
        first_error.map_or(Ok(affected_pages), Err)
    }

    /// 停止单个 watcher；任何未完成阶段都把剩余所有权移交回 service reaper。
    async fn stop_watch_task<Cleanup, CleanupFuture>(
        &self,
        task: WatchTask,
        cooperative_deadline: tokio::time::Instant,
        final_deadline: tokio::time::Instant,
        abort_reap: Duration,
        cleanup: &mut Cleanup,
    ) -> Result<usize, AppError>
    where
        Cleanup: FnMut(SkinHostKind, CdpEndpoint) -> CleanupFuture,
        CleanupFuture: Future<Output = Result<usize, AppError>>,
    {
        let host = task.host;
        let endpoint = task.endpoint;
        let mut task = RetainedWatchTaskGuard::new(task.into_retained());
        match wait_for_watch_task_until(task.task_mut(), cooperative_deadline).await {
            Some(Ok(Ok(affected_pages))) => {
                task.clear_finished_handles();
                task.complete();
                Ok(affected_pages)
            }
            Some(Ok(Err(error))) => {
                task.clear_finished_handles();
                tracing::warn!(
                    error_code = error.code,
                    "skin watcher failed; attempting endpoint cleanup"
                );
                match cleanup_endpoint_until(cleanup, host, endpoint, final_deadline).await {
                    EndpointCleanupOutcome::Completed(Ok(_)) => {
                        task.complete();
                        Err(error)
                    }
                    EndpointCleanupOutcome::Completed(Err(cleanup_error)) => {
                        tracing::warn!(
                            error_code = cleanup_error.code,
                            "skin watcher endpoint cleanup failed"
                        );
                        task.retain_in_service(self);
                        Err(error)
                    }
                    EndpointCleanupOutcome::DeadlineElapsed => {
                        task.retain_in_service(self);
                        Err(watch_stop_timeout_error())
                    }
                }
            }
            Some(Err(error)) => {
                task.clear_finished_handles();
                tracing::warn!(%error, "skin watcher JoinHandle failed");
                let task_error = AppError::new("skin.task_failed", "皮肤后台任务未能正常停止。");
                match cleanup_endpoint_until(cleanup, host, endpoint, final_deadline).await {
                    EndpointCleanupOutcome::Completed(Ok(_)) => {
                        task.complete();
                        Err(task_error)
                    }
                    EndpointCleanupOutcome::Completed(Err(cleanup_error)) => {
                        tracing::warn!(
                            error_code = cleanup_error.code,
                            "failed watcher endpoint cleanup did not complete"
                        );
                        task.retain_in_service(self);
                        Err(task_error)
                    }
                    EndpointCleanupOutcome::DeadlineElapsed => {
                        task.retain_in_service(self);
                        Err(watch_stop_timeout_error())
                    }
                }
            }
            None => {
                tracing::warn!("skin watcher cooperative stop timed out; aborting task");
                task.abort();
                let abort_deadline = (tokio::time::Instant::now() + abort_reap).min(final_deadline);
                match wait_for_watch_task_until(task.task_mut(), abort_deadline).await {
                    Some(Ok(Ok(affected_pages))) => {
                        task.clear_finished_handles();
                        task.complete();
                        Ok(affected_pages)
                    }
                    Some(result) => {
                        task.clear_finished_handles();
                        log_aborted_watch_join_result(result);
                        match cleanup_endpoint_until(cleanup, host, endpoint, final_deadline).await
                        {
                            EndpointCleanupOutcome::Completed(Ok(affected_pages)) => {
                                task.complete();
                                Ok(affected_pages)
                            }
                            EndpointCleanupOutcome::Completed(Err(error)) => {
                                tracing::warn!(
                                    error_code = error.code,
                                    "aborted watcher endpoint cleanup failed"
                                );
                                task.retain_in_service(self);
                                Err(watch_stop_timeout_error())
                            }
                            EndpointCleanupOutcome::DeadlineElapsed => {
                                task.retain_in_service(self);
                                Err(watch_stop_timeout_error())
                            }
                        }
                    }
                    None => {
                        // 不能 drop 尚未终止的 JoinHandle；后续操作或应用退出继续回收。
                        task.retain_in_service(self);
                        Err(watch_stop_timeout_error())
                    }
                }
            }
        }
    }

    /// 回收上一轮因最终期限耗尽而保存的任务；锁内只移交所有权，不跨 await 持锁。
    async fn reap_retained_watch_tasks_until<Cleanup, CleanupFuture>(
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
    /// 执行换皮宿主内部的 `drop` 步骤。
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
fn watch_stop_timeout_error() -> AppError {
    AppError::new(
        "skin.task_stop_timeout",
        "皮肤后台任务停止超时，残留页面清理未完成，请稍后重试。",
    )
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

/// 返回不暴露端口、PID 或路径的稳定 WorkBuddy 端点归属检查错误。
fn workbuddy_owner_inspection_failed() -> AppError {
    AppError::new(
        "skin.workbuddy_cdp_owner_inspection_failed",
        "无法验证 WorkBuddy 调试端口所属进程，未应用皮肤。",
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
