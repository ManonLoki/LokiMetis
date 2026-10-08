//! 应用拥有的查询与单 writer 索引服务；只读查询不取消、不等待后台扫描。

mod queue;
mod query;
mod scan;
mod sources;
mod background;
mod discovery;
mod settings;
mod agents;
mod monitor_catalog;
#[cfg(test)]
mod source_tests;

pub use query::{UsageQuery, UsageResponse};
pub use sources::{SourceOperation,SourceOperationResult};
pub use discovery::{DiscoveryView,ManualAddResult};
use crate::backend::local_index::{current_user_home, ScanCoordinator, ScanProgress};
use loki_metris_core::{CoverageReport, LocalRecordsSummary, RootRecord, SourceClientKind,
    WorkbuddyStatisticsSnapshot, ScanStartOrigin};
use queue::{Intent, Queue};
use std::{collections::BTreeMap, path::PathBuf, sync::{Arc, Mutex}, time::Duration};
use tokio::{runtime::Runtime, sync::oneshot};

/// 一次查询的不可变概览事实，不保存另一份权威领域状态。
#[derive(Clone)]
pub struct UsageData {
    pub records: LocalRecordsSummary,
    pub roots: Vec<RootRecord>,
    pub observed_at_epoch_ms: i64,
    pub workbuddy: Option<WorkbuddyStatisticsSnapshot>,
    pub workbuddy_read_failed: bool,
}

/// 后台扫描的展示投影；revision 只在一轮完成/失败/取消收尾后增加。
#[derive(Clone, Default)]
pub struct ScanStatus {
    pub running: bool,
    pub client:Option<SourceClientKind>,
    pub revision: u64,
    pub error: Option<&'static str>,
    pub progress: Option<ScanProgress>,
}

/// 共享的是扫描元数据和 writer 许可；聚合结果不跨查询缓存。
struct ServiceContext {
    directory: PathBuf,
    coverage: Mutex<BTreeMap<SourceClientKind, CoverageReport>>,
    status: Mutex<ScanStatus>,
    writer: ScanCoordinator,
    settings:Arc<settings::SettingsStore>,
    agents:Arc<agents::AgentSettingsStore>,
    monitor_revision:std::sync::atomic::AtomicU64,
}

/// 两个各限 active+latest 的邮箱分别拥有查询与扫描，关闭时均合作收尾。
pub struct UsageService {
    runtime: Mutex<Option<Runtime>>,
    scans: Arc<Queue<Intent, ()>>,
    queries: Arc<Queue<UsageQuery, UsageResponse>>,
    context: Arc<ServiceContext>,
    background:Arc<background::BackgroundTaskOwner>,
    discovery:Arc<loki_metris_core::RootDiscoveryCoordinator>,
    discovery_batch:Arc<discovery::DiscoveryBatch>,
    lifecycle_started:std::sync::atomic::AtomicBool,
}

impl UsageService {
    /// 真实窗口回归显式绑定临时目录，不依赖进程全局环境或真实用户数据。
    #[cfg(test)]
    pub(crate) fn in_test_directory(directory: PathBuf) -> Result<Arc<Self>, &'static str> { Self::in_directory(directory) }
    /// 事件循环前建立应用 runtime；所有存储仍限定在产品的独立目录。
    pub fn new() -> Result<Arc<Self>, &'static str> {
        #[cfg(target_os = "macos")]
        let directory = current_user_home().map(|home| home.join("Library/Application Support/app-loki-metris/data"));
        #[cfg(target_os = "windows")]
        let directory = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).map(|root| root.join("app-loki-metris/data"));
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let directory = current_user_home().map(|home| home.join(".local/share/app-loki-metris"));
        let directory = std::env::var_os("LOKI_METRIS_DATA_DIR").map(PathBuf::from).or(directory)
            .filter(|path| path.is_absolute()).ok_or("usage_directory_unavailable")?;
        Self::in_directory(directory)
    }

    /// 显式目录入口供隔离回归使用，不重定向 HOME 或读取参考 App 配置。
    fn in_directory(directory: PathBuf) -> Result<Arc<Self>, &'static str> {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2)
            .thread_name("loki-metris-usage").thread_stack_size(16 * 1024 * 1024)
            .enable_all().build().map_err(|_| "usage_runtime_unavailable")?;
        let discovery=Arc::new(loki_metris_core::RootDiscoveryCoordinator::default());
        let background=Arc::new(background::BackgroundTaskOwner::with_runtime(discovery.clone(),runtime.handle().clone()));
        let settings=Arc::new(settings::SettingsStore::new(directory.join("usage-settings.json"),background.clone()));
        let agents=Arc::new(agents::AgentSettingsStore::new(directory.join("monitor"),background.clone()));
        let context = Arc::new(ServiceContext { directory, coverage: Mutex::new(BTreeMap::new()),
            status: Mutex::new(ScanStatus::default()), writer: ScanCoordinator::default(),
            settings, agents,monitor_revision:Default::default() });
        let scans: Arc<Queue<Intent, ()>> = Arc::new(Queue::default());
        let queries: Arc<Queue<UsageQuery, UsageResponse>> = Arc::new(Queue::default());
        let worker = scans.clone(); let state = context.clone();
        runtime.spawn(async move { worker.run(move |intent, token| scan::run(state.clone(), intent, token)).await; });
        let worker = queries.clone(); let state = context.clone();
        runtime.spawn(async move { worker.run(move |query, token| query::load(state.clone(), query, token)).await; });
        Ok(Arc::new(Self { runtime: Mutex::new(Some(runtime)), scans, queries, context,background,discovery,discovery_batch:Default::default(),lifecycle_started:Default::default() }))
    }

    /// 原生启动拥有独立中继任务，配置未读成功前监听门禁始终为空。
    pub fn start_monitoring(&self)->Result<(), &'static str> {
        let rendezvous=crate::monitor::relay::hook_relay_rendezvous_path().map_err(|_|"monitor_rendezvous_unavailable")?;
        self.start_monitoring_at(rendezvous)
    }

    /// 隔离测试只发布到显式传入路径，不覆盖生产或参考 App 缓存。
    fn start_monitoring_at(&self,path:PathBuf)->Result<(), &'static str> {
        let runtime=self.runtime.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let runtime=runtime.as_ref().ok_or("usage_service_closed")?;
        if self.context.agents.listener.get().is_some() {return Ok(())}
        let (_,control)=crate::monitor::listener::spawn_hook_listener(runtime.handle(),self.context.directory.join("monitor"),Vec::new(),path);
        self.context.agents.bind_listener(Arc::new(control))?;
        Ok(())
    }

    /// 返回真实中继状态；宿主尚未启动时显式失败，不伪造离线或成功快照。
    pub fn monitor_status(&self)->Result<crate::monitor::listener::HookRelayStatus,&'static str> {
        self.context.agents.listener.get().map(|control|control.snapshot()).ok_or("monitor_service_unavailable")
    }

    /// 查询只读既有派生库，快速切页只合并查询意图，不取消 writer。
    pub fn query(&self, query: UsageQuery) -> Result<oneshot::Receiver<Result<UsageResponse, &'static str>>, &'static str> {
        self.queries.submit(query)
    }

    /// 扫描在独立邮箱执行；周期触发 busy 则跳过，显式触发保留最新意图。
    pub fn request_scan(&self, client: Option<SourceClientKind>, origin: ScanStartOrigin) -> Result<(), &'static str> {
        if origin == ScanStartOrigin::PeriodicAutomatic {
            self.scans.try_submit_if_idle(Intent { client, origin })?;
        } else {
            let _reply = self.scans.submit_without_cancelling(Intent { client, origin })?;
        }
        Ok(())
    }

    /// 前台只读无路径的状态投影；扫描进度不用访问 SQLite。
    pub fn scan_status(&self) -> ScanStatus {
        self.context.status.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    /// 明确取消当前扫描与待处理扫描；普通查询不受影响。
    pub fn cancel(&self) { self.scans.cancel(); self.context.writer.cancel_active(); }

    /// 停止新 writer 并有界等待合作收尾，超时由 runtime 终止剩余任务。
    pub fn finish(&self) {
        if let Some(runtime) = self.runtime.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take() {
            self.context.writer.shutdown();
            if let Some(listener)=self.context.agents.listener.get() {runtime.block_on(listener.shutdown());}
            runtime.block_on(self.background.shutdown());
            if !self.scans.stop(Duration::from_secs(10)) { tracing::warn!(event = "usage_shutdown_drain_timeout"); }
            if !self.queries.stop(Duration::from_secs(5)) { tracing::warn!(event = "usage_query_drain_timeout"); }
            runtime.shutdown_timeout(Duration::from_secs(1));
        }
    }
}

#[cfg(test)]
mod monitor_tests;
