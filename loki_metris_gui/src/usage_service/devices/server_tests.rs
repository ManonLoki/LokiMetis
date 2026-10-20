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

/// 重启意图取出后再次离页，原代次不能在锁外重新建立监听或广播。
#[tokio::test]
async fn cancelled_restart_epoch_cannot_reopen_empty_device_service() {
    let temp = tempfile::tempdir().unwrap();
    let owner = Arc::new(crate::usage_service::background::BackgroundTaskOwner::with_runtime(
        Arc::new(loki_metris_core::RootDiscoveryCoordinator::default()), tokio::runtime::Handle::current()));
    let devices = Arc::new(Devices::new(temp.path(), owner.clone()));
    devices.finish(Ok(loki_metris_core::devices::DeviceBook::new(CLIENT.into()).unwrap()), None);
    { let mut state = devices.state(); state.server.restart = true; }
    let epoch = devices.finish_server(Ok(())).unwrap(); devices.cancel();
    devices.start_server_for(Some(epoch)).unwrap();
    assert!(devices.state().server.cancel.is_none()); assert!(devices.snapshot().listening_port.is_none());
    assert_eq!(owner.owned_task_count(), 0); assert!(!temp.path().join("devices").exists()); owner.shutdown().await;
}

/// 终端主动连接报到后，桌面原配对按钮仍执行四帧验证并真实落盘；错码不增加设备。
#[tokio::test]
async fn incoming_pairing_offer_uses_existing_pairing_and_private_commit() {
    use loki_metris_core::devices::{PairingHello, PairingConfirmation, pairing_test_reply};
    for (code, success) in [("0007", true), ("0008", false)] {
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
            if let Ok(ack) = confirm(confirmation) { transport::write_frame(&mut stream, &ack).await.unwrap(); }
        };
        let ui = async {
            while !ui_devices.snapshot().discovered.iter().any(|device| device.id == DEVICE) { tokio::task::yield_now().await; }
            ui_devices.pair(DEVICE, code.into()).unwrap();
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
    let snapshot = DisplaySnapshot { version:1, assets:vec![descriptor.clone()], ..Default::default() };
    devices.publications.send_replace(Arc::new(super::super::publication::Publication { snapshot:snapshot.clone(), bytes:[(descriptor.hash.clone(),Arc::from(bytes.clone()))].into() }));
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
        let HostMessage::Snapshot { snapshot:first, capabilities } = application_message(&mut stream, &mut reader, &mut channel).await else { panic!("initial snapshot"); };
        assert_eq!(first.assets, vec![descriptor.clone()]);
        assert!(capabilities.iter().any(|value| value == "watchCompanionV1"));
        // 持续预发合法资源请求时，最新状态仍须优先得到服务，不能等请求流结束。
        for _ in 0..24 {
            let packet = channel.seal(&serde_json::to_vec(&DeviceMessage::AssetRequest { hash:descriptor.hash.clone(), offset:0 }).unwrap()).unwrap();
            write_frame(&mut stream, &packet, MAX_ENCRYPTED_FRAME_BYTES).await.unwrap();
        }
        let mut pipelined = snapshot.clone(); pipelined.token_error = Some("pipelined-update".into());
        publisher.publications.send_replace(Arc::new(super::super::publication::Publication { snapshot:pipelined, bytes:[(descriptor.hash.clone(),Arc::from(bytes.clone()))].into() }));
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
        let mut receiver = AssetReceiver::new(descriptor.clone()).unwrap();
        while receiver.offset() < descriptor.length {
            let request = DeviceMessage::AssetRequest { hash:descriptor.hash.clone(), offset:receiver.offset() };
            let packet = channel.seal(&serde_json::to_vec(&request).unwrap()).unwrap(); write_frame(&mut stream, &packet, MAX_ENCRYPTED_FRAME_BYTES).await.unwrap();
            let HostMessage::AssetChunk { chunk } = application_message(&mut stream, &mut reader, &mut channel).await else { panic!("image chunk"); };
            receiver.accept(&chunk).unwrap();
        }
        assert_eq!(receiver.finish().unwrap().1, bytes);
        let mut next = snapshot; next.catalog_error = Some("device_catalog_unavailable".into()); next.assets.clear();
        publisher.publications.send_replace(Arc::new(super::super::publication::Publication { snapshot:next, bytes:Default::default() }));
        let HostMessage::Snapshot { snapshot:updated, .. } = application_message(&mut stream, &mut reader, &mut channel).await else { panic!("updated snapshot"); };
        assert!(updated.assets.is_empty());
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
