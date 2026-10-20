//! 协议测试直接覆盖共享A/B状态机，移动桥不必另写密码算法。

use super::*;
use std::cell::Cell;

const CLIENT: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const DEVICE: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const SESSION: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const NONCE: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

/// 模拟两端语言桥的JSON边界，不把内部密码状态序列化。
pub(crate) fn wire<T: serde::Serialize + serde::de::DeserializeOwned>(value: &T) -> T {
    serde_json::from_slice(&serde_json::to_vec(value).unwrap()).unwrap()
}

/// A/B经过真实交换和四次JSON往返，B提交的身份/密钥必须与A最终结果一致。
#[test]
fn both_roles_agree_after_commit_and_json_roundtrip() {
    let (sender, hello) = SenderSession::start(CLIENT, DEVICE, "0037", SESSION.into()).unwrap();
    let (receiver, reply) = ReceiverSession::start(wire(&hello), DEVICE, "Display", "0037", NONCE.into()).unwrap();
    let (sender, confirmation) = sender.accept(wire(&reply)).unwrap();
    let mut committed = None;
    let ack = receiver.confirm_and_commit(wire(&confirmation), |material| {
        assert_eq!(material.client_id(), CLIENT); assert_eq!(material.device_id(), DEVICE);
        assert_eq!(material.device_name(), "Display"); committed = Some(*material.secret_bytes()); Ok(())
    }).unwrap();
    let material = sender.verify(wire(&ack)).unwrap();
    assert_eq!(*material.secret_bytes(), committed.unwrap());
    assert_ne!(*material.secret_bytes(), [0; 32]);
    let hello = serde_json::to_value(hello).unwrap();
    assert!(hello.get("code").is_none() && hello.get("password").is_none());
}

/// 错码和篡改元数据均不能触发私有保存回调。
#[test]
fn unauthenticated_peer_cannot_reach_commit() {
    for mutation in ["wrong-code", "name", "nonce"] {
        let (sender, hello) = SenderSession::start(CLIENT, DEVICE, "0037", SESSION.into()).unwrap();
        let code = if mutation == "wrong-code" { "0038" } else { "0037" };
        let (receiver, mut reply) = ReceiverSession::start(hello, DEVICE, "Display", code, NONCE.into()).unwrap();
        if mutation == "name" { reply.name = "Impostor".into(); }
        if mutation == "nonce" { reply.nonce = CLIENT.into(); }
        let (_, confirmation) = sender.accept(reply).unwrap(); let invoked = Cell::new(false);
        assert_eq!(receiver.confirm_and_commit(confirmation, |_| { invoked.set(true); Ok(()) }).unwrap_err(), PairingError::VerificationFailed);
        assert!(!invoked.get());
    }
}

/// 接收端宿主的原子存储/窗口复核失败必须阻止成功回执，不能把内存交换当作保存。
#[test]
fn failed_commit_never_returns_success_ack() {
    let (sender, hello) = SenderSession::start(CLIENT, DEVICE, "0037", SESSION.into()).unwrap();
    let (receiver, reply) = ReceiverSession::start(hello, DEVICE, "Display", "0037", NONCE.into()).unwrap();
    let (_, confirmation) = sender.accept(reply).unwrap(); let calls = Cell::new(0);
    assert_eq!(receiver.confirm_and_commit(confirmation, |_| { calls.set(calls.get() + 1); Err(()) }).unwrap_err(), PairingError::CommitFailed);
    assert_eq!(calls.get(), 1);
}

/// 就算宿主误复用session/nonce，全新随机PAKE消息也不能接受旧回执或方向反射。
#[test]
fn reflection_and_replay_fail_with_reused_host_identifiers() {
    let (sender, hello) = SenderSession::start(CLIENT, DEVICE, "0037", SESSION.into()).unwrap();
    let (receiver, reply) = ReceiverSession::start(hello, DEVICE, "Display", "0037", NONCE.into()).unwrap();
    let (sender, confirmation) = sender.accept(reply).unwrap();
    let ack = receiver.confirm_and_commit(confirmation.clone(), |_| Ok(())).unwrap();
    assert!(sender.verify(ack.clone()).is_ok());
    for reflected in [false, true] {
        let (sender, hello) = SenderSession::start(CLIENT, DEVICE, "0037", SESSION.into()).unwrap();
        let (_, reply) = ReceiverSession::start(hello, DEVICE, "Display", "0037", NONCE.into()).unwrap();
        let (sender, confirmation) = sender.accept(reply).unwrap();
        let ack = if reflected { PairingAck { confirmation: confirmation.confirmation } } else { ack.clone() };
        assert!(sender.verify(ack).is_err());
    }
}

/// 套件、目标、版本和角色必须精确匹配，畸形交换在进入确认前失败关闭。
#[test]
fn receiver_rejects_unknown_suite_identity_version_and_message_role() {
    for mutation in ["suite", "version", "device", "short", "role"] {
        let (_, mut hello) = SenderSession::start(CLIENT, DEVICE, "0037", SESSION.into()).unwrap();
        match mutation {
            "suite" => hello.suite = "RFC9382".into(),
            "version" => hello.version = 2,
            "device" => hello.device_id = CLIENT.into(),
            "short" => hello.exchange = "AA==".into(),
            _ => {
                use base64::Engine as _;
                let mut bytes = base64::engine::general_purpose::STANDARD.decode(&hello.exchange).unwrap();
                bytes[0] = b'B'; hello.exchange = base64::engine::general_purpose::STANDARD.encode(bytes);
            },
        }
        assert!(ReceiverSession::start(hello, DEVICE, "Display", "0037", NONCE.into()).is_err(), "{mutation}");
    }
}

/// 前导零保留；全角数字、未知JSON字段和空身份不能被桥接层宽松转换。
#[test]
fn code_and_wire_schema_are_strict() {
    assert!(valid_pairing_code("0000"));
    for code in ["123", "12345", "１２３４", " 123", "12a4"] { assert!(!valid_pairing_code(code)); }
    assert!(SenderSession::start(CLIENT, CLIENT, "0037", SESSION.into()).is_err());
    assert!(SenderSession::start(CLIENT, DEVICE, "0037", "".into()).is_err());
    assert!(serde_json::from_str::<PairingAck>(r#"{"confirmation":"x","code":"0037"}"#).is_err());
}
