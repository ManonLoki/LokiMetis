//! 连接owner回归只用真实环回TCP；私网peer是领域地址夹具，不冒充实物LAN测试。

use super::*;
use super::super::{tests::{CLIENT, DEVICE, link_book}, framing::write_frame};
use loki_metris_core::devices::{link::{DeviceSession, SessionReply, SecureChannel}, presence::DeviceConnectionState};
use tokio::io::AsyncReadExt;

/// 与真实桌面owner执行恢复握手，首条元信息也由真实密码通道封装。
async fn phone(stream: &mut TcpStream, metadata: serde_json::Value) -> SecureChannel {
    let (waiting, hello) = DeviceSession::start(CLIENT, DEVICE, &[0xa7;32], uuid::Uuid::new_v4().to_string()).unwrap();
    let introduction = ConnectionIntroduction::Resume(hello);
    transport::write_frame(stream, &introduction).await.unwrap();
    let reply: SessionReply = transport::read_frame(stream).await.unwrap();
    let (mut channel, confirmation) = waiting.accept(reply).unwrap();
    transport::write_frame(stream, &confirmation).await.unwrap();
    let packet = channel.seal(&serde_json::to_vec(&metadata).unwrap()).unwrap();
    write_frame(stream, &packet, MAX_ENCRYPTED_FRAME_BYTES).await.unwrap();
    channel
}

/// 当前连接成功、错误元信息、坏密文和取消都走真实owner，并证明socket关闭与状态一致。
#[tokio::test]
async fn connection_owner_closes_on_pause_metadata_failure_tamper_and_cancel() {
    for scenario in ["pause", "metadata-id", "metadata-ip", "tag", "cancel"] {
        let temp = tempfile::tempdir().unwrap();
        let owner = Arc::new(crate::usage_service::background::BackgroundTaskOwner::with_runtime(
            Arc::new(loki_metris_core::RootDiscoveryCoordinator::default()), tokio::runtime::Handle::current()));
        let devices = Arc::new(Devices::new(temp.path(), owner.clone())); devices.finish(Ok(link_book()), None);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap(); let address = listener.local_addr().unwrap();
        let permits = Arc::new(Semaphore::new(MAX_PENDING_CONNECTIONS));
        let (cancel, cancelled) = watch::channel(false); let desktop = devices.clone(); let phone_view = devices.clone();
        let server = async {
            let (stream, _) = listener.accept().await.unwrap();
            // 产品从listener.accept获取真实peer；这里显式注入私网领域夹具以保持网络隔离。
            desktop.serve_peer(stream, "192.168.1.20:51000".parse().unwrap(), permits.acquire_owned().await.unwrap(), cancelled).await;
        };
        let client = async {
            let mut stream = TcpStream::connect(address).await.unwrap();
            let mut metadata = serde_json::json!({"deviceId":DEVICE,"name":"Test phone","platform":"ios","appVersion":"1.0"});
            if scenario == "metadata-id" { metadata["deviceId"] = CLIENT.into(); }
            if scenario == "metadata-ip" { metadata["ip"] = "192.168.9.99".into(); }
            let mut channel = phone(&mut stream, metadata).await;
            if !scenario.starts_with("metadata") {
                let packet: EncryptedPacket = FrameReader::new(MAX_ENCRYPTED_FRAME_BYTES).read(&mut stream).await.unwrap();
                let snapshot: loki_metris_core::devices::link::data::HostMessage = serde_json::from_slice(&channel.open(&packet).unwrap()).unwrap();
                assert!(matches!(snapshot, loki_metris_core::devices::link::data::HostMessage::Snapshot { .. }));
                let packet: EncryptedPacket = FrameReader::new(MAX_ENCRYPTED_FRAME_BYTES).read(&mut stream).await.unwrap();
                let ping: LinkControl = serde_json::from_slice(&channel.open(&packet).unwrap()).unwrap();
                let LinkControl::Ping { nonce } = ping else { panic!("expected heartbeat"); };
                let view = phone_view.snapshot(); assert_eq!(view.presence[0].state, DeviceConnectionState::Reachable);
                assert_eq!(view.presence[0].peer, Some("192.168.1.20:51000".parse().unwrap()));
                if scenario == "cancel" { cancel.send(true).unwrap(); }
                else {
                    let control = if scenario == "pause" { LinkControl::Pause {} } else { LinkControl::Pong { nonce } };
                    let mut packet = channel.seal(&serde_json::to_vec(&control).unwrap()).unwrap();
                    if scenario == "tag" {
                        use base64::Engine as _;
                        let mut bytes = base64::engine::general_purpose::STANDARD.decode(&packet.ciphertext).unwrap();
                        *bytes.last_mut().unwrap() ^= 1; packet.ciphertext = base64::engine::general_purpose::STANDARD.encode(bytes);
                    }
                    write_frame(&mut stream, &packet, MAX_ENCRYPTED_FRAME_BYTES).await.unwrap();
                }
            }
            let mut byte = [0]; let closed = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut byte)).await.unwrap();
            assert!(matches!(closed, Ok(0) | Err(_)), "{scenario}");
        };
        tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(server, client); }).await.unwrap();
        let expected = if scenario.starts_with("metadata") { DeviceConnectionState::NotConnected }
            else if scenario == "pause" { DeviceConnectionState::Unavailable(UnavailableReason::Background) }
            else { DeviceConnectionState::Unavailable(UnavailableReason::Disconnected) };
        assert_eq!(devices.snapshot().presence[0].state, expected, "{scenario}");
        assert!(devices.state().server.sessions.is_empty()); owner.shutdown().await;
    }
}

/// 迟到的启动快照不能恢复已移除的信任，也不能清除用户操作的pending与取消入口。
#[tokio::test]
async fn stale_startup_snapshot_cannot_undo_removal_or_release_operation_gate() {
    let temp = tempfile::tempdir().unwrap();
    let owner = Arc::new(crate::usage_service::background::BackgroundTaskOwner::with_runtime(
        Arc::new(loki_metris_core::RootDiscoveryCoordinator::default()), tokio::runtime::Handle::current()));
    let devices = Arc::new(Devices::new(temp.path(), owner.clone())); let old = link_book();
    devices.finish(Ok(old.clone()), None);
    devices.finish(Ok(old.without(DEVICE).unwrap()), Some("removed"));
    devices.begin().unwrap(); let (cancel, _receiver) = watch::channel(false);
    { let mut state = devices.state(); state.view.pairing = true; state.pair_cancel = Some(cancel); }
    assert!(!devices.publish_startup_book(old));
    assert!(devices.snapshot().saved.is_empty()); assert!(devices.snapshot().pending); assert!(devices.snapshot().pairing);
    assert!(devices.state().pair_cancel.is_some());
    assert!(devices.state().book.as_ref().unwrap().summaries().is_empty()); owner.shutdown().await;
}

/// 取消握手不关闭应用监听；空账本也保留首次配对入口。
#[tokio::test]
async fn cancel_pairing_keeps_empty_device_service_alive() {
    let temp = tempfile::tempdir().unwrap();
    let owner = Arc::new(crate::usage_service::background::BackgroundTaskOwner::with_runtime(
        Arc::new(loki_metris_core::RootDiscoveryCoordinator::default()), tokio::runtime::Handle::current()));
    let devices = Arc::new(Devices::new(temp.path(), owner.clone()));
    let (server_cancel, server_cancelled) = watch::channel(false);
    let (pair_cancel, pair_cancelled) = watch::channel(false);
    { let mut state = devices.state(); state.server.cancel = Some(server_cancel); state.pair_cancel = Some(pair_cancel); }
    devices.cancel();
    assert!(!*server_cancelled.borrow()); assert!(*pair_cancelled.borrow());
    assert!(devices.state().server.cancel.is_some()); owner.shutdown().await;
}

/// 服务轮失败不终止广播职责：保留唯一令牌退避重试，退出仍能及时打断等待。
#[tokio::test]
async fn failed_service_round_keeps_retrying_until_shutdown() {
    let temp = tempfile::tempdir().unwrap();
    // 设备目录被普通文件占据，存储校验必然失败且不会触达真实网络。
    std::fs::write(temp.path().join("devices"), b"").unwrap();
    let owner = Arc::new(crate::usage_service::background::BackgroundTaskOwner::with_runtime(
        Arc::new(loki_metris_core::RootDiscoveryCoordinator::default()), tokio::runtime::Handle::current()));
    let devices = Arc::new(Devices::new(temp.path(), owner.clone()));
    devices.start_server().unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while devices.snapshot().link_error.is_none() { tokio::time::sleep(Duration::from_millis(10)).await; }
    }).await.unwrap();
    assert!(devices.state().server.cancel.is_some());
    devices.start_server().unwrap();
    assert!(!devices.state().server.restart);
    tokio::time::timeout(Duration::from_secs(5), owner.shutdown()).await.unwrap();
    assert!(devices.state().server.cancel.is_none());
}

/// 服务失败重试逐步退避并受最大延迟限制。
#[test]
fn service_retry_delay_backs_off_to_cap() {
    let delays: Vec<u64> = (1..=8).map(|failures| server_retry_delay(failures).as_secs()).collect();
    assert_eq!(delays, [1, 2, 4, 8, 16, 30, 30, 30]);
}

/// 只有墙钟明显多于单调时钟才判定睡眠；正常调度抖动和墙钟回拨都不触发广播重建。
#[test]
fn sleep_detection_requires_wall_clock_gap() {
    let start = (Instant::now(), std::time::SystemTime::now());
    let after = |monotonic: u64, wall: i64| (start.0 + Duration::from_secs(monotonic),
        if wall >= 0 { start.1 + Duration::from_secs(wall as u64) } else { start.1 - Duration::from_secs(wall.unsigned_abs()) });
    assert!(!system_slept(start, after(30, 30)));
    assert!(!system_slept(start, after(30, 40)));
    assert!(system_slept(start, after(30, 41)));
    assert!(system_slept(start, after(30, 3600)));
    assert!(!system_slept(start, after(30, -600)));
}

/// Windows 的 Instant 与墙钟同时前进仍能由原生睡眠累计量发现唤醒，无需已有配对设备。
#[test]
fn windows_sleep_is_detected_when_both_clocks_advance() {
    use super::super::discovery::suspend_advanced;
    let start = (Instant::now(), std::time::SystemTime::now());
    assert!(!system_slept(start, (start.0 + Duration::from_secs(90), start.1 + Duration::from_secs(90))));
    assert!(suspend_advanced(Some(Duration::from_secs(100)), Some(Duration::from_secs(160))));
    assert!(!suspend_advanced(Some(Duration::from_secs(100)), Some(Duration::from_secs(110))));
    assert!(!suspend_advanced(Some(Duration::from_secs(100)), Some(Duration::from_secs(90))));
    assert!(!suspend_advanced(None, Some(Duration::from_secs(160))));
    assert!(!suspend_advanced(Some(Duration::from_secs(100)), None));
}

/// 握手完成时撤销同ID的排队连接，避免成功后又弹出重复配对请求。
#[tokio::test]
async fn saved_device_commit_rejects_duplicate_queued_request() {
    let temp = tempfile::tempdir().unwrap();
    let owner = Arc::new(crate::usage_service::background::BackgroundTaskOwner::with_runtime(
        Arc::new(loki_metris_core::RootDiscoveryCoordinator::default()), tokio::runtime::Handle::current()));
    let devices = Arc::new(Devices::new(temp.path(), owner.clone()));
    let (command, received) = oneshot::channel();
    let target = DiscoveredDevice::new(DEVICE.into(), "Duplicate phone".into(), vec!["192.168.1.21:51002".parse().unwrap()]).unwrap();
    devices.state().server.offers.insert(DEVICE.into(), PendingOffer { target, expires: Instant::now() + PAIRING_OFFER_LIFETIME, command, ticket: 2 });
    devices.finish(Ok(link_book()), Some("paired"));
    assert!(devices.snapshot().requests.is_empty()); assert!(received.await.is_err());
    owner.shutdown().await;
}

/// 入站请求完成四帧认证后才真实落盘；错码或缺少最终认证回执均不增加设备。
#[tokio::test]
async fn incoming_pairing_offer_uses_existing_pairing_and_private_commit() {
    use loki_metris_core::devices::{PairingHello, PairingConfirmation, pairing_test_reply};
    for (code, acknowledge, success) in [("0007", true, true), ("0008", true, false), ("0007", false, false)] {
        let temp = tempfile::tempdir().unwrap();
        let owner = Arc::new(crate::usage_service::background::BackgroundTaskOwner::with_runtime(
            Arc::new(loki_metris_core::RootDiscoveryCoordinator::default()), tokio::runtime::Handle::current()));
        let devices = Arc::new(Devices::new(temp.path(), owner.clone()));
        let initial = devices.store.load().unwrap(); let client_id = initial.client_id().to_owned(); devices.finish(Ok(initial), None);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap(); let address = listener.local_addr().unwrap();
        let permits = Arc::new(Semaphore::new(MAX_PENDING_CONNECTIONS)); let (_cancel, cancelled) = watch::channel(false);
        let desktop = devices.clone(); let ui_devices = devices.clone();
        let server = async {
            let (stream, _) = listener.accept().await.unwrap();
            desktop.serve_peer(stream, "192.168.1.21:51001".parse().unwrap(), permits.acquire_owned().await.unwrap(), cancelled).await;
        };
        let phone = async {
            let mut stream = TcpStream::connect(address).await.unwrap();
            transport::write_frame(&mut stream, &ConnectionIntroduction::Pairing(PairingOffer { version:1, client_id, device_id:DEVICE.into(), name:"Pairing phone".into() })).await.unwrap();
            let hello: PairingHello = transport::read_frame(&mut stream).await.unwrap();
            let (reply, confirm) = pairing_test_reply(&hello, "0007"); transport::write_frame(&mut stream, &reply).await.unwrap();
            let confirmation: PairingConfirmation = transport::read_frame(&mut stream).await.unwrap();
            if let Ok(ack) = confirm(confirmation) && acknowledge { transport::write_frame(&mut stream, &ack).await.unwrap(); }
        };
        let ui = async {
            while ui_devices.snapshot().requests.is_empty() { tokio::task::yield_now().await; }
            let request = ui_devices.snapshot().requests[0].clone();
            assert_eq!(request.device.name, "Pairing phone");
            assert_eq!(request.device.endpoints, vec!["192.168.1.21:51001".parse().unwrap()]);
            ui_devices.pair_target(DEVICE, request.ticket, code.into()).unwrap();
            while ui_devices.snapshot().pending { tokio::task::yield_now().await; }
        };
        tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(server, phone, ui); }).await.unwrap();
        assert_eq!(devices.snapshot().saved.len(), usize::from(success));
        let stored = loki_metris_core::devices::DeviceBook::decode(&std::fs::read(temp.path().join("devices/paired.json")).unwrap()).unwrap();
        assert_eq!(stored.summaries().len(), usize::from(success)); owner.shutdown().await;
    }
}

/// 从真实加密TCP取得初始及变更快照，逐块下载后校验正文，并验证未知摘要无法读取文件。
#[tokio::test]
async fn authenticated_session_pushes_latest_snapshot_and_serves_content_addressed_assets() {
    use loki_metris_core::devices::link::{assets::{AssetDescriptor, AssetReceiver}, data::{DeviceMessage, HostMessage, DisplaySnapshot}};
    let temp = tempfile::tempdir().unwrap();
    let owner = Arc::new(crate::usage_service::background::BackgroundTaskOwner::with_runtime(
        Arc::new(loki_metris_core::RootDiscoveryCoordinator::default()), tokio::runtime::Handle::current()));
    let devices = Arc::new(Devices::new(temp.path(), owner.clone())); devices.finish(Ok(link_book()), None);
    let mut bytes = b"GIF89a".to_vec(); bytes.resize(20_001, 7);
    let descriptor = AssetDescriptor::from_bytes(&bytes).unwrap();
    let quota = loki_metris_core::parse_provider_quota(loki_metris_core::QuotaProvider::Codex,
        r#"{"rate_limits":{"primary":{"used_percent":97,"window_minutes":10080}}}"#).unwrap();
    let snapshot = DisplaySnapshot { version:1, assets:vec![descriptor.clone()], enabled_agents:Some(vec![loki_metris_core::AiTool::Codex,loki_metris_core::AiTool::Cursor]),
        provider_quota: loki_metris_core::ProviderQuotaSnapshot::project(Some(123), &[quota]), ..Default::default() };
    devices.publications.send_replace(Arc::new(super::super::publication::Publication { refreshed_at:None, snapshot:snapshot.clone(), bytes:[(descriptor.hash.clone(),Arc::from(bytes.clone()))].into() }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap(); let address = listener.local_addr().unwrap();
    let permits = Arc::new(Semaphore::new(MAX_PENDING_CONNECTIONS)); let (_cancel, cancelled) = watch::channel(false);
    let desktop = devices.clone(); let publisher = devices.clone();
    let server = async {
        let (stream, _) = listener.accept().await.unwrap();
        desktop.serve_peer(stream, "192.168.1.20:51000".parse().unwrap(), permits.acquire_owned().await.unwrap(), cancelled).await;
    };
    let client = async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let mut channel = phone(&mut stream, serde_json::json!({"deviceId":DEVICE,"name":"Test phone","platform":"ios","appVersion":"1.0"})).await;
        let mut reader = FrameReader::new(MAX_ENCRYPTED_FRAME_BYTES);
        let HostMessage::Snapshot { snapshot:first, capabilities, .. } = application_message(&mut stream, &mut reader, &mut channel).await else { panic!("initial snapshot"); };
        assert_eq!(first.assets, vec![descriptor.clone()]);
        assert!(capabilities.iter().any(|value| value == "watchCompanionV1"));
        assert!(capabilities.iter().any(|value| value == "deviceFormFactorV1"));
        assert!(capabilities.iter().any(|value| value == "snapshotQueryV1"));
        assert_eq!(first.provider_quota, snapshot.provider_quota);
        assert_eq!(first.enabled_agents, snapshot.enabled_agents);
        let packet = channel.seal(&serde_json::to_vec(&DeviceMessage::SnapshotRequest {}).unwrap()).unwrap();
        write_frame(&mut stream, &packet, MAX_ENCRYPTED_FRAME_BYTES).await.unwrap();
        let HostMessage::Snapshot { snapshot:queried, .. } = application_message(&mut stream, &mut reader, &mut channel).await else { panic!("queried snapshot"); };
        assert_eq!(queried.provider_quota, snapshot.provider_quota);
        assert_eq!(queried.enabled_agents, snapshot.enabled_agents);
        let report = DeviceMessage::DeviceFormFactor { form_factor: loki_metris_core::devices::link::DeviceFormFactor::Tablet };
        let packet = channel.seal(&serde_json::to_vec(&report).unwrap()).unwrap();
        write_frame(&mut stream, &packet, MAX_ENCRYPTED_FRAME_BYTES).await.unwrap();
        // 持续预发合法资源请求时，最新状态仍须优先得到服务，不能等请求流结束。
        for _ in 0..24 {
            let packet = channel.seal(&serde_json::to_vec(&DeviceMessage::AssetRequest { hash:descriptor.hash.clone(), offset:0 }).unwrap()).unwrap();
            write_frame(&mut stream, &packet, MAX_ENCRYPTED_FRAME_BYTES).await.unwrap();
        }
        let mut pipelined = snapshot.clone(); pipelined.token_error = Some("pipelined-update".into());
        publisher.publications.send_replace(Arc::new(super::super::publication::Publication { refreshed_at:None, snapshot:pipelined, bytes:[(descriptor.hash.clone(),Arc::from(bytes.clone()))].into() }));
        let mut saw_update = false;
        let mut chunks_before_update = 0;
        for _ in 0..25 {
            match application_message(&mut stream, &mut reader, &mut channel).await {
                HostMessage::Snapshot { snapshot, .. } => { assert_eq!(snapshot.token_error.as_deref(),Some("pipelined-update")); saw_update=true; },
                HostMessage::AssetChunk { .. } => { if !saw_update { chunks_before_update += 1; } },
                _ => panic!("unexpected response"),
            }
        }
        assert!(saw_update); assert!(chunks_before_update < 24, "状态发布不能排在所有ready请求之后");
        assert_eq!(publisher.snapshot().presence[0].form_factor, Some(loki_metris_core::devices::link::DeviceFormFactor::Tablet));
        let mut receiver = AssetReceiver::new(descriptor.clone()).unwrap();
        while receiver.offset() < descriptor.length {
            let request = DeviceMessage::AssetRequest { hash:descriptor.hash.clone(), offset:receiver.offset() };
            let packet = channel.seal(&serde_json::to_vec(&request).unwrap()).unwrap(); write_frame(&mut stream, &packet, MAX_ENCRYPTED_FRAME_BYTES).await.unwrap();
            let HostMessage::AssetChunk { chunk } = application_message(&mut stream, &mut reader, &mut channel).await else { panic!("image chunk"); };
            receiver.accept(&chunk).unwrap();
        }
        assert_eq!(receiver.finish().unwrap().1, bytes);
        let mut next = snapshot; next.catalog_error = Some("device_catalog_unavailable".into()); next.assets.clear(); next.enabled_agents = Some(vec![]);
        publisher.publications.send_replace(Arc::new(super::super::publication::Publication { refreshed_at:None, snapshot:next, bytes:Default::default() }));
        let HostMessage::Snapshot { snapshot:updated, .. } = application_message(&mut stream, &mut reader, &mut channel).await else { panic!("updated snapshot"); };
        assert!(updated.assets.is_empty());
        assert_eq!(updated.enabled_agents, Some(vec![]));
        let packet = channel.seal(&serde_json::to_vec(&DeviceMessage::AssetRequest { hash:descriptor.hash.clone(), offset:0 }).unwrap()).unwrap();
        write_frame(&mut stream, &packet, MAX_ENCRYPTED_FRAME_BYTES).await.unwrap();
        assert!(matches!(application_message(&mut stream, &mut reader, &mut channel).await, HostMessage::AssetUnavailable { .. }));
        let packet = channel.seal(&serde_json::to_vec(&LinkControl::Pause {}).unwrap()).unwrap(); write_frame(&mut stream, &packet, MAX_ENCRYPTED_FRAME_BYTES).await.unwrap();
    };
    tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(server, client); }).await.unwrap();
    assert_eq!(devices.snapshot().presence[0].state, DeviceConnectionState::Unavailable(UnavailableReason::Background));
    owner.shutdown().await;
}

/// 测试终端处理真实心跳后继续消费应用帧，不依赖心跳与快照的调度顺序。
async fn application_message(stream: &mut TcpStream, reader: &mut FrameReader, channel: &mut SecureChannel) -> loki_metris_core::devices::link::data::HostMessage {
    loop {
        let packet: EncryptedPacket = reader.read(stream).await.unwrap();
        let plain = channel.open(&packet).unwrap();
        if let Ok(LinkControl::Ping { nonce }) = serde_json::from_slice::<LinkControl>(&plain) {
            let packet = channel.seal(&serde_json::to_vec(&LinkControl::Pong { nonce }).unwrap()).unwrap();
            write_frame(stream, &packet, MAX_ENCRYPTED_FRAME_BYTES).await.unwrap(); continue;
        }
        return serde_json::from_slice(&plain).unwrap();
    }
}

/// 真实加密TCP验证能力广告、心跳时间锚、iOS父资格及暂停撤销；不冒充模拟器WC互通。
#[tokio::test]
async fn companion_status_requires_authenticated_ios_parent_and_known_heartbeat_anchor() {
    use loki_metris_core::devices::{companion::{WatchCompanionReport, WatchInfo, CompanionObservation}, link::data::{DeviceMessage, HostMessage}};
    for scenario in ["valid", "unknown-anchor", "android"] {
        let temp = tempfile::tempdir().unwrap();
        let owner = Arc::new(crate::usage_service::background::BackgroundTaskOwner::with_runtime(
            Arc::new(loki_metris_core::RootDiscoveryCoordinator::default()), tokio::runtime::Handle::current()));
        let devices = Arc::new(Devices::new(temp.path(), owner.clone())); devices.finish(Ok(link_book()), None);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap(); let address = listener.local_addr().unwrap();
        let permits = Arc::new(Semaphore::new(MAX_PENDING_CONNECTIONS)); let (_cancel, cancelled) = watch::channel(false);
        let desktop = devices.clone(); let observer = devices.clone();
        let server = async {
            let (stream, _) = listener.accept().await.unwrap();
            desktop.serve_peer(stream, "192.168.1.20:51000".parse().unwrap(), permits.acquire_owned().await.unwrap(), cancelled).await;
        };
        let client = async {
            let mut stream = TcpStream::connect(address).await.unwrap();
            let mut channel = phone(&mut stream, serde_json::json!({"deviceId":DEVICE,"name":"Phone","platform":if scenario == "android" {"android"} else {"ios"},"appVersion":"1.0"})).await;
            let mut framing = FrameReader::new(MAX_ENCRYPTED_FRAME_BYTES);
            let packet: EncryptedPacket = framing.read(&mut stream).await.unwrap();
            let HostMessage::Snapshot { capabilities, .. } = serde_json::from_slice(&channel.open(&packet).unwrap()).unwrap() else { panic!("snapshot"); };
            assert!(capabilities.iter().any(|value| value == "watchCompanionV1"));
            let packet: EncryptedPacket = framing.read(&mut stream).await.unwrap();
            let LinkControl::Ping { nonce } = serde_json::from_slice(&channel.open(&packet).unwrap()).unwrap() else { panic!("ping"); };
            let report = WatchCompanionReport { info: WatchInfo { device_id:"c169da9b-4716-4774-bdcb-a33bfd6e1ac1".into(), name:"Watch".into(), app_version:"1.0".into() },
                reachable:true, contact:Some(CompanionObservation { sequence:1, age_ms:0 }), sync:Some(CompanionObservation { sequence:1, age_ms:0 }) };
            let message = DeviceMessage::CompanionStatus { nonce: if scenario == "unknown-anchor" { nonce + 1 } else { nonce }, report };
            let packet = channel.seal(&serde_json::to_vec(&message).unwrap()).unwrap();
            write_frame(&mut stream, &packet, MAX_ENCRYPTED_FRAME_BYTES).await.unwrap();
            if scenario == "valid" {
                // 顺序处理的ping回执证明前面的Watch状态已经由server应用。
                let packet = channel.seal(&serde_json::to_vec(&LinkControl::Ping { nonce:77 }).unwrap()).unwrap();
                write_frame(&mut stream, &packet, MAX_ENCRYPTED_FRAME_BYTES).await.unwrap();
                let packet: EncryptedPacket = framing.read(&mut stream).await.unwrap();
                assert!(matches!(serde_json::from_slice::<LinkControl>(&channel.open(&packet).unwrap()).unwrap(), LinkControl::Pong { nonce:77 }));
                assert!(observer.snapshot().presence[0].companion.as_ref().unwrap().reachable);
                let packet = channel.seal(&serde_json::to_vec(&LinkControl::Pause {}).unwrap()).unwrap();
                write_frame(&mut stream, &packet, MAX_ENCRYPTED_FRAME_BYTES).await.unwrap();
            }
            let mut byte = [0]; assert!(matches!(tokio::time::timeout(Duration::from_secs(2), stream.read(&mut byte)).await.unwrap(), Ok(0) | Err(_)));
        };
        tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(server, client); }).await.unwrap();
        let view = devices.snapshot();
        if scenario == "valid" { assert!(!view.presence[0].companion.as_ref().unwrap().reachable); }
        else { assert!(view.presence[0].companion.is_none()); }
        owner.shutdown().await;
    }
}

/// 当前请求取消、客户端断开或提前发帧均关闭socket并清除投影，旧票据不能拒绝新请求。
#[tokio::test]
async fn incoming_request_rejection_and_disconnect_release_exact_connection() {
    for scenario in ["reject", "disconnect", "unexpected-frame"] {
        let temp = tempfile::tempdir().unwrap();
        let owner = Arc::new(crate::usage_service::background::BackgroundTaskOwner::with_runtime(
            Arc::new(loki_metris_core::RootDiscoveryCoordinator::default()), tokio::runtime::Handle::current()));
        let devices = Arc::new(Devices::new(temp.path(), owner.clone()));
        let book = devices.store.load().unwrap(); let client_id = book.client_id().to_owned(); devices.finish(Ok(book), None);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap(); let address = listener.local_addr().unwrap();
        let permits = Arc::new(Semaphore::new(MAX_PENDING_CONNECTIONS)); let (_cancel, cancelled) = watch::channel(false);
        let desktop = devices.clone(); let observer = devices.clone();
        let server = async {
            let (stream, _) = listener.accept().await.unwrap();
            desktop.serve_peer(stream, "192.168.1.21:51001".parse().unwrap(), permits.acquire_owned().await.unwrap(), cancelled).await;
        };
        let client = async {
            let mut stream = TcpStream::connect(address).await.unwrap();
            transport::write_frame(&mut stream, &ConnectionIntroduction::Pairing(PairingOffer { version:1, client_id, device_id:DEVICE.into(), name:"Phone".into() })).await.unwrap();
            while observer.snapshot().requests.is_empty() { tokio::task::yield_now().await; }
            let ticket = observer.snapshot().requests[0].ticket;
            assert_eq!(observer.pair_target(DEVICE, ticket + 1, "0007".into()), Err("device_request_expired"));
            assert!(!observer.snapshot().pending);
            observer.reject_request(DEVICE, ticket + 1);
            assert_eq!(observer.snapshot().requests.len(), 1, "旧票据不能取消另一条连接");
            match scenario {
                "reject" => {
                    observer.reject_request(DEVICE, ticket);
                    let mut byte = [0]; assert_eq!(stream.read(&mut byte).await.unwrap(), 0);
                },
                "disconnect" => drop(stream),
                _ => {
                    use tokio::io::AsyncWriteExt;
                    stream.write_all(b"unexpected").await.unwrap();
                    let mut byte = [0]; assert!(matches!(stream.read(&mut byte).await, Ok(0) | Err(_)));
                },
            }
        };
        tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(server, client); }).await.unwrap();
        assert!(devices.snapshot().requests.is_empty()); assert!(devices.snapshot().saved.is_empty()); owner.shutdown().await;
    }
}
