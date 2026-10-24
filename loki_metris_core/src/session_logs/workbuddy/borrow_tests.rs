//! 借用只减少行内临时分配，完整JSON校验、稳定身份和输出所有权不变。

use super::*;

/// 只用于行投影，不创建文件或读取真实用户目录。
fn source() -> WorkBuddyLineSource<'static> {
    WorkBuddyLineSource {
        relative_key: "project/session.jsonl",
        project_key_material: "project",
        origin: crate::WorkBuddyUsageOrigin::TopLevel,
    }
}

/// 无转义文本必须实际借用，而非只有Cow类型却仍分配String。
#[test]
fn unescaped_strings_borrow_and_valid_escapes_remain_owned() {
    let plain = br#"{"sessionId":"session-a","cwd":"/tmp/project","providerData":{"messageId":"message-a","model":"model-a"}}"#;
    let event = serde_json::from_slice::<ProjectEvent<'_>>(plain).unwrap();
    assert!(matches!(
        event.session_id.unwrap().0,
        Cow::Borrowed("session-a")
    ));
    assert!(matches!(
        event.cwd.unwrap().0,
        Cow::Borrowed("/tmp/project")
    ));
    let provider = event.provider_data.unwrap();
    assert!(matches!(
        provider.message_id.unwrap().0,
        Cow::Borrowed("message-a")
    ));
    assert!(matches!(
        provider.model.unwrap().0,
        Cow::Borrowed("model-a")
    ));
    let escaped = br#"{"sessionId":"\u0073ession-a","cwd":"\/tmp\/project","providerData":{"messageId":"\u006dessage-a","model":"\u006dodel-a"}}"#;
    let event = serde_json::from_slice::<ProjectEvent<'_>>(escaped).unwrap();
    for (actual, expected) in [
        (event.session_id.unwrap(), "session-a"),
        (event.cwd.unwrap(), "/tmp/project"),
    ] {
        assert!(matches!(&actual.0, Cow::Owned(_)));
        assert_eq!(&*actual, expected);
    }
    let provider = event.provider_data.unwrap();
    for (actual, expected) in [
        (provider.message_id.unwrap(), "message-a"),
        (provider.model.unwrap(), "model-a"),
    ] {
        assert!(matches!(&actual.0, Cow::Owned(_)));
        assert_eq!(&*actual, expected);
    }
    for line in [
        b"{}".as_slice(),
        br#"{"sessionId":null,"cwd":null,"providerData":{"messageId":null,"model":null}}"#,
    ] {
        let event = serde_json::from_slice::<ProjectEvent<'_>>(line).unwrap();
        assert!(event.session_id.is_none() && event.cwd.is_none());
        if let Some(provider) = event.provider_data {
            assert!(provider.message_id.is_none() && provider.model.is_none());
        }
    }
}

/// 即使没有usage也必须完成白名单类型、重复字段和未知内容的JSON语法校验。
#[test]
fn borrowed_projection_preserves_full_line_classification_and_warning_counts() {
    let malformed = [
        r#"{"sessionId":42}"#,
        r#"{"cwd":[]}"#,
        r#"{"providerData":{"messageId":{}}}"#,
        r#"{"providerData":{"model":true}}"#,
        r#"{"sessionId":null,"sessionId":"x"}"#,
        r#"{"cwd":"x","cwd":"y"}"#,
        r#"{"sessionId":"x","\u0073essionId":"y"}"#,
        r#"{"providerData":{"messageId":null,"messageId":"x"}}"#,
        r#"{"providerData":{"model":"x","\u006dodel":"y"}}"#,
        r#"{"providerData":null,"providerData":{}}"#,
        r#"{"unknown":{"nested":[1,]}}"#,
        r#"{} {}"#,
        r#"{"cwd":"\q"}"#,
    ];
    let mut records = Vec::new();
    let mut warnings = ProjectJsonlWarnings::default();
    for line in malformed {
        let outcome = parse_complete_line(line.as_bytes(), source(), "root");
        assert!(
            matches!(outcome, LineOutcome::Malformed),
            "类型或语法缺口必须为Malformed"
        );
        apply_line_outcome(outcome, &mut records, &mut warnings);
    }
    assert_eq!(warnings.malformed_lines, malformed.len() as u64);
    assert_eq!(warnings.total(), malformed.len() as u64);
    assert!(records.is_empty());
    for line in [
        "",
        "{}",
        r#"{"sessionId":null,"cwd":null}"#,
        r#"{"providerData":{"messageId":null,"model":null}}"#,
        r#"{"unknown":1,"unknown":2,"message":{"usage":{"totalTokens":999}}}"#,
    ] {
        assert!(matches!(
            parse_complete_line(line.as_bytes(), source(), "root"),
            LineOutcome::Ignored
        ));
    }
}

/// 返回的记录完全拥有内容，不把下一次复用行缓冲的修改带入模型或稳定身份。
#[test]
fn escaped_and_plain_records_match_after_reusing_the_input_buffer() {
    let suffix = r#", "usage":{"inputTokens":12,"inputTokensDetails":[{"cached_tokens":3}],"outputTokens":4,"totalTokens":16,"requests":1},"rawUsage":{"credit":0.25}}}"#;
    let mut plain = format!(r#"{{"timestamp":1791676800000,"sessionId":"session-a","cwd":"/tmp/project","providerData":{{"messageId":"message-a","model":"model-a"{suffix}"#).into_bytes();
    let first = match parse_complete_line(&plain, source(), "root") {
        LineOutcome::Usage(record) => record,
        _ => panic!("合法夹具必须投影usage"),
    };
    plain.fill(b'~');
    let escaped = format!(
        r#"{{"timestamp":1791676800000,"sessionId":"\u0073ession-a","cwd":"\/tmp\/project","providerData":{{"messageId":"\u006dessage-a","model":"\u006dodel-a"{suffix}"#
    );
    let second = match parse_complete_line(escaped.as_bytes(), source(), "root") {
        LineOutcome::Usage(record) => record,
        _ => panic!("合法转义必须投影usage"),
    };
    assert!(*first == *second, "解码后全部权威记录字段必须一致");
    assert_eq!(first.model.as_deref(), Some("model-a"));
    assert_eq!(first.project_label.as_deref(), Some("project"));
}
