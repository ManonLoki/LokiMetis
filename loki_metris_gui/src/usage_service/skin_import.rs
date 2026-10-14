//! 导入租约把窗口寿命映射为后台资源所有权，迟到的预检也必须回收。

use super::UsageService;
use crate::skins::{
    BatchImportResult, PreparedSkinImportBatch, SkinError, SkinImportPreparationEvent, SkinService,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::oneshot;

/// UI只持单一租约；观察进度的后台任务持session而不延长页面授权。
pub(crate) struct SkinImportLease {
    pub(crate) session: Arc<ImportSession>,
}
/// 一次预检、确认或提交的短锁状态，不复制文件事务的领域结果。
pub(crate) struct ImportSession {
    state: Mutex<ImportState>,
}
/// committing在提交预约时设置，离页不能取消已接管token的事务。
#[derive(Default)]
struct ImportState {
    token: Option<String>,
    abandoned: bool,
    committing: bool,
    finished: bool,
    total: usize,
    processed: usize,
}
impl ImportSession {
    /// 进度仅使用后台真实事件，不能以延迟估算成功。
    pub(crate) fn progress(&self) -> (usize, usize) {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (state.processed, state.total)
    }
    /// 事件先登记token，离页后同步交回后台正在运行的service清理。
    fn event(&self, service: &SkinService, event: SkinImportPreparationEvent) {
        let cancel = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match event {
                SkinImportPreparationEvent::Started { token, total_files } => {
                    state.token = Some(token);
                    state.finished = false;
                    state.total = total_files;
                }
                SkinImportPreparationEvent::ItemReady { .. }
                | SkinImportPreparationEvent::ItemSkipped { .. } => state.processed += 1,
            }
            state.abandoned && !state.committing
        };
        if cancel {
            let _ = self.cancel(service);
        }
    }
    /// 取消失败保留token，显式重试仍有真实资源可回收。
    pub(crate) fn cancel(&self, service: &SkinService) -> Result<(), SkinError> {
        let token = {
            let state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.committing || state.finished {
                return Ok(());
            }
            state.token.clone()
        };
        if let Some(token) = token {
            service.cancel_import(&token)?;
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // token尚未发出时只记离页；Started回调负责晚到的取消。
        state.finished = true;
        Ok(())
    }
    /// 接受提交之前复核当前租约；成功预约后只有commit终态能释放token。
    pub(crate) fn reserve_commit(&self) -> Result<String, SkinError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.abandoned || state.finished || state.committing {
            return Err(SkinError::new(
                "skin.import_expired",
                "待导入皮肤已失效，请重新选择 ZIP。",
            ));
        }
        let token = state
            .token
            .clone()
            .ok_or_else(|| SkinError::new("skin.import_expired", "预检尚未完成。"))?;
        state.committing = true;
        Ok(token)
    }
    /// 提交失败保留当前确认窗的token；离页时改为后台取消。
    fn committed(&self, success: bool) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.committing = false;
        state.finished = success;
        !success && state.abandoned
    }
    /// 返回离页标记，后台即便未收到UI回执也能决定回收。
    pub(super) fn abandon(&self) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .abandoned = true;
    }
    /// 查询导入是否已成功结束。
    fn finished(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .finished
    }
    /// 查询前台是否已离开该导入会话。
    fn abandoned(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .abandoned
    }
}
impl Drop for SkinImportLease {
    /// Drop只预约后台工作，不在GUI线程删除目录。
    fn drop(&mut self) {
        self.session.abandon();
    }
}
/// 导入预检的最终回执：预检通过的批次或换肤错误。
type PreparationReceipt = oneshot::Receiver<Result<PreparedSkinImportBatch, SkinError>>;

impl UsageService {
    /// ZIP选择已确定后才建立资源租约；后台接管迟到token的回收责任。
    pub(crate) fn prepare_skin_import(
        self: &Arc<Self>,
        paths: Vec<PathBuf>,
    ) -> Result<(SkinImportLease, PreparationReceipt), &'static str> {
        let session = Arc::new(ImportSession {
            state: Mutex::new(ImportState::default()),
        });
        let lease = SkinImportLease {
            session: session.clone(),
        };
        let state = self.context.skins.clone();
        {
            let mut sessions = state
                .imports
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            sessions.retain(|session| !session.finished());
            sessions.push(session.clone());
        }
        let observer = session.clone();
        let owner = state.clone();
        let registered = self.background.spawn("skin-import-owner", move |mut shutdown| async move {
            loop {
                if shutdown.is_cancelled() { observer.abandon(); }
                if observer.abandoned() && let Err(error) = observer.cancel(&owner.service) && shutdown.is_cancelled() { tracing::warn!(code=error.code, "skin import shutdown cleanup failed"); break; }
                if observer.finished() { break; }
                tokio::select! { _ = shutdown.cancelled(), if !shutdown.is_cancelled() => {}, _ = tokio::time::sleep(std::time::Duration::from_millis(30)) => {} }
            }
            owner.imports.lock().unwrap_or_else(std::sync::PoisonError::into_inner).retain(|session| !Arc::ptr_eq(session, &observer));
        });
        if let Err(error) = registered {
            state
                .imports
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .retain(|entry| !Arc::ptr_eq(entry, &session));
            return Err(error);
        }
        let work = self.skin_work(move |service| async move {
            let observer = session.clone();
            let event_service = service.clone();
            let result = service
                .prepare_import_batch_with_progress(paths, move |event| {
                    observer.event(&event_service, event)
                })
                .await;
            if result.is_err() {
                session
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .finished = true;
            }
            if session.abandoned() {
                session.cancel(&service)?;
                return Err(SkinError::new(
                    "skin.operation_cancelled",
                    "导入预检已取消。",
                ));
            }
            result
        })?;
        Ok((lease, work))
    }
    /// UI提交预约与后台终态使用同一session，离页不会重复取消成功事务。
    pub(crate) fn commit_skin_import(
        &self,
        session: Arc<ImportSession>,
        token: String,
        selected: Vec<String>,
        trusted: bool,
    ) -> Result<oneshot::Receiver<Result<BatchImportResult, SkinError>>, &'static str> {
        let rollback = session.clone();
        let work = self.skin_work(move |service| async move {
            let result = service
                .commit_import_batch(&token, &selected, trusted)
                .await;
            if session.committed(result.is_ok()) {
                session.cancel(&service)?;
            }
            result
        });
        if work.is_err() {
            rollback.committed(false);
        }
        work
    }
}

#[cfg(test)]
#[path = "skin_import_tests.rs"]
mod tests;
