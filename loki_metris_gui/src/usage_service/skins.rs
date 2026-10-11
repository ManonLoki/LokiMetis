//! 换肤操作在应用拥有的阻塞岛执行，文件和CDP future不借用GPUI窗口。

use super::UsageService;
use crate::skins::{SkinDescriptor, SkinError, SkinService};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::{
    future::Future,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::{OnceCell, oneshot};

/// 目录快照同时准备原生图像，主线程不解Base64或读取文件。
pub(crate) struct SkinEntry {
    pub skin: SkinDescriptor,
    pub preview: Option<Arc<gpui_kit::Image>>,
}
/// 目录成功与否是单一事实，失败不以空资源库替代。
pub(crate) struct SkinCatalog {
    pub entries: Vec<SkinEntry>,
}
/// 应用唯一资源库owner，首次成功初始化以后不重复清理导入暂存。
pub(super) struct OwnedSkins {
    pub(super) service: Arc<SkinService>,
    pub(super) imports: std::sync::Mutex<Vec<Arc<super::skin_import::ImportSession>>>,
    initialized: OnceCell<()>,
    resource_error: Option<SkinError>,
    pub(super) started: AtomicBool,
}
impl OwnedSkins {
    /// 仅解析当前安装包或当前工程资源，不读取历史工程运行资源。
    pub(super) fn new(directory: &Path) -> Self {
        let resources = super::skin_resources::resource_root();
        let resource_error = resources.as_ref().err().cloned();
        let resources = resources.unwrap_or_else(|_| directory.join(".unavailable-bundled-skins"));
        Self {
            service: Arc::new(SkinService::new(
                resources.join("builtin-skins"),
                directory.join("skins"),
            )),
            imports: Default::default(),
            initialized: OnceCell::new(),
            resource_error,
            started: AtomicBool::new(false),
        }
    }
    /// 已接受操作也参与退出门禁，防止先关runtime再丢失事务收尾。
    pub(super) async fn shutdown(&self) {
        self.service.close_host_admission();
        self.cleanup_imports();
        if self.started.load(Ordering::Acquire) {
            self.service.shutdown().await;
        }
    }
    /// 关闭前后都清理应用登记的导入，迟到预检仍由同一session标记取消。
    pub(super) fn cleanup_imports(&self) {
        let sessions = self
            .imports
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        for session in sessions {
            session.abandon();
            if let Err(error) = session.cancel(&self.service) {
                tracing::warn!(code = error.code, "skin import cleanup failed");
            }
        }
    }
}

impl UsageService {
    /// 后台owner保留任务到终态，接收窗口消失不取消已接受写入。
    pub(crate) fn skin_work<T, F, Fut>(
        &self,
        work: F,
    ) -> Result<oneshot::Receiver<Result<T, SkinError>>, &'static str>
    where
        T: Send + 'static,
        F: FnOnce(Arc<SkinService>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, SkinError>> + Send + 'static,
    {
        self.skin_work_kind(work, false)
    }
    /// 只有真实宿主任务才拥有进程级CDP/launcher关闭责任。
    pub(crate) fn skin_host_work<T, F, Fut>(
        &self,
        work: F,
    ) -> Result<oneshot::Receiver<Result<T, SkinError>>, &'static str>
    where
        T: Send + 'static,
        F: FnOnce(Arc<SkinService>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, SkinError>> + Send + 'static,
    {
        self.skin_work_kind(work, true)
    }
    /// 资源事务与宿主事务共用阻塞岛，进程级owner仅由宿主入口启用。
    fn skin_work_kind<T, F, Fut>(
        &self,
        work: F,
        host: bool,
    ) -> Result<oneshot::Receiver<Result<T, SkinError>>, &'static str>
    where
        T: Send + 'static,
        F: FnOnce(Arc<SkinService>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, SkinError>> + Send + 'static,
    {
        let runtime = self
            .runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let handle = runtime
            .as_ref()
            .ok_or("usage_service_closed")?
            .handle()
            .clone();
        let state = self.context.skins.clone();
        let (tx, rx) = oneshot::channel();
        self.background.spawn_blocking("skin-operation", move |_| {
            let result = handle.block_on(async {
                if let Some(error) = &state.resource_error { return Err(error.clone()); }
                state
                    .initialized
                    .get_or_try_init(|| async { state.service.initialize() })
                    .await?;
                work(state.service.clone()).await
            });
            let _ = tx.send(result);
        })?;
        if host {
            self.context.skins.started.store(true, Ordering::Release);
        }
        Ok(rx)
    }
    /// 原生预览只接受本地库校验后的图片类型。
    pub(crate) fn skin_catalog(
        &self,
    ) -> Result<oneshot::Receiver<Result<SkinCatalog, SkinError>>, &'static str> {
        self.skin_work(|service| async move {
            let entries = service
                .list_skins()?
                .into_iter()
                .map(|mut skin| {
                    let preview =
                        skin.preview_data_url
                            .split_once(',')
                            .and_then(|(header, data)| {
                                let format = match header {
                                    "data:image/png;base64" => gpui_kit::ImageFormat::Png,
                                    "data:image/jpeg;base64" => gpui_kit::ImageFormat::Jpeg,
                                    _ => return None,
                                };
                                STANDARD.decode(data).ok().map(|bytes| {
                                    Arc::new(gpui_kit::Image::from_bytes(format, bytes))
                                })
                            });
                    // 描述快照不再携带Base64；释放容量，避免每个目录同时保留两份原图编码。
                    skin.preview_data_url = String::new();
                    SkinEntry { skin, preview }
                })
                .collect();
            Ok(SkinCatalog { entries })
        })
    }
}

/// 跨Tokio与GPUI只轮询回执；不把测试前台waker注册到外部runtime。
pub(crate) async fn receipt<T>(
    mut work: oneshot::Receiver<Result<T, SkinError>>,
    executor: gpui_kit::BackgroundExecutor,
) -> Result<T, SkinError> {
    loop {
        match work.try_recv() {
            Ok(value) => return value,
            Err(oneshot::error::TryRecvError::Closed) => {
                return Err(SkinError::new(
                    "skin.operation_unavailable",
                    "换肤操作回执不可用。",
                ));
            }
            Err(oneshot::error::TryRecvError::Empty) => {
                executor.timer(std::time::Duration::from_millis(30)).await
            }
        }
    }
}
