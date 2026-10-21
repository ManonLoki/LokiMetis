//! 设备能力复用应用后台owner，发现会话与串行配对/存储分别有界拥有。

mod discovery;
mod framing;
mod link_transport;
mod server;
mod publication;
mod widget_http;
mod store;
mod transport;
#[cfg(test)]
mod tests;

use super::{UsageService, background::BackgroundTaskOwner};
use loki_metris_core::devices::{DeviceBook, DiscoveredDevice, SavedDeviceSummary, valid_pairing_code};
use loki_metris_core::devices::presence::{DevicePresence, DevicePresenceView};
use std::{collections::BTreeMap, path::Path, sync::{Arc, Mutex}, time::{Duration, Instant}};
use tokio::sync::{oneshot, watch};

/// 页面只读无秘密的当前投影；“已发现”只表示近期广告，不能宣称已认证在线。
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub(crate) struct DeviceView {
    /// 已成功读取过保存列表。
    pub loaded: bool,
    /// 读取、配对或移除正在执行；同一时刻只有一项。
    pub pending: bool,
    /// 仅配对握手允许取消；读取和已接受的原子移除不显示假取消入口。
    pub pairing: bool,
    /// 本轮mDNS浏览仍在运行。
    pub scanning: bool,
    /// 已保存设备的无凭据摘要。
    pub saved: Vec<SavedDeviceSummary>,
    /// 当前短时有效的未认证广告。
    pub discovered: Vec<DiscoveredDevice>,
    /// 仅真实客户端连接产生的待配对请求；票据区分同一设备的不同连接。
    pub requests: Vec<DevicePairingRequest>,
    /// 真实失败类别；UI负责双语映射。
    pub error: Option<&'static str>,
    /// 发现错误独立于存储操作，成功读取不能吞掉并发的组播失败。
    pub discovery_error: Option<&'static str>,
    /// 最近完成的操作，用于显示真实保存或移除反馈。
    pub completed: Option<&'static str>,
    /// 当前应用拥有的局域网服务实际已监听端口，启动中为None。
    pub listening_port: Option<u16>,
    /// 监听/广播错误独立于用户配对或发现错误。
    pub link_error: Option<&'static str>,
    /// 仅来自已认证会话的在线、平台和IP投影。
    pub presence: Vec<DevicePresenceView>,
}

/// 未认证的客户端介绍与本机生成的请求身份，不包含配对码或可信平台信息。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DevicePairingRequest {
    /// 桌面本进程生成的单调请求票据，同ID重连也不得复用。
    pub ticket: u64,
    /// 未认证介绍与实际连接地址，仅供用户核对。
    pub device: DiscoveredDevice,
}

/// 服务状态只在短锁内访问；磁盘读写与网络等待均在锁外执行。
#[derive(Default)]
struct DeviceState {
    view: DeviceView,
    candidates: BTreeMap<String, (DiscoveredDevice, Instant)>,
    scan_cancel: Option<watch::Sender<bool>>,
    pair_cancel: Option<watch::Sender<bool>>,
    restart_scan: bool,
    /// 取消代次在同一短锁内撤销尚未预约的自动重启，避免离页后迟到发包。
    scan_epoch: u64,
    /// 真实存储提交后的账本快照，与presence提交在同一短锁中校验。
    book: Option<DeviceBook>,
    /// 已认证连接的唯一领域状态。
    presence: DevicePresence,
    /// 临时配对报到与长期连接均由同一应用服务拥有。
    server: server::ServerState,
}

/// 领域账本与网络会话的应用级唯一owner，页面切换不改变监听与连接寿命。
pub(super) struct Devices {
    store: Arc<store::DeviceStore>,
    owner: Arc<BackgroundTaskOwner>,
    state: Mutex<DeviceState>,
    /// 未确认Shutdown的daemon仍由应用保留，下一次发现或退出先重试清理。
    daemon: Mutex<Option<mdns_sd::ServiceDaemon>>,
    /// 数据服务广播单独拥有，临时发现结束不关闭它。
    host_daemon: Mutex<Option<mdns_sd::ServiceDaemon>>,
    /// 弱引用只供后台发布读取，不能形成context与devices的所有权环。
    context: std::sync::OnceLock<std::sync::Weak<super::ServiceContext>>,
    /// 每个连接只保留最新完整发布，不因慢设备积压状态队列。
    publications: watch::Sender<Arc<publication::Publication>>,
    /// 独立唤醒，不消费通知worker的today_refresh。
    publication_refresh: tokio::sync::Notify,
    /// 旧发布任务取消后，仍在运行的有界图库读只能与下一轮串行，不能叠加大正文。
    catalog_gate: Arc<tokio::sync::Semaphore>,
}

impl Devices {
    /// 构造不加载文件、不发包，不建立第二套Tokio运行时。
    pub(super) fn new(directory: &Path, owner: Arc<BackgroundTaskOwner>) -> Self {
        Self { store: Arc::new(store::DeviceStore::new(directory)), owner, state: Mutex::new(DeviceState::default()), daemon: Mutex::new(None), host_daemon: Mutex::new(None),
            context: Default::default(), publications: watch::channel(Arc::new(publication::Publication::default())).0, publication_refresh: Default::default(), catalog_gate: Arc::new(tokio::sync::Semaphore::new(1)) }
    }

    /// 初始化后仅绑定一次，构造与绑定均不读取磁盘或网络。
    pub(super) fn bind_context(&self, context: std::sync::Weak<super::ServiceContext>) { let _ = self.context.set(context); }

    /// 统一恢复poison仅用于短内存锁，不向日志输出快照或秘密。
    fn state(&self) -> std::sync::MutexGuard<'_, DeviceState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// 清除超过30秒的发现租约，保证旧IP不再作为可配对候选。
    fn snapshot(&self) -> DeviceView {
        let mut state = self.state();
        state.candidates.retain(|_, (_, expires)| *expires > Instant::now());
        let mut ids = std::collections::BTreeSet::new();
        let mut view = state.view.clone();
        view.discovered.clear();
        view.requests.clear();
        for offer in state.server.offers.values() {
            if offer.expires > Instant::now() && ids.insert(offer.target.id.clone()) {
                view.discovered.push(offer.target.clone());
                view.requests.push(DevicePairingRequest { ticket: offer.ticket, device: offer.target.clone() });
            }
        }
        view.requests.sort_by_key(|request| request.ticket);
        view.discovered.extend(state.candidates.values().filter_map(|(device, _)| ids.insert(device.id.clone()).then_some(device.clone())));
        if let Some(book) = &state.book {
            view.presence = state.presence.snapshot(book, Instant::now());
            for presence in &mut view.presence {
                presence.since_last_contact = presence.since_last_contact.map(|age| Duration::from_secs(age.as_secs()));
                if let Some(watch) = &mut presence.companion {
                    watch.since_last_contact = watch.since_last_contact.map(|age| Duration::from_secs(age.as_secs()));
                    watch.since_last_sync = watch.since_last_sync.map(|age| Duration::from_secs(age.as_secs()));
                }
            }
        }
        view
    }

    /// 拒绝第二个写入意图；错误不会让已有在途操作失去owner。
    fn begin(&self) -> Result<(), &'static str> {
        let mut state = self.state();
        if state.view.pending { return Err("device_busy"); }
        state.view.pending = true; state.view.error = None; state.view.completed = None; Ok(())
    }

    /// 仅把真实读取或提交成功的账本投影给页面，失败保留先前保存列表。
    fn finish(&self, result: Result<DeviceBook, &'static str>, completed: Option<&'static str>) {
        let mut state = self.state(); state.view.pending = false; state.view.pairing = false; state.pair_cancel = None;
        match result {
            Ok(book) => {
                state.view.loaded = true; state.view.saved = book.summaries(); state.view.error = None; state.view.completed = completed;
                // 握手期间同ID的另一条报到不能在保存成功后再次要求配对。
                let trusted: std::collections::BTreeSet<_> = state.view.saved.iter().map(|saved| saved.id.clone()).collect();
                state.server.offers.retain(|id, _| !trusted.contains(id));
                let removed: Vec<_> = state.server.sessions.keys().filter(|id| !state.view.saved.iter().any(|device| &device.id == *id)).cloned().collect();
                for id in removed {
                    if let Some(session) = state.server.sessions.remove(&id) { let _ = session.cancel.send(true); }
                    state.presence.forget(&id);
                }
                state.book = Some(book);
            },
            Err(error) => { state.view.error = Some(error); state.view.completed = None; },
        }
    }

    /// 在同一应用owner里登记阻塞文件阶段，退出时仍有明确回收责任。
    async fn disk<T: Send + 'static>(&self, operation: impl FnOnce(&store::DeviceStore) -> Result<T, &'static str> + Send + 'static) -> Result<T, &'static str> {
        let store = self.store.clone(); let (tx, rx) = oneshot::channel();
        self.owner.spawn_blocking("device-storage", move |_| { let _ = tx.send(operation(&store)); })?;
        rx.await.map_err(|_| "device_storage_failed")?
    }

    /// 懒读取独立设备账本，读取失败可由同一入口显式重试。
    fn load(self: &Arc<Self>) -> Result<(), &'static str> {
        self.begin()?; let this = self.clone();
        if let Err(error) = self.owner.spawn_blocking("device-load", move |_| { this.finish(this.store.load(), None); }) {
            self.finish(Err(error), None); return Err(error);
        } Ok(())
    }

    /// 用户主动配对仅消费一项当前候选，四位码不进入状态、文件或日志。
    fn pair(self: &Arc<Self>, id: &str, code: String) -> Result<(), &'static str> {
        self.pair_target(id, None, code)
    }

    /// 配对弹窗必须绑定精确请求；旧弹窗不能把配对码交给同ID的新连接。
    fn pair_target(self: &Arc<Self>, id: &str, ticket: Option<u64>, code: String) -> Result<(), &'static str> {
        if !valid_pairing_code(&code) { return Err("device_code_invalid"); }
        let view = self.snapshot();
        if !view.loaded { return Err("device_storage_failed"); }
        let target = view.discovered.into_iter().find(|device| device.id == id).ok_or("device_not_discovered")?;
        if view.saved.iter().any(|device| device.id == id) { return Err("device_already_saved"); }
        if !target.pairing_available { return Err("device_pairing_unavailable"); }
        self.begin()?;
        let offered = {
            let mut state = self.state();
            if ticket.is_some_and(|ticket| state.server.offers.get(id).is_none_or(|offer| offer.ticket != ticket || offer.expires <= Instant::now())) {
                drop(state); self.finish(Err("device_request_expired"), None); return Err("device_request_expired");
            }
            state.server.offers.remove(id)
        };
        let (cancel, mut cancelled) = watch::channel(false);
        { let mut state = self.state(); state.pair_cancel = Some(cancel); state.view.pairing = true; }
        let this = self.clone();
        if let Err(error) = self.owner.spawn("device-pair", move |mut shutdown| async move {
            let result = async {
                let id = target.id.clone();
                let prepare_id = id.clone();
                let client = this.disk(move |store| store.prepare_pairing(&prepare_id)).await?;
                let proof = tokio::select! {
                    biased;
                    _ = shutdown.cancelled() => return Err("device_cancelled"),
                    _ = cancelled.changed() => return Err("device_cancelled"),
                    result = tokio::time::timeout(Duration::from_secs(15), async {
                        if let Some(offer) = offered {
                            let (result, received) = oneshot::channel();
                            offer.command.send(server::PairingCommand { code, client, result }).map_err(|_| "device_not_discovered")?;
                            received.await.map_err(|_| "device_connection_failed")?
                        } else { transport::pair(&target, &client, &code).await }
                    }) => result.map_err(|_| "device_timeout")??,
                };
                // 最终认证回执已接收，接受本地提交后不因离页丢弃真实存储结果。
                this.disk(move |store| store.paired(proof)).await
            }.await;
            this.finish(result, Some("paired"));
            let resume = { let state = this.state(); state.server.requested && !state.view.saved.is_empty() };
            if resume { let _ = this.start_server(); }
        }) { self.finish(Err(error), None); return Err(error); }
        Ok(())
    }

    /// 只拒绝弹窗所属连接，取消旧票据不会撤销同ID的新请求。
    fn reject_request(&self, id: &str, ticket: u64) {
        let mut state = self.state();
        if state.server.offers.get(id).is_some_and(|offer| offer.ticket == ticket) { state.server.offers.remove(id); }
    }

    /// 已接受的忘记操作由应用持有至落盘，与页面寿命无关。
    fn remove(self: &Arc<Self>, id: String) -> Result<(), &'static str> {
        self.begin()?; let this = self.clone();
        if let Err(error) = self.owner.spawn_blocking("device-remove", move |_| {
            this.finish(this.store.remove(&id), Some("removed"));
        }) { self.finish(Err(error), None); return Err(error); } Ok(())
    }

    /// 用户取消只撤销临时发现及握手，应用级监听与其他待处理请求继续运行。
    fn cancel(&self) {
        let mut state = self.state();
        state.restart_scan = false;
        state.scan_epoch = state.scan_epoch.wrapping_add(1);
        if let Some(cancel) = state.scan_cancel.as_ref() { let _ = cancel.send(true); }
        if let Some(cancel) = state.pair_cancel.as_ref() { let _ = cancel.send(true); }
        state.candidates.clear();
    }
}

impl UsageService {
    /// 设备快照纯内存读取，可由GPUI现有定时器安全调用。
    pub(crate) fn devices(&self) -> DeviceView { self.context.devices.snapshot() }
    /// 用户进入页面时加载保存列表，失败通过快照披露。
    pub(crate) fn load_devices(&self) -> Result<(), &'static str> { self.context.devices.load() }
    /// 启动或恢复设备面板只确保应用服务存在，不再主动浏览旧终端。
    pub(crate) fn start_device_service(&self) -> Result<(), &'static str> { self.context.devices.start_server() }
    /// 对当前客户端连接明确提交用户输入，不能退回旧广告地址。
    pub(crate) fn pair_device_request(&self, id: &str, ticket: u64, code: String) -> Result<(), &'static str> {
        self.context.devices.pair_target(id, Some(ticket), code)
    }
    /// 模态关闭拒绝精确请求，释放其连接与未认证并发槽。
    pub(crate) fn reject_device_request(&self, id: &str, ticket: u64) { self.context.devices.reject_request(id, ticket); }
    /// 传入用户明确选中身份和四位草稿；不会自动重试配对。
    pub(crate) fn pair_device(&self, id: &str, code: String) -> Result<(), &'static str> { self.context.devices.pair(id, code) }
    /// 忘记一条本机保存记录；没有远端撤销或业务数据副作用。
    pub(crate) fn remove_device(&self, id: String) -> Result<(), &'static str> { self.context.devices.remove(id) }
    /// 用户明确取消在途握手，保留应用服务与其他待配对请求。
    pub(crate) fn cancel_device_activity(&self) { self.context.devices.cancel(); }
}
