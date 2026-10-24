//! 隔离设备回归使用真实TCP密码交换和私有临时目录，不依赖LAN或真实用户数据。

use super::*;
use loki_metris_core::devices::{DEVICE_SERVICE_TYPE, PairingConfirmation, PairingHello, PairingSession, pairing_test_reply};
use tokio::net::TcpListener;

pub(super) const CLIENT: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
pub(super) const DEVICE: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const SESSION: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";

/// 仅隔离网络夹具使用固定秘密，产品账本仍只能由真实配对证明生成。
pub(super) fn link_book() -> DeviceBook {
    let bytes = serde_json::to_vec(&serde_json::json!({"schemaVersion":1,"clientId":CLIENT,
        "devices":[{"id":DEVICE,"name":"Phone","key":vec![0xa7u8;32]}]})).unwrap();
    DeviceBook::decode(&bytes).unwrap()
}

/// 构造真实双向认证证明，供文件事务回归使用。
fn proof(client: &str) -> loki_metris_core::devices::VerifiedPairing {
    let target = DiscoveredDevice::new(DEVICE.into(), "Receiver".into(), vec!["192.168.1.2:4000".parse().unwrap()]).unwrap().with_pairing_protocol(Some("1"));
    let (state, hello) = PairingSession::start(client, &target, "0007", SESSION.into()).unwrap();
    let (reply, receiver) = pairing_test_reply(&hello, "0007");
    let (state, confirmation) = state.accept(reply).unwrap();
    state.verify(receiver(confirmation).unwrap()).unwrap()
}

/// 首次读不创建文件，成功配对能重启恢复；保存失败时旧字节与缓存均不改变。
#[test]
fn private_storage_roundtrip_and_failed_removal_preserve_records() {
    let temp = tempfile::tempdir().unwrap(); let store = store::DeviceStore::new(temp.path());
    let initial = store.load().unwrap(); assert!(!temp.path().join("devices").exists());
    store.prepare_pairing(DEVICE).unwrap();
    let saved = store.paired(proof(initial.client_id())).unwrap();
    let path = temp.path().join("devices/paired.json"); let original = std::fs::read(&path).unwrap();
    let restored = store::DeviceStore::new(temp.path()).load().unwrap(); assert_eq!(restored.summaries(), saved.summaries());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
    }
    // 将精确目标替换为目录模拟无法原子覆盖，原文件备份和内存快照必须保留。
    let backup = path.with_extension("backup"); std::fs::rename(&path, &backup).unwrap(); std::fs::create_dir(&path).unwrap();
    assert!(store.remove(DEVICE).is_err()); assert_eq!(store.load().unwrap().summaries(), saved.summaries());
    assert_eq!(std::fs::read(&backup).unwrap(), original);
    std::fs::remove_dir(&path).unwrap(); std::fs::rename(backup, &path).unwrap();
    assert!(store.remove(DEVICE).unwrap().summaries().is_empty());
    assert!(store::DeviceStore::new(temp.path()).load().unwrap().summaries().is_empty());
}

/// 损坏文件和符号链接不变成默认空账本，也不会被后续操作覆盖。
#[test]
fn corrupt_and_symlinked_storage_fail_closed() {
    let temp = tempfile::tempdir().unwrap(); let store = store::DeviceStore::new(temp.path());
    store.prepare_pairing(DEVICE).unwrap(); let path = temp.path().join("devices/paired.json");
    std::fs::write(&path, b"corrupt").unwrap();
    let other = store::DeviceStore::new(temp.path()); assert!(other.load().is_err());
    assert!(other.prepare_pairing(DEVICE).is_err()); assert_eq!(std::fs::read(&path).unwrap(), b"corrupt");
    #[cfg(unix)]
    {
        let untouched = temp.path().join("untouched"); std::fs::write(&untouched, b"keep").unwrap();
        std::fs::remove_file(&path).unwrap(); std::os::unix::fs::symlink(&untouched, &path).unwrap();
        assert!(other.load().is_err()); assert!(store.remove(DEVICE).is_err()); assert_eq!(std::fs::read(untouched).unwrap(), b"keep");
    }
}

/// 网络夹具只监听随机环回端口；产品发现规则仍拒绝环回端点。
#[tokio::test]
async fn real_transport_requires_correct_code_and_authenticated_final_ack() {
    for (client_code, server_code, success) in [("0007", "0007", true), ("0008", "0007", false)] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap(); let address = listener.local_addr().unwrap();
        let receiver = async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let hello: PairingHello = transport::read_frame(&mut stream).await.unwrap();
            let (reply, confirm) = pairing_test_reply(&hello, server_code);
            transport::write_frame(&mut stream, &reply).await.unwrap();
            let confirmation: PairingConfirmation = transport::read_frame(&mut stream).await.unwrap();
            if let Ok(ack) = confirm(confirmation) { transport::write_frame(&mut stream, &ack).await.unwrap(); }
        };
        let target = DiscoveredDevice { id: DEVICE.into(), name: "Receiver".into(), endpoints: vec![address], pairing_available: true };
        let (result, _) = tokio::join!(transport::pair(&target, CLIENT, client_code), receiver);
        assert_eq!(result.is_ok(), success);
    }
}

/// 长度超限在读取载荷前拒绝，断开和阻塞接收可由外层超时回收。
#[tokio::test]
async fn malformed_frame_disconnect_and_timeout_do_not_pair() {
    use tokio::io::AsyncWriteExt;
    for length in [0, 4097, u32::MAX] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap(); let address = listener.local_addr().unwrap();
        let server = async move { let (mut socket, _) = listener.accept().await.unwrap(); socket.write_u32(length).await.unwrap(); };
        let target = DiscoveredDevice { id: DEVICE.into(), name: "Receiver".into(), endpoints: vec![address], pairing_available: true };
        let (result, _) = tokio::join!(transport::pair(&target, CLIENT, "0007"), server); assert!(result.is_err());
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap(); let address = listener.local_addr().unwrap();
    let target = DiscoveredDevice { id: DEVICE.into(), name: "Receiver".into(), endpoints: vec![address], pairing_available: true };
    let result = tokio::time::timeout(Duration::from_millis(30), transport::pair(&target, CLIENT, "0007")).await;
    assert!(result.is_err());
}

/// 只解析固定服务和兼容版本，伪造公网地址与未知协议不能进入候选。
#[test]
fn mdns_candidate_rejects_incompatible_or_non_lan_advertisements() {
    for (service, version, address, expected) in [
        (DEVICE_SERVICE_TYPE, "1", "192.168.1.2", true),
        (DEVICE_SERVICE_TYPE, "2", "192.168.1.2", false),
        ("_http._tcp.local.", "1", "192.168.1.2", false),
        (DEVICE_SERVICE_TYPE, "1", "8.8.8.8", false),
    ] {
        let info = mdns_sd::ServiceInfo::new(service, "Receiver", "receiver.local.", address, 4000,
            [("v", version), ("id", DEVICE), ("name", "Receiver")].as_slice()).unwrap().as_resolved_service();
        assert_eq!(discovery::candidate(&info).is_some(), expected);
    }
    for pairing in [None, Some("off"), Some("1"), Some("2")] {
        let mut properties = vec![("v", "1"), ("id", DEVICE), ("name", "Receiver")];
        if let Some(pairing) = pairing { properties.push(("pair", pairing)); }
        let info = mdns_sd::ServiceInfo::new(DEVICE_SERVICE_TYPE, "Receiver", "receiver.local.", "192.168.1.2", 4000,
            properties.as_slice()).unwrap().as_resolved_service();
        assert_eq!(discovery::candidate(&info).unwrap().pairing_available, pairing == Some("1"));
    }
}

/// 未进入设备页不发现或写文件；无候选和错码在任何网络任务前拒绝。
#[test]
fn service_is_lazy_and_rejects_unapproved_or_stale_pairing() {
    let temp = tempfile::tempdir().unwrap(); let directory = temp.path().join("data");
    let service = UsageService::in_test_directory(directory.clone()).unwrap();
    assert_eq!(service.devices(), DeviceView::default()); assert!(!directory.exists());
    assert_eq!(service.pair_device(DEVICE, "123".into()), Err("device_code_invalid"));
    assert_eq!(service.pair_device(DEVICE, "0007".into()), Err("device_storage_failed"));
    service.load_devices().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while service.devices().pending { assert!(Instant::now() < deadline); std::thread::sleep(Duration::from_millis(5)); }
    assert!(service.devices().loaded); assert!(!directory.exists());
    assert_eq!(service.pair_device(DEVICE, "0007".into()), Err("device_not_discovered"));
    let target = DiscoveredDevice::new(DEVICE.into(), "Broadcast only".into(), vec!["192.168.1.2:4000".parse().unwrap()]).unwrap();
    service.context.devices.state().candidates.insert("broadcast-only".into(), (target, Instant::now() + Duration::from_secs(30)));
    assert_eq!(service.pair_device(DEVICE, "0007".into()), Err("device_pairing_unavailable"));
    assert!(!directory.exists());
    service.finish();
}

/// 快速取消再进入登记重启意图，旧轮收尾只消费一次；存储成功不清除发现失败。
#[test]
fn discovery_restart_and_operation_errors_have_independent_ownership() {
    let temp = tempfile::tempdir().unwrap();
    let service = UsageService::in_test_directory(temp.path().join("data")).unwrap();
    let devices = &service.context.devices;
    let (cancel, _receiver) = watch::channel(false);
    { let mut state = devices.state(); state.view.scanning = true; state.scan_cancel = Some(cancel); }
    devices.cancel(); devices.discover().unwrap();
    let restart = devices.finish_discovery(Err("device_discovery_failed")).unwrap();
    devices.cancel();
    devices.discover_for(Some(restart)).unwrap();
    assert!(!devices.snapshot().scanning, "离页后迟到重启不能创建新轮");
    assert!(devices.finish_discovery(Ok(())).is_none());
    devices.finish(Ok(DeviceBook::new(CLIENT.into()).unwrap()), None);
    assert_eq!(devices.snapshot().discovery_error, Some("device_discovery_failed"));
    assert!(devices.snapshot().error.is_none());
    service.finish();
}

/// 应用服务真实握手到落盘闭环：没有B最终认证回执时磁盘和页面都不能新增设备。
#[test]
fn service_commits_only_after_real_transport_final_confirmation() {
    for acknowledge in [true, false] {
        let temp = tempfile::tempdir().unwrap(); let directory = temp.path().join("data");
        let service = UsageService::in_test_directory(directory.clone()).unwrap();
        service.load_devices().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while service.devices().pending { assert!(Instant::now() < deadline); std::thread::sleep(Duration::from_millis(5)); }
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap(); let address = listener.local_addr().unwrap();
        let peer = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            runtime.block_on(async move {
                tokio::time::timeout(Duration::from_secs(5), async move {
                    let listener = TcpListener::from_std(listener).unwrap(); let (mut stream, _) = listener.accept().await.unwrap();
                    let hello: PairingHello = transport::read_frame(&mut stream).await.unwrap();
                    let (reply, receiver) = pairing_test_reply(&hello, "0007");
                    transport::write_frame(&mut stream, &reply).await.unwrap();
                    let confirmation: PairingConfirmation = transport::read_frame(&mut stream).await.unwrap();
                    let ack = receiver(confirmation).unwrap();
                    if acknowledge { transport::write_frame(&mut stream, &ack).await.unwrap(); }
                }).await.unwrap();
            });
        });
        service.context.devices.state().candidates.insert("isolated-test-peer".into(),
            (DiscoveredDevice { id: DEVICE.into(), name: "Receiver".into(), endpoints: vec![address], pairing_available: true }, Instant::now() + Duration::from_secs(30)));
        service.pair_device(DEVICE, "0007".into()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while service.devices().pending { assert!(Instant::now() < deadline); std::thread::sleep(Duration::from_millis(5)); }
        peer.join().unwrap();
        assert_eq!(service.devices().saved.len(), usize::from(acknowledge));
        let stored = DeviceBook::decode(&std::fs::read(directory.join("devices/paired.json")).unwrap()).unwrap();
        assert_eq!(stored.summaries().len(), usize::from(acknowledge));
        service.finish();
    }
}
