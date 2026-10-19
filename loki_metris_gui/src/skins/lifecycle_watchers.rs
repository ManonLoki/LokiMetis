// 本文件是 lifecycle.rs 的分片（第 2/3 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：取得 watcher 回收器短临界区；即使测试污染锁，也不能遗失真实任务句柄。

impl SkinService {
    /// 取得 watcher 回收器短临界区；即使测试污染锁，也不能遗失真实任务句柄。
    pub(super) fn lock_watch_task_reaper(&self) -> std::sync::MutexGuard<'_, WatchTaskReaper> {
        self.watch_task_reaper
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// 保存未到终态的原 JoinHandle，或保存已到终态但尚未执行的端点清理。
    pub(super) fn retain_watch_task(&self, task: RetainedWatchTask) {
        self.lock_watch_task_reaper().retained.push(task);
    }

    /// 在同一短锁中复核 shutdown 门禁并登记需要在退出前完成的活动。
    pub(super) fn register_shutdown_participant(
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

    /// 登记已构造但尚未提交到运行态的 watcher，使退出能取消并等待其完成。
    pub(super) fn register_active_watch_task(
        &self,
        cancel: watch::Sender<bool>,
    ) -> Result<ActiveWatchRegistration, AppError> {
        self.register_shutdown_participant(cancel)
    }

    /// 等待全部已登记 watcher 与安装事务到终态；所有调用共享首次 shutdown 的最终截止。
    async fn wait_for_active_shutdown_participants_until(
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
    pub(super) fn ensure_watch_runtime_open(&self) -> Result<(), AppError> {
        if self.lock_watch_task_reaper().shutting_down {
            Err(operation_cancelled())
        } else {
            Ok(())
        }
    }

    /// 返回后续安装/停用操作需要优先回收的任务数。
    #[cfg(test)]
    pub(super) fn retained_watch_task_count(&self) -> usize {
        self.lock_watch_task_reaper().retained.len()
    }

    /// 为正常操作建立一次完整而有界的 watcher 回收轮次。
    pub(super) async fn reap_watch_tasks(&self) -> Result<usize, AppError> {
        reap_finished_handler_tasks();
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
    pub(super) async fn stop_watch_tasks(&self, tasks: Vec<WatchTask>) -> Result<usize, AppError> {
        reap_finished_handler_tasks();
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

    /// 应用关闭后排队的宿主操作也必须在锁后拒绝，不新建未取消通道。
    pub(crate) fn close_host_admission(&self) {
        let cancellations = {
            let mut state = self.lock_watch_task_reaper();
            state.shutting_down = true;
            state.active.values().cloned().collect::<Vec<_>>()
        };
        for cancel in cancellations {
            let _ = cancel.send(true);
        }
        let _ = self.cancel_codex_operation();
    }
    /// 应用退出时关闭 watcher 登记、取消在途安装，并在首次确定的共同截止内回收。
    pub(crate) async fn shutdown(&self) {
        begin_handler_task_shutdown();
        let mut cleanup = remove_from_endpoint;
        if let Err(error) = self
            .shutdown_with_budget_cleanup_and_tail(
                WatchStopBudget::production(),
                &mut cleanup,
                true,
            )
            .await
        {
            tracing::error!(
                error_code = error.code,
                "skin watcher shutdown did not fully complete before its deadline"
            );
        }
        close_handler_task_reservations();
        let deadline = self
            .lock_watch_task_reaper()
            .shutdown_deadline
            .unwrap_or_else(tokio::time::Instant::now);
        if let Err(error) = reap_handler_tasks_until(deadline).await {
            tracing::error!(
                error_code = error.code,
                "skin handler shutdown did not reach terminal state"
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

    /// 生产退出在同一总预算内给最终handler abort与终态确认保留尾部。
    pub(super) async fn shutdown_with_budget_cleanup_and_tail<Cleanup, CleanupFuture>(
        &self,
        budget: WatchStopBudget,
        cleanup: &mut Cleanup,
        reserve_handlers: bool,
    ) -> Result<usize, AppError>
    where
        Cleanup: FnMut(SkinHostKind, CdpEndpoint) -> CleanupFuture,
        CleanupFuture: Future<Output = Result<usize, AppError>>,
    {
        let (cooperative_deadline, final_deadline, active_cancellations) = {
            let now = tokio::time::Instant::now();
            let mut reaper = self.lock_watch_task_reaper();
            reaper.shutting_down = true;
            let overall_deadline = *reaper.shutdown_deadline.get_or_insert(now + budget.total);
            let tail = if reserve_handlers {
                budget.abort_reap.min(budget.total / 2)
            } else {
                Duration::ZERO
            };
            let final_deadline = overall_deadline
                .checked_sub(tail)
                .unwrap_or(overall_deadline);
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
        if let Err(error) = self
            .wait_for_active_shutdown_participants_until(final_deadline)
            .await
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
        if let Err(error) = reap_handler_tasks_until(final_deadline).await
            && first_error.is_none()
        {
            first_error = Some(error);
        }
        first_error.map_or(Ok(affected_pages), Err)
    }

    /// 同一轮所有任务先收到合作取消，再共享合作、abort 确认与 endpoint cleanup 总截止。
    pub(super) async fn stop_watch_tasks_until<Cleanup, CleanupFuture>(
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
    pub(super) async fn stop_watch_task<Cleanup, CleanupFuture>(
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
}
