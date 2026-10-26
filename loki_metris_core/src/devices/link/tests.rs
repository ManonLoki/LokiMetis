use super::*;
use crate::devices::tests::link_fixture;
use loki_metris_pairing::session::DeviceSession;

/// 账本查找精确双端身份，未配对及错误桌面不能借广告身份进入恢复握手。
#[test]
fn only_saved_device_and_exact_desktop_identity_can_resume() {
    let (book, _, _) = link_fixture();
    let id = book.summaries()[0].id.clone();
    let (_, hello) = DeviceSession::start(book.client_id(), &id, &[0xa7; 32], "dddddddd-dddd-4ddd-8ddd-dddddddddddd".into()).unwrap();
    let empty = DeviceBook::new(book.client_id().into()).unwrap();
    assert!(matches!(empty.begin_link_session(hello.clone(), "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into()), Err(LinkError::Untrusted)));
    let other = DeviceBook::new("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee".into()).unwrap();
    assert!(matches!(other.begin_link_session(hello, "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into()), Err(LinkError::Untrusted)));
}

/// 恢复握手完成后使用同一业务通道传真实字节，不通过页面或Debug暴露密钥。
#[test]
fn saved_credentials_produce_bidirectional_authenticated_channel() {
    let (_, mut desktop, mut device) = link_fixture();
    assert_eq!(device.open(&desktop.seal(b"usage snapshot").unwrap()).unwrap(), b"usage snapshot");
    assert_eq!(desktop.open(&device.seal(b"device metadata").unwrap()).unwrap(), b"device metadata");
}

/// 移除、同ID换密钥或更换桌面身份都会撤销旧认证的保存资格。
#[test]
fn saved_trust_changes_invalidate_previously_authenticated_session() {
    let (book, session, _) = link_fixture();
    assert!(session.authorized_by(&book));
    assert!(!session.authorized_by(&book.without(session.device_id()).unwrap()));
    let mut replacement = book.clone(); replacement.devices[0].key = PairingKey([0xb8; 32]);
    assert!(!session.authorized_by(&replacement));
    let mut other_desktop = book; other_desktop.client_id = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee".into();
    assert!(!session.authorized_by(&other_desktop));
}

/// 设备元信息不能改认证身份、自报权威IP或夹带控制符。
#[test]
fn metadata_validates_identity_name_and_bounded_version() {
    let (_, session, _) = link_fixture();
    let valid = DeviceInfo { device_id: session.device_id().into(), name: "My iPhone".into(), platform: DevicePlatform::Ios, app_version: "1.0".into() };
    valid.validate_for(session.device_id()).unwrap();
    for mutation in ["identity", "name", "version", "long"] {
        let mut info = valid.clone();
        match mutation {
            "identity" => info.device_id = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee".into(),
            "name" => info.name = "bad\nname".into(),
            "version" => info.app_version = "   ".into(),
            _ => info.app_version = "1".repeat(65),
        }
        assert_eq!(info.validate_for(session.device_id()), Err(LinkError::Invalid));
    }
    let mut value = serde_json::to_value(valid).unwrap(); value["ip"] = "192.168.1.9".into();
    assert!(serde_json::from_value::<DeviceInfo>(value).is_err());
}

/// 报到不能伪造桌面目标、版本或身份，连接引导和控制帧拒绝未知字段。
#[test]
fn introduction_and_control_messages_have_strict_bounded_semantics() {
    let (book, session, _) = link_fixture();
    let valid = PairingOffer { version:1, client_id:book.client_id().into(), device_id:session.device_id().into(), name:"Phone".into() };
    valid.validate_for(book.client_id()).unwrap();
    let mut invalid = valid.clone(); invalid.version = 2; assert_eq!(invalid.validate_for(book.client_id()), Err(LinkError::Invalid));
    let mut invalid = valid.clone(); invalid.device_id = book.client_id().into(); assert_eq!(invalid.validate_for(book.client_id()), Err(LinkError::Invalid));
    let mut invalid = valid; invalid.client_id = "invalid".into(); assert_eq!(invalid.validate_for("invalid"), Err(LinkError::Invalid));
    assert!(serde_json::from_str::<LinkControl>(r#"{"type":"pong","nonce":0,"ip":"192.168.1.1"}"#).is_err());
    assert!(serde_json::from_str::<ConnectionIntroduction>(r#"{"kind":"pairing","value":{},"trusted":true}"#).is_err());
}


/// 桌面展示终端使用真实平台值；新增标识不放宽身份绑定或未知平台检查。
#[test]
fn display_terminal_platforms_preserve_authenticated_metadata_validation() {
    let (_, session, _) = link_fixture();
    for platform in ["android", "ios", "macos", "windows", "linux"] {
        let value = serde_json::json!({
            "deviceId": session.device_id(), "name": "Display terminal",
            "platform": platform, "appVersion": "1.0.0"
        });
        let info: DeviceInfo = serde_json::from_value(value.clone()).unwrap();
        info.validate_for(session.device_id()).unwrap();
        assert_eq!(serde_json::to_value(info).unwrap(), value);
        let mut wrong_identity = value;
        wrong_identity["deviceId"] = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee".into();
        let info: DeviceInfo = serde_json::from_value(wrong_identity).unwrap();
        assert_eq!(info.validate_for(session.device_id()), Err(LinkError::Invalid));
    }
    for unknown in ["watchos", "unknown", "", "iOS"] {
        assert!(serde_json::from_value::<DevicePlatform>(unknown.into()).is_err());
    }
}
