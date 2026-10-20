use super::*;

const CLIENT: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const DEVICE: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const SESSION: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";

/// 构造已保存的隔离账本并执行真实恢复握手，供联动与在线状态回归复用。
pub(super) fn link_fixture() -> (DeviceBook, link::AuthenticatedLinkSession, loki_metris_pairing::session::SecureChannel) {
    let mut book = DeviceBook::new(CLIENT.into()).unwrap();
    book.devices.push(SavedDevice { id: DEVICE.into(), name: "Receiver".into(), key: PairingKey([0xa7; 32]) });
    let (device, hello) = loki_metris_pairing::session::DeviceSession::start(CLIENT, DEVICE, &[0xa7; 32], SESSION.into()).unwrap();
    let (desktop, reply) = book.begin_link_session(hello, "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into()).unwrap();
    let (device, confirmation) = device.accept(reply).unwrap();
    (book, desktop.verify(confirmation).unwrap(), device)
}

/// 夹具仍经过真实广告规则，包含合法私网端点。
fn target() -> DiscoveredDevice {
    DiscoveredDevice::new(DEVICE.into(), "Receiver".into(), vec!["192.168.1.8:1024".parse().unwrap()]).unwrap().with_pairing_protocol(Some("1"))
}

/// 正确四位码完成真实密码交换、双向确认后才能保存，重启可恢复凭据并移除。
#[test]
fn pairing_saves_only_after_authenticated_ack_and_roundtrips() {
    let book = DeviceBook::new(CLIENT.into()).unwrap();
    let (session, hello) = PairingSession::start(CLIENT, &target(), "0037", SESSION.into()).unwrap();
    let wire = serde_json::to_value(&hello).unwrap();
    assert!(wire.get("code").is_none() && wire.get("password").is_none());
    let (reply, receiver) = pairing_test_reply(&hello, "0037");
    let (waiting, confirmation) = session.accept(reply).unwrap();
    assert!(book.summaries().is_empty());
    let proof = waiting.verify(receiver(confirmation).unwrap()).unwrap();
    let saved = book.paired(proof).unwrap();
    let restored = DeviceBook::decode(&saved.encode_private().unwrap()).unwrap();
    assert_eq!(restored.summaries(), vec![SavedDeviceSummary { id: DEVICE.into(), name: "Test receiver".into() }]);
    assert!(!format!("{restored:?}").contains(&format!("{:?}", restored.devices[0].key.0)));
    assert!(restored.without(DEVICE).unwrap().summaries().is_empty());
    assert_eq!(restored.without(CLIENT).unwrap_err(), DeviceError::NotSaved);
}

/// 错码即使SPAKE2返回密钥也无法通过确认，且旧会话回执无法重放。
#[test]
fn wrong_code_tampering_and_replay_cannot_create_saved_proof() {
    let (session, hello) = PairingSession::start(CLIENT, &target(), "0037", SESSION.into()).unwrap();
    let (reply, receiver) = pairing_test_reply(&hello, "0038");
    let (waiting, confirmation) = session.accept(reply).unwrap();
    assert_eq!(receiver(confirmation).unwrap_err(), DeviceError::PairingFailed);
    assert!(waiting.verify(PairingAck { confirmation: "not-authenticated".into() }).is_err());
    let (session, hello) = PairingSession::start(CLIENT, &target(), "0037", SESSION.into()).unwrap();
    let (reply, receiver) = pairing_test_reply(&hello, "0037");
    let (waiting, confirmation) = session.accept(reply).unwrap();
    let ack = receiver(confirmation).unwrap();
    assert!(waiting.verify(ack.clone()).is_ok());
    let (session, hello) = PairingSession::start(CLIENT, &target(), "0037", SESSION.into()).unwrap();
    let (reply, _) = pairing_test_reply(&hello, "0037");
    let (waiting, _) = session.accept(reply).unwrap();
    assert!(waiting.verify(ack).is_err());
}

/// 发现不信任公网/环回/零端口，IPv6作用域和四位码前导零保持正确。
#[test]
fn advertisements_and_codes_have_bounded_validated_inputs() {
    assert!(valid_pairing_code("0000"));
    for value in ["123", "12345", "１２３４", " 123", "12a4"] { assert!(!valid_pairing_code(value)); }
    for value in ["127.0.0.1:1234", "8.8.8.8:1234", "0.0.0.0:1234", "192.168.1.1:0", "[fe80::1]:1234", "[::ffff:127.0.0.1]:1234"] {
        assert!(!is_lan_endpoint(value.parse().unwrap()), "{value}");
    }
    assert!(is_lan_endpoint("[fe80::1%2]:1234".parse().unwrap()));
    assert!(is_lan_endpoint("[fd00::1]:1234".parse().unwrap()));
    assert!(DiscoveredDevice::new(DEVICE.into(), "Bad\nname".into(), target().endpoints).is_err());
}

/// 损坏/未知schema、重复身份和容量非法快照均失败关闭，不产生空列表替换。
#[test]
fn corrupt_books_are_rejected_and_duplicate_pairing_preserves_existing_key() {
    assert!(DeviceBook::decode(b"{}").is_err());
    let book = DeviceBook::new(CLIENT.into()).unwrap();
    let (session, hello) = PairingSession::start(CLIENT, &target(), "0001", SESSION.into()).unwrap();
    let (reply, receiver) = pairing_test_reply(&hello, "0001");
    let (waiting, confirmation) = session.accept(reply).unwrap();
    let book = book.paired(waiting.verify(receiver(confirmation).unwrap()).unwrap()).unwrap();
    let (session, hello) = PairingSession::start(CLIENT, &target(), "0001", SESSION.into()).unwrap();
    let (reply, receiver) = pairing_test_reply(&hello, "0001");
    let (waiting, confirmation) = session.accept(reply).unwrap();
    assert_eq!(book.paired(waiting.verify(receiver(confirmation).unwrap()).unwrap()).unwrap_err(), DeviceError::AlreadySaved);
    let mut invalid = serde_json::to_value(&book).unwrap();
    let duplicate = invalid["devices"][0].clone();
    invalid["devices"].as_array_mut().unwrap().push(duplicate);
    assert!(DeviceBook::decode(&serde_json::to_vec(&invalid).unwrap()).is_err());
}

/// 修改身份/版本/会话立即拒绝；名称、nonce和消息方向篡改不能沿用原确认。
#[test]
fn metadata_identity_and_direction_tampering_are_rejected() {
    for field in ["version", "device", "session", "name", "nonce", "exchange", "reflection", "ack"] {
        let (session, hello) = PairingSession::start(CLIENT, &target(), "0037", SESSION.into()).unwrap();
        let (mut reply, receiver) = pairing_test_reply(&hello, "0037");
        // 正常对端基于原始回应产生回执；篡改后的A不能获得相同确认上下文。
        match field {
            "version" => reply.version = 2,
            "device" => reply.device_id = CLIENT.into(),
            "session" => reply.session = CLIENT.into(),
            "name" => reply.name = "Other receiver".into(),
            "nonce" => reply.nonce = CLIENT.into(),
            "exchange" => reply.exchange = hello.exchange.clone(),
            _ => {},
        }
        let accepted = session.accept(reply);
        if ["version", "device", "session", "exchange"].contains(&field) { assert!(accepted.is_err(), "{field}"); continue; }
        let (waiting, confirmation) = accepted.unwrap();
        if field == "reflection" { assert!(waiting.verify(PairingAck { confirmation: confirmation.confirmation }).is_err()); }
        else if field == "ack" {
            let mut ack = receiver(confirmation).unwrap(); ack.confirmation.replace_range(0..1, "!");
            assert!(waiting.verify(ack).is_err());
        } else { assert!(receiver(confirmation).is_err(), "{field}"); }
    }
}
