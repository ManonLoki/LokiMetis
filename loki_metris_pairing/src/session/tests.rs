//! 业务恢复使用真实密码交换，测试覆盖双向认证、独立nonce与失败后永久关闭。

use super::*;
use crate::tests::wire;

const CLIENT: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const DEVICE: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const SESSION: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const CHALLENGE: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const SECRET: [u8; 32] = [0xa7; 32];

/// 使用固定宿主UUID和真实随机PAKE交换建立两个方向的可用通道。
fn channels() -> (SecureChannel, SecureChannel) {
    let (device, hello) = DeviceSession::start(CLIENT, DEVICE, &SECRET, SESSION.into()).unwrap();
    let (desktop, reply) = AwaitingDeviceProof::accept(wire(&hello), CLIENT, DEVICE, &SECRET, CHALLENGE.into()).unwrap();
    let (device, proof) = device.accept(wire(&reply)).unwrap();
    (desktop.verify(wire(&proof)).unwrap(), device)
}

/// 三帧JSON握手后双向连续发包，非UTF8秘密及应用字节不被字符串转换破坏。
#[test]
fn authenticated_channels_exchange_ordered_binary_messages() {
    let (mut desktop, mut device) = channels();
    for sequence in 0..4 {
        let message = [0, 0xff, sequence as u8];
        let packet = desktop.seal(&message).unwrap(); assert_eq!(packet.sequence, sequence);
        assert_eq!(device.open(&wire(&packet)).unwrap(), message);
        let reply = device.seal(&message).unwrap(); assert_eq!(reply.sequence, sequence);
        assert_ne!(packet.ciphertext, reply.ciphertext);
        assert_eq!(desktop.open(&wire(&reply)).unwrap(), message);
    }
}

/// 错误持久密钥不能通过桌面确认，也不能让桌面生成可收发业务的通道。
#[test]
fn wrong_saved_secret_cannot_authenticate_either_peer() {
    let (device, hello) = DeviceSession::start(CLIENT, DEVICE, &SECRET, SESSION.into()).unwrap();
    let (desktop, reply) = AwaitingDeviceProof::accept(hello, CLIENT, DEVICE, &[0xb8; 32], CHALLENGE.into()).unwrap();
    assert!(matches!(device.accept(reply.clone()), Err(SessionError::VerificationFailed)));
    assert!(matches!(desktop.verify(SessionConfirmation { confirmation: reply.confirmation }), Err(SessionError::VerificationFailed)));
}

/// 双方UUID即使被宿主误复用，全新PAKE仍使旧确认与旧业务包不能跨连接重放。
#[test]
fn reused_host_identifiers_do_not_reuse_session_keys() {
    let (device, hello) = DeviceSession::start(CLIENT, DEVICE, &SECRET, SESSION.into()).unwrap();
    let (desktop, old_reply) = AwaitingDeviceProof::accept(hello, CLIENT, DEVICE, &SECRET, CHALLENGE.into()).unwrap();
    let (_, old_proof) = device.accept(old_reply.clone()).unwrap();
    let mut old_desktop = desktop.verify(old_proof.clone()).unwrap();
    let old_packet = old_desktop.seal(b"old snapshot").unwrap();

    let (new_device, hello) = DeviceSession::start(CLIENT, DEVICE, &SECRET, SESSION.into()).unwrap();
    let (new_desktop, new_reply) = AwaitingDeviceProof::accept(hello, CLIENT, DEVICE, &SECRET, CHALLENGE.into()).unwrap();
    assert!(matches!(new_desktop.verify(old_proof), Err(SessionError::VerificationFailed)));
    assert!(matches!(new_device.accept(old_reply), Err(SessionError::VerificationFailed)));
    assert_ne!(new_reply.confirmation, "");
    let (_, mut new_device) = channels();
    assert_eq!(new_device.open(&old_packet), Err(SessionError::VerificationFailed));
    assert!(matches!(new_device.seal(b"late"), Err(SessionError::Closed)));
}

/// 引导消息的身份、算法、版本、随机标识与角色必须满足精确合同。
#[test]
fn handshake_rejects_wrong_identity_suite_version_and_exchange_role() {
    for mutation in ["client", "device", "suite", "version", "session", "exchange", "role"] {
        let (_, mut hello) = DeviceSession::start(CLIENT, DEVICE, &SECRET, SESSION.into()).unwrap();
        match mutation {
            "client" => hello.client_id = CHALLENGE.into(),
            "device" => hello.device_id = CHALLENGE.into(),
            "suite" => hello.suite = crate::PAIRING_SUITE.into(),
            "version" => hello.version = 2,
            "session" => hello.session = "invalid".into(),
            "role" => {
                use base64::Engine as _;
                let mut bytes = base64::engine::general_purpose::STANDARD.decode(&hello.exchange).unwrap();
                bytes[0] = b'A'; hello.exchange = base64::engine::general_purpose::STANDARD.encode(bytes);
            },
            _ => hello.exchange = "AA==".into(),
        }
        assert!(AwaitingDeviceProof::accept(hello, CLIENT, DEVICE, &SECRET, CHALLENGE.into()).is_err(), "{mutation}");
    }
    assert!(DeviceSession::start(CLIENT, DEVICE, &[0; 32], SESSION.into()).is_err());
    assert!(DeviceSession::start(CLIENT, CLIENT, &SECRET, SESSION.into()).is_err());
}

/// 桌面回复元数据或确认被篡改时，终端拒绝返回可用通道。
#[test]
fn reply_tampering_and_direction_proof_reflection_fail() {
    for mutation in ["challenge", "session", "version", "proof"] {
        let (device, hello) = DeviceSession::start(CLIENT, DEVICE, &SECRET, SESSION.into()).unwrap();
        let (desktop, mut reply) = AwaitingDeviceProof::accept(hello, CLIENT, DEVICE, &SECRET, CHALLENGE.into()).unwrap();
        let reflected = SessionConfirmation { confirmation: reply.confirmation.clone() };
        assert!(matches!(desktop.verify(reflected), Err(SessionError::VerificationFailed)));
        match mutation {
            "challenge" => reply.challenge = CLIENT.into(),
            "session" => reply.session = CLIENT.into(),
            "version" => reply.version = 2,
            _ => reply.confirmation = "AA==".into(),
        }
        assert!(matches!(device.accept(reply), Err(SessionError::VerificationFailed)), "{mutation}");
    }
}

/// 重放、跳号、位翻转和错误编码均关闭双向通道，后续正确消息也不得复活。
#[test]
fn invalid_packet_permanently_closes_both_directions() {
    for mutation in ["replay", "skip", "ciphertext", "base64", "short", "oversized"] {
        let (mut desktop, mut device) = channels();
        let valid = desktop.seal(b"snapshot").unwrap(); let mut invalid = valid.clone();
        match mutation {
            "replay" => { device.open(&valid).unwrap(); },
            "skip" => invalid.sequence = 1,
            "ciphertext" => {
                use base64::Engine as _;
                let mut bytes = base64::engine::general_purpose::STANDARD.decode(&invalid.ciphertext).unwrap();
                *bytes.last_mut().unwrap() ^= 1;
                invalid.ciphertext = base64::engine::general_purpose::STANDARD.encode(bytes);
            },
            "base64" => invalid.ciphertext.push('='),
            "short" => invalid.ciphertext = "AA==".into(),
            _ => invalid.ciphertext = "A".repeat(90_000),
        }
        assert_eq!(device.open(&invalid), Err(SessionError::VerificationFailed), "{mutation}");
        assert_eq!(device.open(&valid), Err(SessionError::Closed));
        assert!(matches!(device.seal(b"response"), Err(SessionError::Closed)));
    }
}

/// 相同序号也不能把一个方向的密文反射到原发送者。
#[test]
fn data_reflection_is_rejected_in_each_direction() {
    let (mut desktop, _) = channels(); let packet = desktop.seal(b"desktop").unwrap();
    assert_eq!(desktop.open(&packet), Err(SessionError::VerificationFailed));
    let (_, mut device) = channels(); let packet = device.seal(b"device").unwrap();
    assert_eq!(device.open(&packet), Err(SessionError::VerificationFailed));
}

/// 本地空包和超限明文不消耗序号；边界大小的真实字节可完整往返。
#[test]
fn plaintext_limits_do_not_consume_nonce_for_unsent_local_errors() {
    let (mut desktop, mut device) = channels();
    assert!(matches!(desktop.seal(&[]), Err(SessionError::Invalid)));
    assert!(matches!(desktop.seal(&vec![1; MAX_PLAINTEXT_BYTES + 1]), Err(SessionError::Invalid)));
    let message = vec![0x5a; MAX_PLAINTEXT_BYTES]; let packet = desktop.seal(&message).unwrap();
    assert_eq!(packet.sequence, 0);
    assert!(serde_json::to_vec(&packet).unwrap().len() < MAX_ENCRYPTED_FRAME_BYTES);
    assert_eq!(device.open(&packet).unwrap(), message);
}

/// 未知JSON字段和缺失字段均不能被跨语言桥静默丢弃。
#[test]
fn session_wire_schema_rejects_unknown_fields() {
    assert!(serde_json::from_str::<SessionConfirmation>(r#"{"confirmation":"x","key":"secret"}"#).is_err());
    assert!(serde_json::from_str::<EncryptedPacket>(r#"{"sequence":0,"ciphertext":"x","plaintext":"secret"}"#).is_err());
    assert!(serde_json::from_str::<EncryptedPacket>(r#"{"ciphertext":"x"}"#).is_err());
}
