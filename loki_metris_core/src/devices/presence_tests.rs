use super::*;
use crate::devices::{link::DevicePlatform, tests::link_fixture};

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
