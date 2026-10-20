//! 设备能力复用应用后台owner，发现会话与串行配对/存储分别有界拥有。

mod discovery;
mod store;
mod transport;
#[cfg(test)]
mod tests;

use super::{UsageService, background::BackgroundTaskOwner};
use loki_metris_core::devices::{DeviceBook, DiscoveredDevice, SavedDeviceSummary, valid_pairing_code};
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
    /// 真实失败类别；UI负责双语映射。
    pub error: Option<&'static str>,
    /// 发现错误独立于存储操作，成功读取不能吞掉并发的组播失败。
    pub discovery_error: Option<&'static str>,
    /// 最近完成的操作，用于显示真实保存或移除反馈。
    pub completed: Option<&'static str>,
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
}

/// 领域账本与网络会话的应用级唯一owner，离页只撤销临时发现和未完成握手。
pub(super) struct Devices {
    store: Arc<store::DeviceStore>,
    owner: Arc<BackgroundTaskOwner>,
    state: Mutex<DeviceState>,
    /// 未确认Shutdown的daemon仍由应用保留，下一次发现或退出先重试清理。
    daemon: Mutex<Option<mdns_sd::ServiceDaemon>>,
}

impl Devices {
    /// 构造不加载文件、不发包，不建立第二套Tokio运行时。
    pub(super) fn new(directory: &Path, owner: Arc<BackgroundTaskOwner>) -> Self {
        Self { store: Arc::new(store::DeviceStore::new(directory)), owner, state: Mutex::new(DeviceState::default()), daemon: Mutex::new(None) }
    }

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
        view.discovered = state.candidates.values().filter_map(|(device, _)| ids.insert(device.id.clone()).then_some(device.clone())).collect();
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
            Ok(book) => { state.view.loaded = true; state.view.saved = book.summaries(); state.view.error = None; state.view.completed = completed; },
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
        if !valid_pairing_code(&code) { return Err("device_code_invalid"); }
        let view = self.snapshot();
        if !view.loaded { return Err("device_storage_failed"); }
        let target = view.discovered.into_iter().find(|device| device.id == id).ok_or("device_not_discovered")?;
        if view.saved.iter().any(|device| device.id == id) { return Err("device_already_saved"); }
        if !target.pairing_available { return Err("device_pairing_unavailable"); }
        self.begin()?;
        let (cancel, mut cancelled) = watch::channel(false);
        { let mut state = self.state(); state.pair_cancel = Some(cancel); state.view.pairing = true; }
        let this = self.clone();
        if let Err(error) = self.owner.spawn("device-pair", move |mut shutdown| async move {
            let result = async {
                let id = target.id.clone();
                let client = this.disk(move |store| store.prepare_pairing(&id)).await?;
                let proof = tokio::select! {
                    biased;
                    _ = shutdown.cancelled() => return Err("device_cancelled"),
                    _ = cancelled.changed() => return Err("device_cancelled"),
                    result = tokio::time::timeout(Duration::from_secs(15), transport::pair(&target, &client, &code)) => result.map_err(|_| "device_timeout")??,
                };
                // 最终认证回执已接收，接受本地提交后不因离页丢弃真实存储结果。
                this.disk(move |store| store.paired(proof)).await
            }.await;
            this.finish(result, Some("paired"));
        }) { self.finish(Err(error), None); return Err(error); }
        Ok(())
    }

    /// 已接受的忘记操作由应用持有至落盘，与页面寿命无关。
    fn remove(self: &Arc<Self>, id: String) -> Result<(), &'static str> {
        self.begin()?; let this = self.clone();
        if let Err(error) = self.owner.spawn_blocking("device-remove", move |_| {
            this.finish(this.store.remove(&id), Some("removed"));
        }) { self.finish(Err(error), None); return Err(error); } Ok(())
    }

    /// 离开页面和取消动作撤销发现及握手；已经进入原子提交的记录仍真实收尾。
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
    /// 开始有界mDNS浏览，不注册本机接收端服务。
    pub(crate) fn discover_devices(&self) -> Result<(), &'static str> { self.context.devices.discover() }
    /// 传入用户明确选中身份和四位草稿；不会自动重试配对。
    pub(crate) fn pair_device(&self, id: &str, code: String) -> Result<(), &'static str> { self.context.devices.pair(id, code) }
    /// 忘记一条本机保存记录；没有远端撤销或业务数据副作用。
    pub(crate) fn remove_device(&self, id: String) -> Result<(), &'static str> { self.context.devices.remove(id) }
    /// 离页或隐藏暂停临时联网会话，保留已保存设备。
    pub(crate) fn cancel_device_activity(&self) { self.context.devices.cancel(); }
}
