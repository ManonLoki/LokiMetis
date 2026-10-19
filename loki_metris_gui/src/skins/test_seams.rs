//! 仅测试编译的 `SkinService` 接缝；生产路径不调用，集中于此以免测试形状渗入生产文件。

use super::*;

impl SkinService {
    /// 刷新用户皮肤库指纹，返回目录相对上次是否发生变化。
    pub fn catalog_changed(&self) -> Result<bool, AppError> {
        std::fs::create_dir_all(&self.user_root)
            .map_err(|_| AppError::new("skin.library_unavailable", "无法准备用户皮肤资源库。"))?;
        let fingerprint = catalog_fingerprint(&self.builtin_root, &self.user_root)?;
        let catalog = self.catalog.read().map_err(|_| catalog_cache_error())?;
        Ok(match catalog.as_ref() {
            Some(cached) => cached.fingerprint != fingerprint,
            None => true,
        })
    }

    /// 删除单个皮肤；批量删除未恰好删除一项时视为失败。
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

    /// 关闭未开放调试通道的指定宿主后重新启动。
    pub async fn force_launch_host(
        &self,
        host: SkinHostKind,
    ) -> Result<CodexRuntimeStatus, AppError> {
        if host == SkinHostKind::WorkBuddy && !cfg!(target_os = "windows") {
            return Err(AppError::new(
                "skin.workbuddy_close_all_unsupported",
                "关闭全部 WorkBuddy 的恢复流程仅在 Windows 上可用。",
            ));
        }
        let _operation = self.operation.lock().await;
        let _runtime_mutation = self.begin_host_runtime_mutation(host);
        let (_completion, cancel_sender, mut cancel) = self.begin_install_completion()?;
        let _guard = self.bind_install_codex_operation(cancel_sender)?;
        let preferred_endpoint = self.verified_endpoint_hint(host)?;
        if host == SkinHostKind::WorkBuddy && cfg!(target_os = "windows") {
            let (_, endpoint) = self
                .recover_windows_workbuddy_target(&mut cancel, preferred_endpoint)
                .await?;
            self.remember_verified_endpoint(host, endpoint)?;
            return Ok(CodexRuntimeStatus::new(CodexRuntimeState::Ready));
        }
        let mut attempted_endpoint = None;
        let (status, endpoint) = force_launch_and_wait_for_cdp(
            host,
            &mut cancel,
            preferred_endpoint,
            &[],
            &mut attempted_endpoint,
        )
        .await?;
        if let Some(endpoint) = endpoint {
            self.remember_verified_endpoint(host, endpoint)?;
        }
        Ok(status)
    }

    /// 登记一次 Codex 宿主操作，返回操作守卫与取消信号接收端。
    pub(super) fn begin_codex_operation(
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

    /// 使用可注入预算和清理器关闭服务，供生产路径与生命周期回归共用。
    pub(super) async fn shutdown_with_budget_and_cleanup<Cleanup, CleanupFuture>(
        &self,
        budget: WatchStopBudget,
        cleanup: &mut Cleanup,
    ) -> Result<usize, AppError>
    where
        Cleanup: FnMut(SkinHostKind, CdpEndpoint) -> CleanupFuture,
        CleanupFuture: Future<Output = Result<usize, AppError>>,
    {
        self.shutdown_with_budget_cleanup_and_tail(budget, cleanup, false)
            .await
    }
}

/// 在 spawn 前原子保留一个 handler 容量名额；满额时拒绝继续创建任务。
pub(super) fn reserve_handler_task_capacity() -> Result<HandlerTaskReservation, AppError> {
    reserve_handler_task_for_purpose(false)
}
