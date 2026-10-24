//! 应用拥有的查询与单 writer 索引服务；只读查询不取消、不等待后台扫描。

mod queue;
mod today;
mod today_cache;
mod page_cache;
#[cfg(test)]
mod page_cache_tests;
#[cfg(test)]
mod today_cache_tests;
mod notifications;
use crate::notification_events as activity_notifications;
pub use notifications::Progress as NotificationProgress;
mod query;
mod cursor_usage;
mod scan;
mod sources;
mod background;
mod discovery;
mod settings;
mod agents;
mod devices;
pub use devices::{DeviceView, DevicePairingRequest};
mod pet;
mod skins;
mod skin_import;
pub use skin_import::SkinImportLease;
pub use skins::{SkinCatalogEntry, SkinPreviewFormat};
mod skin_resources;
pub use skin_resources::resource_root as skin_resource_root;
pub use pet::PetShutdownPlan;
#[cfg(any(test, feature = "test-support"))]
pub use agents::shutdown_tests::{block_pet_mutations, pet_background_task_count};
mod monitor_catalog;
mod hooks;
pub mod provider_quota;
#[cfg(test)]
mod source_tests;

pub use query::{UsageQuery, UsageResponse};
pub use sources::{SourceOperation,SourceOperationResult};
pub use discovery::{DiscoveryView,ManualAddResult};
use loki_metris_host::local_index::{current_user_home, ScanCoordinator};
/// 扫描进度属于本服务的公开状态；展示层经服务层取得类型，不直接依赖数据源驱动。
pub use loki_metris_host::local_index::ScanProgress;
/// 通知预约时间戳由本服务按此时钟写入，宿主比较时必须使用同一时钟。
pub use loki_metris_host::local_index::current_epoch_ms;
use loki_metris_core::{CoverageReport, LocalRecordsSummary, SourceClientKind,
    WorkBuddyStatisticsSnapshot, ScanStartOrigin};
use queue::{Intent, Queue};
use std::{collections::BTreeMap, path::PathBuf, sync::{Arc, Mutex}, time::Duration};
use tokio::{runtime::Runtime, sync::oneshot};

/// 一次查询的不可变概览事实，不保存另一份权威领域状态。
#[derive(Clone)]
pub struct UsageData {
    /// 所选 Agent 的本地用量窗口摘要。
    pub records: LocalRecordsSummary,
    /// WorkBuddy 统计快照；非 WorkBuddy 视图或读取失败时为 None。
    pub workbuddy: Option<WorkBuddyStatisticsSnapshot>,
    /// “全部”概览中 WorkBuddy 读取失败：结果只含其他 Agent，界面必须明示不完整，不能静默少算。
    pub workbuddy_unavailable: bool,
}

/// 后台扫描的展示投影；revision 在扫描收尾或派生来源/清理变更后递增。
#[derive(Clone, Default)]
pub struct ScanStatus {
    /// 当前有扫描在途。
    pub running: bool,
    /// 在途扫描的 Agent；全部 Agent 扫描时为 None。
    pub client:Option<SourceClientKind>,
    /// 展示刷新代数，由扫描收尾、候选登记、来源操作及清理等变更递增。
    pub revision: u64,
    /// 最近一轮扫描的失败类别。
    pub error: Option<&'static str>,
    /// 在途扫描的进度；空闲时为 None。
    pub progress: Option<ScanProgress>,
}

/// 共享扫描元数据、writer 许可与受输入键约束的派生汇总/页面缓存。
struct ServiceContext {
    /// 所有阻塞摘要计算登记应用owner，取消网络等待不会脱离退出回收。
    background: Arc<background::BackgroundTaskOwner>,
    /// 大摘要聚合跨消费者串行，反复取消或重连不会叠加大快照计算。
    today_aggregate_gate: Arc<tokio::sync::Semaphore>,
    /// 应用启动即拥有局域网设备服务；页面只消费投影及临时配对交互。
    devices: Arc<devices::Devices>,
    /// 应用私有数据目录，各 Agent 的索引、设置与资源都在其下。
    directory: PathBuf,
    /// 当日摘要需要重新读取时唤醒后台 worker。
    today_refresh: Arc<tokio::sync::Notify>,
    /// 通知来源代次；数据根或启用 Agent 变化时递增，旧来源的里程碑预约随之作废。
    notification_source_epoch: Arc<std::sync::atomic::AtomicU64>,
    /// 应用唯一的皮肤资源库 owner。
    skins:Arc<skins::OwnedSkins>,
    /// 各 Agent 最近一次扫描的覆盖报告。
    coverage: Mutex<BTreeMap<SourceClientKind, CoverageReport>>,
    /// 后台扫描的展示投影。
    status: Mutex<ScanStatus>,
    /// 索引写入许可，保证同一时刻只有一个扫描 writer。
    writer: ScanCoordinator,
    /// 用量设置存储。
    settings:Arc<settings::SettingsStore>,
    /// 监控设置（启用 Agent、桌宠偏好等）存储。
    agents:Arc<agents::AgentSettingsStore>,
    /// 监控图库修订号；图片或配置草稿变化时递增。
    monitor_revision:std::sync::atomic::AtomicU64,
    /// 当日摘要的索引侧结果缓存。
    today_cache: today_cache::TodayCache,
    /// 当前查看的统计页或图表页结果缓存，页面定时刷新在索引未变时直接复用。
    page_cache: page_cache::PageCache,
    /// Hook 与换肤终态的瞬时通知邮箱。
    activity_notifications: Arc<activity_notifications::ActivityQueue>,
    /// 提供商 5 小时与周用量快照。窗口线程只复制它。
    quota: Arc<provider_quota::QuotaBoard>,
    /// Cursor 云端账户的内存窗口，不参与本机汇总或终端本机 Token。
    cursor_usage: cursor_usage::Reader,
}

/// 两个各限 active+latest 的邮箱分别拥有查询与扫描，关闭时均合作收尾。
pub struct UsageService {
    /// 服务专用 Tokio 运行时；退出时取出并在其上完成收尾。
    runtime: Mutex<Option<Runtime>>,
    /// Token 里程碑通知的后台账本 worker。
    notifications: notifications::Monitor,
    /// 扫描邮箱：至多一个在途加一个最新待办。
    scans: Arc<Queue<Intent, ()>>,
    /// 查询邮箱：至多一个在途加一个最新待办，新请求取代旧的待办。
    queries: Arc<Queue<UsageQuery, UsageResponse>>,
    /// 各 worker 共享的服务上下文。
    context: Arc<ServiceContext>,
    /// 数据根发现与一次性维护任务的唯一 owner。
    background:Arc<background::BackgroundTaskOwner>,
    /// 数据根自动发现的共享协调器。
    discovery:Arc<loki_metris_core::RootDiscoveryCoordinator>,
    /// 发现候选的登记与索引批次状态。
    discovery_batch:Arc<discovery::DiscoveryBatch>,
    /// 后台用量生命周期已启动，重复调用直接返回。
    lifecycle_started:std::sync::atomic::AtomicBool,
}

impl UsageService {
    /// 真实窗口回归显式绑定临时目录，不依赖进程全局环境或真实用户数据。
    #[cfg(any(test, feature = "test-support"))]
    pub fn in_test_directory(directory: PathBuf) -> Result<Arc<Self>, &'static str> { Self::in_directory(directory) }
    /// 窗口回归仅在隔离路径启动真实监听，不写用户端点或 Hook 配置。
    #[cfg(any(test, feature = "test-support"))]
    pub fn start_test_monitoring_at(&self, path: PathBuf) -> Result<(), &'static str> {
        self.start_monitoring_at(path)
    }
    /// 事件循环前建立应用 runtime；所有存储仍限定在产品的独立目录。
    pub fn new() -> Result<Arc<Self>, &'static str> {
        #[cfg(target_os = "macos")]
        let directory = current_user_home().map(|home| home.join("Library/Application Support/app-loki-metris/data"));
        #[cfg(target_os = "windows")]
        let directory = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).map(|root| root.join("app-loki-metris/data"));
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let directory = current_user_home().map(|home| home.join(".local/share/app-loki-metris"));
        let override_directory = std::env::var_os("LOKI_METRIS_DATA_DIR").map(PathBuf::from);
        let isolated = override_directory.as_ref().is_some_and(|selected| directory.as_ref().is_none_or(|normal| {
            selected != normal && std::fs::canonicalize(selected).ok().zip(std::fs::canonicalize(normal).ok()).is_none_or(|(a,b)| a != b)
        }));
        let directory = override_directory.or(directory)
            .filter(|path| path.is_absolute()).ok_or("usage_directory_unavailable")?;
        let service = Self::in_directory_profile(directory, isolated)?;
        if let Err(error) = service.context.devices.start_application_server() {
            tracing::warn!(code = error, "device service could not start");
        }
        Ok(service)
    }

    /// 显式目录入口供隔离回归使用，不重定向 HOME 或读取参考 App 配置。
    #[cfg(any(test, feature = "test-support"))]
    fn in_directory(directory: PathBuf) -> Result<Arc<Self>, &'static str> {
        Self::in_directory_profile(directory, true)
    }

    /// 数据根与实例类型一起冻结，正式/隔离运行从不隐式共享或迁移配对账本。
    fn in_directory_profile(directory: PathBuf, isolated: bool) -> Result<Arc<Self>, &'static str> {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2)
            .thread_name("loki-metris-usage").thread_stack_size(16 * 1024 * 1024)
            .enable_all().build().map_err(|_| "usage_runtime_unavailable")?;
        let discovery=Arc::new(loki_metris_core::RootDiscoveryCoordinator::default());
        let background=Arc::new(background::BackgroundTaskOwner::with_runtime(discovery.clone(),runtime.handle().clone()));
        let settings=Arc::new(settings::SettingsStore::new(directory.join("usage-settings.json"),background.clone()));
        let today_refresh=Arc::new(tokio::sync::Notify::new());
        let notification_source_epoch=Arc::new(std::sync::atomic::AtomicU64::new(0));
        let agents=Arc::new(agents::AgentSettingsStore::new(directory.join("monitor"),background.clone(),today_refresh.clone(),notification_source_epoch.clone()));
        let skins=Arc::new(skins::OwnedSkins::new(&directory));
        let mut devices = devices::Devices::new(&directory, background.clone());
        devices.isolated = isolated;
        let devices = Arc::new(devices);
        let context = Arc::new(ServiceContext { background: background.clone(), today_aggregate_gate: Arc::new(tokio::sync::Semaphore::new(1)), devices, directory, today_refresh, notification_source_epoch, skins, coverage: Mutex::new(BTreeMap::new()),
            status: Mutex::new(ScanStatus::default()), writer: ScanCoordinator::default(),
            settings, agents,monitor_revision:Default::default(),today_cache:Default::default(),page_cache:Default::default(),activity_notifications:Default::default(), quota: Arc::new(provider_quota::QuotaBoard::new()), cursor_usage: Default::default() });
        context.devices.bind_context(Arc::downgrade(&context));
        let scans: Arc<Queue<Intent, ()>> = Arc::new(Queue::default());
        let queries: Arc<Queue<UsageQuery, UsageResponse>> = Arc::new(Queue::default());
        let worker = scans.clone(); let state = context.clone();
        runtime.spawn(async move { worker.run(move |intent, token| scan::run(state.clone(), intent, token)).await; });
        let worker = queries.clone(); let state = context.clone();
        runtime.spawn(async move { worker.run(move |query, token| query::load(state.clone(), query, token)).await; });
        let notifications=notifications::Monitor::start(&runtime,context.clone());
        Ok(Arc::new(Self { notifications, runtime: Mutex::new(Some(runtime)), scans, queries, context,background,discovery,discovery_batch:Default::default(),lifecycle_started:Default::default() }))
    }

    /// 原生启动拥有独立中继任务，配置未读成功前监听门禁始终为空。
    pub fn start_monitoring(&self)->Result<(), &'static str> {
        let rendezvous=crate::monitor::relay::hook_relay_rendezvous_path().map_err(|_|"monitor_rendezvous_unavailable")?;
        self.start_monitoring_at(rendezvous)?;
        let executable=std::env::current_exe().map_err(|_|"hook_executable_unavailable")?;
        // 从磁盘映像或临时转移目录运行时不写入 Hook，避免在其它工具配置里留下之后失效的命令。
        if hooks::is_ephemeral_executable(&executable) {return Err("hook_executable_ephemeral")}
        let home=current_user_home().ok_or("hook_home_unavailable")?;
        self.start_hook_writer_at(executable,home)
    }

    /// 隔离测试只发布到显式传入路径，不覆盖生产或参考 App 缓存。
    fn start_monitoring_at(&self,path:PathBuf)->Result<(), &'static str> {
        let runtime=self.runtime.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let runtime=runtime.as_ref().ok_or("usage_service_closed")?;
        if self.context.agents.listener.get().is_some() {return Ok(())}
        let (_,control)=crate::monitor::listener::spawn_hook_listener_with_notifications(runtime.handle(),self.context.directory.join("monitor"),Vec::new(),path,Some(self.activity_notifications()));
        self.context.agents.bind_listener(Arc::new(control))?;
        Ok(())
    }

    /// 返回真实中继状态；宿主尚未启动时显式失败，不伪造离线或成功快照。
    pub fn monitor_status(&self)->Result<crate::monitor::listener::HookRelayStatus,&'static str> {
        self.context.agents.listener.get().map(|control|control.snapshot()).ok_or("monitor_service_unavailable")
    }

    /// 复制提供商用量快照。不读取登录文件。
    pub fn provider_quota_snapshot(&self) -> provider_quota::QuotaBoardSnapshot {
        self.context.quota.snapshot()
    }

    /// 请求后台刷新用量。测试构建（含 GUI 测试启用的 test-support）直接返回，避免触碰本机登录态。
    pub fn kick_provider_quota(&self, force: bool) -> bool {
        if cfg!(any(test, feature = "test-support")) {
            let _ = force;
            return true;
        }
        self.context.quota.kick(force, &self.context.background)
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

    /// 退出先拥有最终位置屏障，有界回收后台，再提交前台尚未派发的桌宠最终意图。
    pub fn finish_with_pet(&self, plan:PetShutdownPlan) {
        if let Some(runtime) = self.runtime.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take() {
            runtime.block_on(self.notifications.finish());
            self.context.agents.freeze_final_pet_position(plan.position);
            self.context.writer.shutdown();
            if let Some(host)=self.context.agents.hooks.get() {host.writer.shutdown();}
            if let Some(listener)=self.context.agents.listener.get() {runtime.block_on(listener.shutdown());}
            runtime.block_on(self.context.skins.shutdown());
            runtime.block_on(self.background.shutdown());
            if runtime.block_on(self.context.devices.cleanup_daemon()).is_err() {
                tracing::warn!(event="device_mdns_shutdown_unconfirmed");
            }
            if runtime.block_on(self.context.devices.cleanup_host_daemon()).is_err() {
                tracing::warn!(event="device_host_mdns_shutdown_unconfirmed");
            }
            self.context.skins.cleanup_imports();
            for change in plan.changes {
                if let Err(error)=self.context.agents.save_pet_blocking(Some(change),None) {tracing::warn!(code=error.code,"pet shutdown preference failed");}
            }
            if let Some(position)=plan.position&& let Err(error)=self.context.agents.save_pet_blocking(None,Some(position)) {tracing::warn!(code=error.code,"pet shutdown position failed");}
            if !self.scans.stop(Duration::from_secs(10)) { tracing::warn!(event = "usage_shutdown_drain_timeout"); }
            if !self.queries.stop(Duration::from_secs(5)) { tracing::warn!(event = "usage_query_drain_timeout"); }
            runtime.shutdown_timeout(Duration::from_secs(1));
        }
    }
}

#[cfg(test)]
mod monitor_tests;

#[cfg(any(test, feature = "test-support"))]
mod test_seams;
