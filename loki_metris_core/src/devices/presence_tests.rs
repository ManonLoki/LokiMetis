use super::*;
use crate::devices::{link::DevicePlatform, tests::link_fixture};

/// 形态只来自当前认证租约，断线保留最后报告但重连、撤销和超时不能复用旧资格。
#[test]
fn reported_form_factor_is_bound_to_current_authenticated_lease() {
    use crate::devices::link::DeviceFormFactor::{Phone, Tablet};
    let (book, mut session, _) = link_fixture(); let now = Instant::now(); let mut presence = DevicePresence::default();
    let peer = "192.168.1.8:40000".parse().unwrap();
    let first = connect(&mut presence, &book, &mut session, "Lenovo TB-J606F", peer, now).unwrap();
    assert_eq!(presence.snapshot(&book, now)[0].form_factor, None);
    presence.report_form_factor(&book, &first, Tablet, now).unwrap();
    assert_eq!(presence.snapshot(&book, now)[0].form_factor, Some(Tablet));
    presence.disconnected(&first, UnavailableReason::Background).unwrap();
    assert_eq!(presence.snapshot(&book, now)[0].form_factor, Some(Tablet));
    assert_eq!(presence.report_form_factor(&book, &first, Phone, now), Err(LinkError::Expired));
    let (_, mut resumed, _) = link_fixture();
    let second = connect(&mut presence, &book, &mut resumed, "Renamed", peer, now).unwrap();
    assert_eq!(presence.snapshot(&book, now)[0].form_factor, None);
    assert_eq!(presence.report_form_factor(&book, &first, Phone, now), Err(LinkError::Stale));
    let mut revoked = book.clone(); revoked.devices[0].key.0 = [0xb8; 32];
    assert_eq!(presence.report_form_factor(&revoked, &second, Phone, now), Err(LinkError::Untrusted));
    assert_eq!(presence.report_form_factor(&book, &second, Tablet, now + DEVICE_HEARTBEAT_TIMEOUT), Err(LinkError::Expired));
    presence.report_form_factor(&book, &second, Phone, now).unwrap();
    assert_eq!(presence.snapshot(&book, now)[0].form_factor, Some(Phone));
}

/// 元信息是应用内名称，IP另由适配器的socket事实提供。
fn connect(presence: &mut DevicePresence, book: &DeviceBook, session: &mut AuthenticatedLinkSession, name: &str, peer: SocketAddr, now: Instant) -> Result<PresenceLease, LinkError> {
    let info = DeviceInfo { device_id: session.device_id().into(), name: name.into(), platform: DevicePlatform::Ios, app_version: "1.0".into() };
    presence.connected(book, session, info, peer, now)
}

/// 保存配对及收到广告不算在线，认证连接后才展示真实socket地址和元信息。
#[test]
fn only_authenticated_current_trust_creates_reachable_presence() {
    let (book, mut session, _) = link_fixture(); let now = Instant::now(); let mut presence = DevicePresence::default();
    assert_eq!(presence.snapshot(&book, now)[0].state, DeviceConnectionState::NotConnected);
    let peer = "192.168.1.9:45000".parse().unwrap();
    let lease = connect(&mut presence, &book, &mut session, "Phone", peer, now).unwrap();
    let view = &presence.snapshot(&book, now)[0];
    assert_eq!(view.state, DeviceConnectionState::Reachable); assert_eq!(view.peer, Some(peer));
    assert_eq!(view.info.as_ref().unwrap().name, "Phone"); assert_eq!(lease.device_id(), session.device_id());
    let empty = book.without(session.device_id()).unwrap();
    assert!(matches!(connect(&mut presence, &empty, &mut session, "Phone", peer, now), Err(LinkError::Untrusted)));
    assert!(presence.snapshot(&empty, now).is_empty());
}

/// 新连接换IP与名称后，旧socket的心跳及关闭均不能覆盖新代次。
#[test]
fn reconnect_updates_ip_and_stale_socket_cannot_mutate_new_session() {
    let (book, mut session, _) = link_fixture(); let now = Instant::now(); let mut presence = DevicePresence::default();
    let first = connect(&mut presence, &book, &mut session, "Old", "192.168.1.8:40000".parse().unwrap(), now).unwrap();
    let peer = "192.168.1.10:50000".parse().unwrap();
    let (_, mut reconnected, _) = link_fixture();
    let second = connect(&mut presence, &book, &mut reconnected, "New", peer, now + Duration::from_secs(1)).unwrap();
    assert_ne!(first, second);
    assert_eq!(presence.received(&book, &first, now + Duration::from_secs(2)), Err(LinkError::Stale));
    assert_eq!(presence.disconnected(&first, UnavailableReason::Disconnected), Err(LinkError::Stale));
    let view = &presence.snapshot(&book, now + Duration::from_secs(2))[0];
    assert_eq!(view.state, DeviceConnectionState::Reachable); assert_eq!(view.peer, Some(peer));
    assert_eq!(view.info.as_ref().unwrap().name, "New");
}

/// 45秒边界即失效，迟到的认证包不能复活旧会话，必须重新握手。
#[test]
fn timeout_boundary_and_late_heartbeat_require_reauthentication() {
    let (book, mut session, _) = link_fixture(); let now = Instant::now(); let mut presence = DevicePresence::default();
    let lease = connect(&mut presence, &book, &mut session, "Phone", "192.168.1.8:40000".parse().unwrap(), now).unwrap();
    let deadline = now + DEVICE_HEARTBEAT_TIMEOUT;
    assert_eq!(presence.snapshot(&book, deadline)[0].state, DeviceConnectionState::Unavailable(UnavailableReason::HeartbeatTimeout));
    assert_eq!(presence.received(&book, &lease, deadline), Err(LinkError::Expired));
    assert_eq!(presence.received(&book, &lease, deadline + Duration::from_secs(1)), Err(LinkError::Expired));
    assert!(presence.expire(&book, deadline).is_empty());
    assert_eq!(connect(&mut presence, &book, &mut session, "Phone", "192.168.1.8:40000".parse().unwrap(), deadline), Err(LinkError::Expired));
}

/// 认证入站通信续期，单调时钟倒退不续期；后台断开保留最后确认时间和历史IP。
#[test]
fn valid_heartbeat_renews_and_background_does_not_fake_last_contact() {
    let (book, mut session, _) = link_fixture(); let now = Instant::now(); let mut presence = DevicePresence::default();
    let lease = connect(&mut presence, &book, &mut session, "Phone", "192.168.1.8:40000".parse().unwrap(), now).unwrap();
    presence.received(&book, &lease, now + Duration::from_secs(30)).unwrap();
    assert_eq!(presence.received(&book, &lease, now), Err(LinkError::Invalid));
    assert_eq!(presence.snapshot(&book, now)[0].state, DeviceConnectionState::Unavailable(UnavailableReason::TimeUnknown));
    assert!(presence.expire(&book, now + Duration::from_secs(60)).is_empty());
    presence.disconnected(&lease, UnavailableReason::Background).unwrap();
    let view = &presence.snapshot(&book, now + Duration::from_secs(65))[0];
    assert_eq!(view.since_last_contact, Some(Duration::from_secs(35)));
    assert_eq!(view.state, DeviceConnectionState::Unavailable(UnavailableReason::Background));
    assert!(view.peer.is_some());
}

/// 过期返回精确租约且只返回一次，忘记再连接仍不能复用原代次。
#[test]
fn expire_and_forget_preserve_generation_ownership() {
    let (book, mut session, _) = link_fixture(); let now = Instant::now(); let mut presence = DevicePresence::default();
    let lease = connect(&mut presence, &book, &mut session, "Phone", "192.168.1.8:40000".parse().unwrap(), now).unwrap();
    assert_eq!(presence.expire(&book, now + DEVICE_HEARTBEAT_TIMEOUT), vec![lease.clone()]);
    assert!(presence.expire(&book, now + DEVICE_HEARTBEAT_TIMEOUT).is_empty());
    presence.forget(session.device_id());
    let (_, mut reconnected, _) = link_fixture();
    let new = connect(&mut presence, &book, &mut reconnected, "Phone", "192.168.1.8:50000".parse().unwrap(), now + Duration::from_secs(50)).unwrap();
    assert_ne!(lease, new);
    assert_eq!(presence.received(&book, &lease, now + Duration::from_secs(51)), Err(LinkError::Stale));
}

/// 同ID换凭据不继承旧在线投影或续期资格，已损坏的加密通道也不能上线。
#[test]
fn revoked_trust_and_poisoned_channel_cannot_become_reachable() {
    let (book, mut session, _) = link_fixture(); let now = Instant::now(); let mut presence = DevicePresence::default();
    let lease = connect(&mut presence, &book, &mut session, "Phone", "192.168.1.8:40000".parse().unwrap(), now).unwrap();
    let mut replacement = book.clone(); replacement.devices[0].key.0 = [0xb8; 32];
    assert_eq!(presence.snapshot(&replacement, now)[0].state, DeviceConnectionState::NotConnected);
    assert_eq!(presence.received(&replacement, &lease, now), Err(LinkError::Untrusted));
    let (_, mut poisoned, _) = link_fixture();
    assert_eq!(poisoned.open(&super::super::link::EncryptedPacket { sequence: 0, ciphertext: "AA==".into() }), Err(LinkError::Authentication));
    assert_eq!(connect(&mut presence, &book, &mut poisoned, "Phone", "192.168.1.8:50000".parse().unwrap(), now), Err(LinkError::Expired));
}

/// IPv6链路本地保留接口scope，公网/环回/无scope地址拒绝；代次耗尽不回绕。
#[test]
fn endpoint_scope_and_generation_limits_fail_closed() {
    let (book, mut session, _) = link_fixture(); let now = Instant::now(); let mut presence = DevicePresence::default();
    for peer in ["8.8.8.8:50000", "127.0.0.1:50000", "[fe80::1]:50000"] {
        assert_eq!(connect(&mut presence, &book, &mut session, "Phone", peer.parse().unwrap(), now), Err(LinkError::Invalid));
    }
    let peer = "[fe80::1%2]:50000".parse().unwrap();
    connect(&mut presence, &book, &mut session, "Phone", peer, now).unwrap();
    assert_eq!(presence.snapshot(&book, now)[0].peer, Some(peer));
    presence.generation = u64::MAX;
    let (_, mut reconnected, _) = link_fixture();
    assert_eq!(connect(&mut presence, &book, &mut reconnected, "Phone", peer, now), Err(LinkError::Limit));
}

/// 每次真实回执的序号与单调年龄形成独立Watch报告，完整快照ACK共享该次序号。
fn watch_report(sequence: u64, age_ms: u64) -> super::super::companion::WatchCompanionReport {
    use super::super::companion::{WatchCompanionReport, WatchInfo, CompanionObservation};
    WatchCompanionReport { info: WatchInfo { device_id: "c169da9b-4716-4774-bdcb-a33bfd6e1ac1".into(), name: "My Watch".into(), app_version: "1.0".into() },
        reachable: true, contact: Some(CompanionObservation { sequence, age_ms }), sync: Some(CompanionObservation { sequence, age_ms }) }
}

/// 手机心跳或重复报告不能延长Watch通信/同步，撤销后相同回执也不能复活。
#[test]
fn companion_has_independent_expiry_and_duplicate_proof_cannot_revive() {
    let (book, mut session, _) = link_fixture(); let now = Instant::now(); let mut presence = DevicePresence::default();
    let lease = connect(&mut presence, &book, &mut session, "Phone", "192.168.1.8:40000".parse().unwrap(), now).unwrap();
    let report = watch_report(1, 0); presence.companion(&book, &lease, report.clone(), now, now).unwrap();
    presence.received(&book, &lease, now + Duration::from_secs(30)).unwrap();
    presence.companion(&book, &lease, report.clone(), now + Duration::from_secs(30), now + Duration::from_secs(30)).unwrap();
    let view = presence.snapshot(&book, now + Duration::from_secs(45));
    assert_eq!(view[0].state, DeviceConnectionState::Reachable);
    let watch = view[0].companion.as_ref().unwrap(); assert!(!watch.reachable);
    assert_eq!(watch.since_last_contact, Some(Duration::from_secs(45))); assert_eq!(watch.since_last_sync, Some(Duration::from_secs(45)));
    let mut pause = watch_report(2, 0); pause.reachable = false;
    presence.companion(&book, &lease, pause, now + Duration::from_secs(31), now + Duration::from_secs(31)).unwrap();
    presence.companion(&book, &lease, watch_report(2, 0), now + Duration::from_secs(32), now + Duration::from_secs(32)).unwrap();
    assert!(!presence.snapshot(&book, now + Duration::from_secs(32))[0].companion.as_ref().unwrap().reachable);
    let mut contact = watch_report(3, 0); contact.sync = watch_report(2, 1_000).sync;
    presence.companion(&book, &lease, contact, now + Duration::from_secs(33), now + Duration::from_secs(33)).unwrap();
    let view = presence.snapshot(&book, now + Duration::from_secs(33)); let watch = view[0].companion.as_ref().unwrap();
    assert!(watch.reachable); assert_eq!(watch.since_last_sync, Some(Duration::from_secs(2)));
    assert_eq!(presence.companion(&book, &lease, watch_report(1, 0), now + Duration::from_secs(34), now + Duration::from_secs(34)), Err(LinkError::Stale));
}

/// 新序号即使第一次到达也必须扣除桌面challenge至收到报告的全部延迟，历史年龄不依赖开机时长。
#[test]
fn companion_heartbeat_anchor_bounds_delayed_new_proof() {
    let (book, mut session, _) = link_fixture(); let now = Instant::now(); let mut presence = DevicePresence::default();
    let lease = connect(&mut presence, &book, &mut session, "Phone", "192.168.1.8:40000".parse().unwrap(), now).unwrap();
    presence.companion(&book, &lease, watch_report(1, 35_000), now, now + Duration::from_secs(20)).unwrap();
    assert_eq!(presence.companion(&book, &lease, watch_report(2, 0), now, now + Duration::from_secs(19)), Err(LinkError::Invalid));
    presence.received(&book, &lease, now + Duration::from_secs(30)).unwrap();
    assert!(!presence.snapshot(&book, now + Duration::from_secs(20))[0].companion.as_ref().unwrap().reachable);
    assert_eq!(presence.companion(&book, &lease, watch_report(2, 0), now, now + Duration::from_secs(45)), Err(LinkError::Expired));
    assert_eq!(presence.companion(&book, &lease, watch_report(2, 0), now + Duration::from_secs(40), now + Duration::from_secs(35)), Err(LinkError::Expired));
    let mut history = watch_report(2, 0); history.sync.as_mut().unwrap().age_ms = 604_800_000;
    presence.companion(&book, &lease, history, now + Duration::from_secs(31), now + Duration::from_secs(31)).unwrap();
    assert_eq!(presence.snapshot(&book, now + Duration::from_secs(31))[0].companion.as_ref().unwrap().since_last_sync, Some(Duration::from_millis(604_800_000)));
}

/// 父租约暂停/撤销/换代与忘记都撤销子设备，Android及身份碰撞不能创建Watch。
#[test]
fn companion_is_bound_to_current_ios_parent_trust_and_generation() {
    let (book, mut session, _) = link_fixture(); let now = Instant::now(); let mut presence = DevicePresence::default();
    let lease = connect(&mut presence, &book, &mut session, "Phone", "192.168.1.8:40000".parse().unwrap(), now).unwrap();
    presence.companion(&book, &lease, watch_report(1, 0), now, now).unwrap();
    presence.disconnected(&lease, UnavailableReason::Background).unwrap();
    assert!(!presence.snapshot(&book, now)[0].companion.as_ref().unwrap().reachable);
    assert_eq!(presence.companion(&book, &lease, watch_report(2, 0), now, now), Err(LinkError::Expired));
    let (_, mut resumed, _) = link_fixture();
    let next = connect(&mut presence, &book, &mut resumed, "Phone", "192.168.1.9:50000".parse().unwrap(), now).unwrap();
    assert!(presence.snapshot(&book, now)[0].companion.is_none());
    assert_eq!(presence.companion(&book, &lease, watch_report(2, 0), now, now), Err(LinkError::Stale));
    let mut report = watch_report(2, 0); report.info.device_id = book.client_id().into();
    assert_eq!(presence.companion(&book, &next, report, now, now), Err(LinkError::Invalid));
    presence.entries.get_mut(session.device_id()).unwrap().info.platform = DevicePlatform::Android;
    assert_eq!(presence.companion(&book, &next, watch_report(2, 0), now, now), Err(LinkError::Invalid));
    presence.entries.get_mut(session.device_id()).unwrap().info.platform = DevicePlatform::Ios;
    let mut revoked = book.clone(); revoked.devices[0].key.0 = [0x55;32];
    assert_eq!(presence.companion(&revoked, &next, watch_report(2, 0), now, now), Err(LinkError::Untrusted));
    assert!(presence.snapshot(&revoked, now)[0].companion.is_none());
    presence.forget(session.device_id()); assert!(presence.snapshot(&book, now)[0].companion.is_none());
}

/// 45秒边界、计数上限和年龄关系严格校验，拒绝的报告不损坏先前有效状态。
#[test]
fn companion_rejects_contradictory_or_unbounded_facts() {
    let mut report = watch_report(1, 45_000); assert_eq!(report.validate(), Err(LinkError::Invalid));
    report.reachable = false; assert!(report.validate().is_ok());
    report.contact.as_mut().unwrap().sequence = 0; assert_eq!(report.validate(), Err(LinkError::Invalid));
    report = watch_report(1, 0); report.sync.as_mut().unwrap().sequence = 2; assert_eq!(report.validate(), Err(LinkError::Invalid));
    report = watch_report(1, 5); report.sync.as_mut().unwrap().age_ms = 4; assert_eq!(report.validate(), Err(LinkError::Invalid));
    report = watch_report(9_007_199_254_740_992, 0); assert_eq!(report.validate(), Err(LinkError::Invalid));
}

/// 换表只替换展示身份，父租约的序号高水位继续防止旧表报告重新创建可达证明。
#[test]
fn companion_identity_switch_cannot_bypass_parent_sequence_high_watermark() {
    let (book, mut session, _) = link_fixture(); let now = Instant::now(); let mut presence = DevicePresence::default();
    let lease = connect(&mut presence, &book, &mut session, "Phone", "192.168.1.8:40000".parse().unwrap(), now).unwrap();
    presence.companion(&book, &lease, watch_report(5, 0), now, now).unwrap();
    let mut pause = watch_report(5, 0); pause.reachable = false;
    presence.companion(&book, &lease, pause, now, now).unwrap();
    let mut other = watch_report(6, 0); other.info.device_id = "c169da9b-4716-4774-bdcb-a33bfd6e1ac2".into();
    presence.companion(&book, &lease, other.clone(), now, now).unwrap();
    assert_eq!(presence.companion(&book, &lease, watch_report(5, 0), now, now), Err(LinkError::Stale));
    let view = presence.snapshot(&book, now); assert_eq!(view[0].companion.as_ref().unwrap().info, other.info);
    let mut stale_sync = watch_report(7, 0); stale_sync.sync = watch_report(5, 0).sync;
    assert_eq!(presence.companion(&book, &lease, stale_sync, now, now), Err(LinkError::Stale));
    let mut returned = watch_report(7, 0); returned.sync = None;
    presence.companion(&book, &lease, returned, now, now).unwrap();
    assert!(presence.snapshot(&book, now)[0].companion.as_ref().unwrap().since_last_sync.is_none());
}
