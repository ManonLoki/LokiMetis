//! Cursor 会话协议回归；全部令牌为合成夹具，不读取真实登录材料。

use base64::{Engine as _, engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD}};
use loki_metris_core::QuotaFailure;
use serde_json::{Value, json};

use super::credentials::{Secret, cursor_session_cookie};

/// 以无签名意义的固定段构造协议夹具，上游认证不在本机测试中模拟。
pub(super) fn jwt(payload: Value) -> String {
    format!("e30.{}.fixture-signature", URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap()))
}

/// 从 Cursor 当前 JWT subject 取用户ID，仅将组合后的会话交给请求层。
#[test]
fn cursor_jwt_subject_builds_session_cookie() {
    for subject in ["auth0|user_fixture", "user_fixture"] {
        let token = jwt(json!({"sub": subject}));
        let secret = Secret::new(&token).unwrap();
        let session = cursor_session_cookie(&secret).unwrap();
        assert_eq!(session.expose(), format!("user_fixture%3A%3A{token}"));
        assert_eq!(cursor_session_cookie(&session).unwrap().expose(), session.expose());
        assert!(!format!("{secret:?} {session:?}").contains(&token));
    }
}

/// JWT payload 同时兼容有填充和无填充的 base64url。
#[test]
fn cursor_jwt_accepts_padded_payload() {
    let token = format!("e30.{}.signature", URL_SAFE.encode(br#"{"sub":"user_a"}"#));
    assert!(token.contains('='));
    let session = cursor_session_cookie(&Secret::new(&token).unwrap()).unwrap();
    assert_eq!(session.expose(), format!("user_a%3A%3A{token}"));
}

/// 已有完整会话不重复添加用户ID，分隔符编码大小写统一且不通用解码。
#[test]
fn cursor_existing_sessions_are_normalized_once() {
    for delimiter in ["::", "%3A%3A", "%3a%3a", "%3a%3A"] {
        let secret = Secret::new(format!("user_fixture{delimiter}opaque-token")).unwrap();
        let session = cursor_session_cookie(&secret).unwrap();
        assert_eq!(session.expose(), "user_fixture%3A%3Aopaque-token");
        assert_eq!(cursor_session_cookie(&session).unwrap().expose(), session.expose());
    }
}

/// 缺失或损坏的协议字段在请求前失败，不猜测用户身份。
#[test]
fn cursor_invalid_jwt_or_subject_is_rejected() {
    let mut invalid = vec![
        "opaque-token".to_owned(), "a.b".to_owned(), "a.b.c.d".to_owned(),
        ".e30.c".to_owned(), "a..c".to_owned(), "a.e30.".to_owned(), "a.%%%.c".to_owned(),
        format!("a.{}.c", URL_SAFE_NO_PAD.encode(b"not-json")),
    ];
    for value in [json!({}), json!([]), json!(null), json!({"sub": 1}), json!({"sub": ""})] {
        invalid.push(jwt(value));
    }
    for subject in ["auth0|wrong_fixture", "auth0|user_", "auth0|user_good|", "auth0|user_a;b", "user_a\r\nX", "user_a b", "user_a\"b", "user_中文", "user_a%3Ab", "user_a:b"] {
        invalid.push(jwt(json!({"sub": subject})));
    }
    invalid.push(jwt(json!({"sub": format!("user_{}", "a".repeat(124))})));
    invalid.extend(["user_::token", "wrong_id::token", "user_good::", "user_good::token;other=1", "user_good::token%3Bother", "user_good::user_other::token"].map(str::to_owned));
    for token in invalid {
        let result = Secret::new(token).ok_or(QuotaFailure::Malformed)
            .and_then(|secret| cursor_session_cookie(&secret));
        assert!(matches!(result, Err(QuotaFailure::Malformed)), "invalid session must not reach the network");
    }
}

/// 最终会话连同 Cookie 固定键名受请求头上限约束，长令牌不能在拼接时越界。
#[test]
fn cursor_session_respects_final_cookie_header_limit() {
    let prefix = "user_a%3A%3A";
    let max_token = 8192 - "WorkosCursorSessionToken=".len() - prefix.len();
    let boundary = Secret::new(format!("user_a::{}", "x".repeat(max_token))).unwrap();
    let session = cursor_session_cookie(&boundary).unwrap();
    assert_eq!(session.expose().len() + "WorkosCursorSessionToken=".len(), 8192);
    let oversized = Secret::new(format!("user_a::{}", "x".repeat(max_token + 1))).unwrap();
    assert!(matches!(cursor_session_cookie(&oversized), Err(QuotaFailure::Malformed)));
    assert!(Secret::new("x".repeat(8193)).is_none());
}
