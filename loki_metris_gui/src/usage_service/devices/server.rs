//! 桌面拥有监听、广播和所有连接JoinSet；离开页面只撤销临时配对，不关闭已配对联动。

use super::{Devices, Duration, Instant, framing::FrameReader, link_transport, transport};
use crate::usage_service::background::BackgroundTaskShutdown;
use loki_metris_core::devices::{DiscoveredDevice, VerifiedPairing, is_lan_endpoint, MAX_SAVED_DEVICES,
    link::{ConnectionIntroduction, HOST_SERVICE_TYPE, LinkControl, PairingOffer, EncryptedPacket, MAX_ENCRYPTED_FRAME_BYTES},
    presence::{PresenceLease, UnavailableReason, DEVICE_HEARTBEAT_INTERVAL}};
use std::{collections::BTreeMap, net::SocketAddr, sync::Arc};
use tokio::{net::{TcpListener, TcpStream}, sync::{Semaphore, OwnedSemaphorePermit, oneshot, watch}, task::JoinSet};

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;

/// 限制未认证连接数量，不能用任意慢速连接耗尽应用任务或内存。
const MAX_PENDING_CONNECTIONS: usize = 8;
/// 报到最多等待用户60秒，终端自己的配对窗口仍是最终有效期权威。
const PAIRING_OFFER_LIFETIME: Duration = Duration::from_secs(60);
/// 每次发送有界等待，失败或中断后绝不在同一socket重发半帧。
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// 服务轮失败后的首次重试等待；之后指数退避，持续失败也不会停止广播尝试。
const SERVER_RETRY_BASE: Duration = Duration::from_secs(1);
/// 退避上限保证网络或权限恢复后半分钟内重新可被发现。
const SERVER_RETRY_MAX: Duration = Duration::from_secs(30);
/// 一轮稳定运行超过此时长后视为已恢复，下次失败重新从首档退避。
const SERVER_RETRY_RESET_AFTER: Duration = Duration::from_secs(60);
/// accept瞬时失败（连接中止、句柄暂满）只短暂让出，不拆除广播与已认证连接。
const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);

/// 桌面用户选择候选后才传递四位码，任务完成结果仍由原有文件提交入口消费。
pub(super) struct PairingCommand { pub(super) code: String, pub(super) client: String, pub(super) result: oneshot::Sender<Result<VerifiedPairing, &'static str>> }
/// 临时介绍只持有实际连接地址与一次性命令槽，不保存配对码。
pub(super) struct PendingOffer { pub(super) target: DiscoveredDevice, pub(super) expires: Instant, pub(super) command: oneshot::Sender<PairingCommand>, pub(super) ticket: u64 }
/// 活动连接只由精确租约撤销；旧连接退出不能删除新连接的取消入口。
pub(super) struct ActiveSession { pub(super) lease: PresenceLease, pub(super) cancel: watch::Sender<bool> }
/// 监听状态与连接表受Devices短锁拥有，所有网络等待都在锁外。
#[derive(Default)]
pub(super) struct ServerState {
    pub(super) offers: BTreeMap<String, PendingOffer>, pub(super) sessions: BTreeMap<String, ActiveSession>,
    pub(super) cancel: Option<watch::Sender<bool>>, pub(super) restart: bool, ticket: u64,
    /// 当前进程曾请求应用服务，配对落盘后可恢复被中断的启动轮。
    pub(super) requested: bool,
    /// 服务轮次身份耗尽时停止启动，不能回绕复用旧代次。
    pub(super) epoch: u64,
    /// 有界HTTP请求等待期间唤醒发布器，不伪造在线TCP会话。
    pub(super) widget_readers: usize,
    /// 每设备保留有界近期请求nonce，重复网络请求不重复触发统计扫描。
    pub(super) widget_nonces: BTreeMap<String, std::collections::VecDeque<String>>,
}

impl Devices {
    /// 应用启动或页面重试确保唯一数据服务存在，重复请求只预约一次重启。
    pub(super) fn start_server(self: &Arc<Self>) -> Result<(), &'static str> {
        self.start_server_for(None)
    }

    /// 锁外重启须持原取消代次重新预约，离页后的旧令牌保持零监听、零广播。
    fn start_server_for(self: &Arc<Self>, expected_epoch: Option<u64>) -> Result<(), &'static str> {
        let (cancel, mut cancelled) = watch::channel(false);
        {
            let mut state = self.state();
            if expected_epoch.is_some_and(|epoch| epoch != state.server.epoch) { return Ok(()); }
            if state.server.epoch == u64::MAX { return Err("device_link_unavailable"); }
            state.server.requested = true;
            if let Some(running) = &state.server.cancel {
                if *running.borrow() { state.server.restart = true; } return Ok(());
            }
            state.server.cancel = Some(cancel); state.server.restart = false; state.view.link_error = None;
        }
        let this = self.clone();
        if let Err(error) = self.owner.spawn("device-data-server", move |mut shutdown| async move {
            let mut failures = 0;
            let result = loop {
                let started = Instant::now();
                let result = this.run_server(&mut shutdown, &mut cancelled).await;
                let Err(error) = result else { break result; };
                if shutdown.is_cancelled() || *cancelled.borrow() { break result; }
                // 失败轮保留同一取消令牌退避重建监听与广播，期间重复启动请求不会派生第二个服务。
                if started.elapsed() >= SERVER_RETRY_RESET_AFTER { failures = 0; }
                failures += 1;
                this.reset_server_round(Some(error));
                tokio::select! {
                    biased;
                    _ = shutdown.cancelled() => break Ok(()),
                    _ = cancelled.changed() => break Ok(()),
                    _ = tokio::time::sleep(server_retry_delay(failures)) => {},
                }
            };
            let restart = this.finish_server(result);
            if let Some(epoch) = restart && !shutdown.is_cancelled() { let _ = this.start_server_for(Some(epoch)); }
        }) {
            let mut state = self.state(); state.server.cancel = None; state.view.link_error = Some(error); return Err(error);
        } Ok(())
    }

    /// 监听和子连接终态后清理当前投影，只返回带代次的重启意图。
    fn finish_server(&self, result: Result<(), &'static str>) -> Option<u64> {
        self.reset_server_round(result.err());
        let mut state = self.state(); state.server.cancel = None;
        std::mem::take(&mut state.server.restart).then_some(state.server.epoch)
    }

    /// 单轮服务退出后撤销被中止连接的投影；重试轮沿用取消令牌，错误留给页面披露。
    fn reset_server_round(&self, error: Option<&'static str>) {
        let mut state = self.state(); state.view.listening_port = None;
        state.server.offers.clear();
        for session in std::mem::take(&mut state.server.sessions).into_values() {
            let _ = session.cancel.send(true);
            let _ = state.presence.disconnected(&session.lease, UnavailableReason::Disconnected);
        }
        if let Some(error) = error { state.view.link_error = Some(error); }
    }

    /// 应用启动加载私有账本并接受首个客户端请求，不依赖设备页或已有配对。
    pub(in crate::usage_service) fn start_application_server(self: &Arc<Self>) -> Result<(), &'static str> {
        let this = self.clone();
        self.owner.spawn_blocking("device-resume", move |shutdown| {
            if shutdown.is_cancelled() { return; }
            match this.store.load() {
                Ok(book) => {
                    this.publish_startup_book(book);
                    if !shutdown.is_cancelled() { let _ = this.start_server(); }
                },
                Err(error) => {
                    {
                        let mut state = this.state();
                        if state.book.is_none() && !state.view.pending { state.view.link_error = Some(error); }
                    }
                    // 账本暂不可读也交给服务退避重试，存储恢复后无需用户进入设备页即可重新广播。
                    if !shutdown.is_cancelled() { let _ = this.start_server(); }
                },
            }
        })
    }

    /// 启动只读快照只能填充未初始化状态，绝不覆盖用户更新或清除正在进行的写入。
    fn publish_startup_book(&self, book: loki_metris_core::devices::DeviceBook) -> bool {
        let mut state = self.state();
        if state.book.is_none() && !state.view.pending && !state.view.loaded {
            state.view.loaded = true; state.view.saved = book.summaries(); state.book = Some(book);
        }
        state.book.as_ref().is_some_and(|book| !book.summaries().is_empty())
    }

    /// 初始化I/O、广播和监听在同一任务；全部连接由JoinSet有界拥有并在退出时回收。
    async fn run_server(self: &Arc<Self>, shutdown: &mut BackgroundTaskShutdown, cancelled: &mut watch::Receiver<bool>) -> Result<(), &'static str> {
        let book = self.disk(|store| store.prepare_service()).await?;
        if shutdown.is_cancelled() || *cancelled.borrow() { return Ok(()); }
        let listener = TcpListener::bind("0.0.0.0:0").await.map_err(|_| "device_link_unavailable")?;
        let port = listener.local_addr().map_err(|_| "device_link_unavailable")?.port();
        super::discovery::cleanup_mdns(&self.host_daemon).await?;
        let daemon = mdns_sd::ServiceDaemon::new().map_err(|_| "device_discovery_failed")?;
        *self.host_daemon.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(daemon.clone());
        let mut connections = JoinSet::new();
        let result = async {
            // 首版监听IPv4；不发布无法接受连接的IPv6或环回端点。
            daemon.disable_interface(vec![mdns_sd::IfKind::IPv6, mdns_sd::IfKind::LoopbackV4]).map_err(|_| "device_discovery_failed")?;
            let instance = self.instance_name(&book); let host = format!("loki-{}.local.", book.client_id());
            let properties = [("v", "1"), ("id", book.client_id()), ("name", instance.as_str())];
            let info = mdns_sd::ServiceInfo::new(HOST_SERVICE_TYPE, &instance, &host, "", port, properties.as_slice())
                .map_err(|_| "device_discovery_failed")?.enable_addr_auto();
            let monitor = daemon.monitor().map_err(|_| "device_discovery_failed")?;
            daemon.register(info).map_err(|_| "device_discovery_failed")?;
            { let mut state = self.state(); state.book.get_or_insert(book); state.view.listening_port = Some(port); state.view.link_error = None; }
            let pending = Arc::new(Semaphore::new(MAX_PENDING_CONNECTIONS));
            let publisher = self.clone(); let publisher_cancelled = cancelled.clone();
            connections.spawn(async move { publisher.publish_loop(publisher_cancelled).await; });
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown.cancelled() => break,
                    _ = cancelled.changed() => break,
                    event = monitor.recv_async() => {
                        if matches!(event, Ok(mdns_sd::DaemonEvent::Error(_)) | Err(_)) { return Err("device_discovery_failed"); }
                    },
                    result = connections.join_next(), if !connections.is_empty() => {
                        if matches!(result, Some(Err(_))) { return Err("device_link_unavailable"); }
                    },
                    accepted = listener.accept() => {
                        let Ok((stream, peer)) = accepted else { tokio::time::sleep(ACCEPT_RETRY_DELAY).await; continue; };
                        if !is_lan_endpoint(peer) || connections.len() >= MAX_SAVED_DEVICES + MAX_PENDING_CONNECTIONS { continue; }
                        let Ok(permit) = pending.clone().try_acquire_owned() else { continue; };
                        let this = self.clone(); let cancellation = cancelled.clone();
                        connections.spawn(async move { this.serve_peer(stream, peer, permit, cancellation).await; });
                    },
                }
            } Ok(())
        }.await;
        drop(listener); self.state().view.listening_port = None;
        connections.shutdown().await;
        let cleanup = super::discovery::cleanup_mdns(&self.host_daemon).await;
        result.and(cleanup)
    }

    /// 首帧也受15秒总预算及未认证并发槽约束，所有错误路径销毁整个socket。
    pub(super) async fn serve_peer(self: Arc<Self>, mut stream: TcpStream, peer: SocketAddr, permit: OwnedSemaphorePermit, mut cancelled: watch::Receiver<bool>) {
        if *cancelled.borrow() { return; }
        let deadline = tokio::time::Instant::now() + link_transport::AUTHENTICATION_TIMEOUT;
        // HTTP与原有有界帧复用同一实际监听端口；不新增广告或任意URL入口。
        let mut prefix = [0; 1];
        let peek = tokio::select! {
            biased;
            _ = cancelled.changed() => return,
            result = tokio::time::timeout_at(deadline, stream.peek(&mut prefix)) => result,
        };
        if !matches!(peek, Ok(Ok(1))) { return; }
        if prefix[0] == b'P' {
            tokio::select! {
                biased;
                _ = cancelled.changed() => {},
                _ = tokio::time::timeout(Duration::from_secs(5), self.serve_widget_http(&mut stream)) => {},
            }
            return;
        }
        let introduction = tokio::select! {
            biased;
            _ = cancelled.changed() => return,
            result = tokio::time::timeout_at(deadline, transport::read_frame::<ConnectionIntroduction>(&mut stream)) => match result { Ok(Ok(value)) => value, _ => return },
        };
        match introduction {
            ConnectionIntroduction::Pairing(offer) => { self.serve_offer(&mut stream, peer, offer, &mut cancelled).await; },
            ConnectionIntroduction::Resume(hello) => {
                let Some(book) = self.state().book.clone() else { return; };
                let result = tokio::select! {
                    biased;
                    _ = cancelled.changed() => return,
                    result = tokio::time::timeout_at(deadline, link_transport::authenticate(&mut stream, &book, hello)) => result,
                };
                let Ok(Ok(mut authenticated)) = result else { return; };
                let (cancel, cancellation) = watch::channel(false);
                let lease = {
                    let mut state = self.state(); let Some(book) = state.book.clone() else { return; };
                    let Ok(lease) = state.presence.connected(&book, &mut authenticated.session, authenticated.info, peer, Instant::now()) else { return; };
                    if let Some(old) = state.server.sessions.insert(lease.device_id().into(), ActiveSession { lease: lease.clone(), cancel }) { let _ = old.cancel.send(true); }
                    lease
                };
                drop(permit);
                self.publication_refresh.notify_one();
                let reason = self.run_session(stream, &lease, authenticated.session, cancelled, cancellation).await;
                let mut state = self.state(); let _ = state.presence.disconnected(&lease, reason);
                if state.server.sessions.get(lease.device_id()).is_some_and(|active| active.lease == lease) { state.server.sessions.remove(lease.device_id()); }
            },
        }
    }

    /// 临时介绍等待弹窗命令；客户端断开或提前发送协议数据立即撤销请求。
    async fn serve_offer(&self, stream: &mut TcpStream, peer: SocketAddr, offer: PairingOffer, cancelled: &mut watch::Receiver<bool>) {
        let (command, received) = oneshot::channel();
        let (ticket, target) = {
            let mut state = self.state(); let Some(book) = &state.book else { return; };
            if offer.validate_for(book.client_id()).is_err() || state.server.offers.contains_key(&offer.device_id)
                || book.summaries().iter().any(|saved| saved.id == offer.device_id) { return; }
            let Ok(target) = DiscoveredDevice::new(offer.device_id, offer.name, vec![peer]) else { return; };
            let target = target.with_pairing_protocol(Some("1"));
            let Some(ticket) = state.server.ticket.checked_add(1) else { return; }; state.server.ticket = ticket;
            state.server.offers.insert(target.id.clone(), PendingOffer { target: target.clone(), expires: Instant::now() + PAIRING_OFFER_LIFETIME, command, ticket });
            (ticket, target)
        };
        let mut unexpected = [0u8; 1];
        let received = tokio::select! {
            biased;
            _ = cancelled.changed() => None,
            result = tokio::time::timeout(PAIRING_OFFER_LIFETIME, received) => result.ok().and_then(Result::ok),
            _ = stream.peek(&mut unexpected) => None,
        };
        {
            let mut state = self.state();
            if state.server.offers.get(&target.id).is_some_and(|offer| offer.ticket == ticket) { state.server.offers.remove(&target.id); }
        }
        let Some(mut command) = received else { return; };
        let result = tokio::select! {
            biased;
            _ = cancelled.changed() => Err("device_cancelled"),
            _ = command.result.closed() => return,
            result = tokio::time::timeout(Duration::from_secs(15), transport::pair_stream(stream, &target, &command.client, &command.code)) => result.unwrap_or(Err("device_timeout")),
        };
        let _ = command.result.send(result);
    }

    /// 单owner驱动读写；认证包/语义/超时/取消任一失败均退出并销毁读写两半socket。
    async fn run_session(&self, stream: TcpStream, lease: &PresenceLease, mut session: loki_metris_core::devices::link::AuthenticatedLinkSession,
        mut server_cancelled: watch::Receiver<bool>, mut cancelled: watch::Receiver<bool>) -> UnavailableReason {
        let (mut reader, mut writer) = stream.into_split(); let mut framing = FrameReader::new(MAX_ENCRYPTED_FRAME_BYTES);
        if *server_cancelled.borrow() || *cancelled.borrow() { return UnavailableReason::Disconnected; }
        let mut heartbeat = tokio::time::interval(DEVICE_HEARTBEAT_INTERVAL); heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut nonce = 0u64; let mut outstanding = None; let mut recent_pings = std::collections::VecDeque::new();
        let mut last_snapshot_request: Option<Instant> = None;
        let mut publications = self.publications.subscribe();
        let port = writer.as_ref().local_addr().ok().map(|value| value.port());
        let first = self.with_widget_endpoint(publications.borrow_and_update().message(super::super::current_epoch_ms()), lease.device_id(), port);
        if !matches!(tokio::time::timeout(WRITE_TIMEOUT, link_transport::send(&mut writer, &mut session, &first)).await, Ok(Ok(()))) { return UnavailableReason::Disconnected; }
        loop {
            tokio::select! {
                biased;
                _ = server_cancelled.changed() => break,
                _ = cancelled.changed() => break,
                _ = heartbeat.tick() => {
                    {
                        let mut state = self.state(); let Some(book) = state.book.clone() else { break; };
                        let expired = state.presence.expire(&book, Instant::now());
                        for expired in &expired {
                            if let Some(active) = state.server.sessions.get(expired.device_id()) && &active.lease == expired { let _ = active.cancel.send(true); }
                        }
                        if expired.iter().any(|expired| expired == lease) { return UnavailableReason::HeartbeatTimeout; }
                        if state.server.sessions.get(lease.device_id()).is_none_or(|active| &active.lease != lease) { break; }
                    }
                    if outstanding.is_none() {
                        let ping = LinkControl::Ping { nonce };
                        let sent_at = Instant::now();
                        if !matches!(tokio::time::timeout(WRITE_TIMEOUT, link_transport::send(&mut writer, &mut session, &ping)).await, Ok(Ok(()))) { break; }
                        recent_pings.push_back((nonce, sent_at));
                        if recent_pings.len() > 4 { recent_pings.pop_front(); }
                        outstanding = Some(nonce); let Some(next) = nonce.checked_add(1) else { break; }; nonce = next;
                    }
                },
                result = publications.changed() => {
                    if result.is_err() { break; }
                    let message = self.with_widget_endpoint(publications.borrow_and_update().message(super::super::current_epoch_ms()), lease.device_id(), port);
                    if !matches!(tokio::time::timeout(WRITE_TIMEOUT, link_transport::send(&mut writer, &mut session, &message)).await, Ok(Ok(()))) { break; }
                },
                result = framing.read::<EncryptedPacket>(&mut reader) => {
                    let Ok(packet) = result else { break; };
                    let Ok(plaintext) = session.open(&packet) else { break; };
                    let Ok(message) = serde_json::from_slice::<loki_metris_core::devices::link::data::DeviceMessage>(&plaintext) else { break; };
                    {
                        let mut state = self.state(); let Some(book) = state.book.clone() else { break; };
                        if state.presence.received(&book, lease, Instant::now()).is_err() { break; }
                    }
                    let control = match message {
                        loki_metris_core::devices::link::data::DeviceMessage::SnapshotRequest {} => {
                            // 每连接至多每秒唤醒一次；额度继续复用五分钟节流，不由远端强制连续请求上游。
                            if last_snapshot_request.is_none_or(|at| at.elapsed() >= Duration::from_secs(1)) {
                                last_snapshot_request = Some(Instant::now());
                                self.publication_refresh.notify_one();
                                let response = self.with_widget_endpoint(publications.borrow().message(super::super::current_epoch_ms()), lease.device_id(), port);
                                if !matches!(tokio::time::timeout(WRITE_TIMEOUT, link_transport::send(&mut writer, &mut session, &response)).await, Ok(Ok(()))) { break; }
                            }
                            continue;
                        },
                        loki_metris_core::devices::link::data::DeviceMessage::DeviceFormFactor { form_factor } => {
                            let mut state = self.state(); let Some(book) = state.book.clone() else { break; };
                            if state.presence.report_form_factor(&book, lease, form_factor, Instant::now()).is_err() { break; }
                            continue;
                        },
                        loki_metris_core::devices::link::data::DeviceMessage::Control(control) => control,
                        loki_metris_core::devices::link::data::DeviceMessage::CompanionStatus { nonce, report } => {
                            // 同时在途的上一轮状态可引用旧ping，但仍受当前租约及45秒时间锚约束。
                            let Some((_, anchor)) = recent_pings.iter().find(|(expected, _)| *expected == nonce) else { break; };
                            let anchor = *anchor;
                            let mut state = self.state(); let Some(book) = state.book.clone() else { break; };
                            if state.presence.companion(&book, lease, report, anchor, Instant::now()).is_err() { break; }
                            continue;
                        },
                        loki_metris_core::devices::link::data::DeviceMessage::AssetRequest { hash, offset } => {
                            let response = publications.borrow().asset(&hash, offset);
                            let Ok(response) = response else { break; };
                            if !matches!(tokio::time::timeout(WRITE_TIMEOUT, link_transport::send(&mut writer, &mut session, &response)).await, Ok(Ok(()))) { break; }
                            continue;
                        },
                    };
                    match control {
                        LinkControl::Pause {} => return UnavailableReason::Background,
                        LinkControl::Pong { nonce } => { if outstanding != Some(nonce) { break; } outstanding = None; },
                        LinkControl::Ping { nonce } => {
                            if !matches!(tokio::time::timeout(WRITE_TIMEOUT, link_transport::send(&mut writer, &mut session, &LinkControl::Pong { nonce })).await, Ok(Ok(()))) { break; }
                        },
                    }
                },
            }
        } UnavailableReason::Disconnected
    }
}

impl Devices {
    /// 元数据只在已认证会话中附加，并从当前账本派生读取权限。
    fn with_widget_endpoint(&self, mut message: loki_metris_core::devices::link::data::HostMessage, device: &str, port: Option<u16>) -> loki_metris_core::devices::link::data::HostMessage {
        if let loki_metris_core::devices::link::data::HostMessage::Snapshot { widget_endpoint, capabilities, .. } = &mut message {
            *widget_endpoint = port.and_then(|port| self.state().book.as_ref()?.widget_endpoint(device, port).ok());
            if widget_endpoint.is_some() { capabilities.push("widgetHttpV1".into()); }
        }
        message
    }
}

/// 第N次连续失败的重试等待：1、2、4、8、16秒后封顶30秒。
fn server_retry_delay(failures: u32) -> Duration {
    SERVER_RETRY_BASE.saturating_mul(1 << failures.saturating_sub(1).min(5)).min(SERVER_RETRY_MAX)
}
