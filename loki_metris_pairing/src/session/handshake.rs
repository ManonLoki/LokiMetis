//! 恢复会话复用配对得到的高熵秘密作为PAKE输入，每次仍产生全新随机交换。

use super::{SecureChannel, SessionConfirmation, SessionError, SessionHello, SessionReply, SESSION_SUITE};
use crate::{valid_device_id, handshake::{exchange_bytes, transcript_hash}};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chacha20poly1305::{ChaCha20Poly1305, Nonce, aead::{Aead, KeyInit, Payload}};
use hkdf::Hkdf;
use sha2::Sha256;
use spake2::{Ed25519Group, Identity, Password, Spake2};

/// 终端B角色的一次性恢复状态；只能消费，不能复制临时密码状态。
pub struct DeviceSession { hello: SessionHello, exchange: Spake2<Ed25519Group> }
/// 桌面已经发送自己的确认，必须验证终端确认后才能生成业务通道。
pub struct AwaitingDeviceProof { keys: SessionKeys }
/// 四个独立用途的密钥与完整交换摘要；不实现Debug、Clone或序列化。
struct SessionKeys {
    desktop_proof: [u8; 32], device_proof: [u8; 32],
    desktop_data: [u8; 32], device_data: [u8; 32], transcript: [u8; 32],
}

impl DeviceSession {
    /// 终端以已保存密钥开始B交换；session须由宿主系统随机源生成而非复用远端字段。
    pub fn start(client_id: &str, device_id: &str, secret: &[u8; 32], session: String) -> Result<(Self, SessionHello), SessionError> {
        validate_identity(client_id, device_id, secret)?;
        if !valid_device_id(&session) { return Err(SessionError::Invalid); }
        let (a, b) = identities(client_id, device_id);
        let (exchange, bytes) = Spake2::<Ed25519Group>::start_b(&Password::new(secret), &a, &b);
        let hello = SessionHello { version: 1, suite: SESSION_SUITE.into(), client_id: client_id.into(), device_id: device_id.into(), session, exchange: STANDARD.encode(bytes) };
        Ok((Self { hello: hello.clone(), exchange }, hello))
    }

    /// 验证桌面A确认后输出终端B确认与本地通道；宿主必须先发确认再发送业务帧。
    pub fn accept(self, reply: SessionReply) -> Result<(SecureChannel, SessionConfirmation), SessionError> {
        if reply.version != 1 || reply.session != self.hello.session || !valid_device_id(&reply.challenge) {
            return Err(SessionError::VerificationFailed);
        }
        let bytes = exchange_bytes(&reply.exchange).map_err(|_| SessionError::VerificationFailed)?;
        let shared = self.exchange.finish(&bytes).map_err(|_| SessionError::VerificationFailed)?;
        let keys = derive(&self.hello, &reply, &shared)?;
        verify(&keys.desktop_proof, &keys.transcript, &reply.confirmation, b"LokiMetris/link/1/desktop-confirm")?;
        let confirmation = SessionConfirmation { confirmation: confirm(&keys.device_proof, &keys.transcript, b"LokiMetris/link/1/device-confirm")? };
        Ok((SecureChannel::new(keys.device_data, keys.desktop_data, keys.transcript), confirmation))
    }
}

impl AwaitingDeviceProof {
    /// 桌面严格匹配已保存的双端身份后开始A交换；未保存的对端不得尝试此入口。
    pub fn accept(hello: SessionHello, expected_client: &str, expected_device: &str, secret: &[u8; 32], challenge: String) -> Result<(Self, SessionReply), SessionError> {
        validate_identity(expected_client, expected_device, secret)?;
        if hello.version != 1 || hello.suite != SESSION_SUITE || hello.client_id != expected_client
            || hello.device_id != expected_device || !valid_device_id(&hello.session) || !valid_device_id(&challenge) {
            return Err(SessionError::Invalid);
        }
        let peer = exchange_bytes(&hello.exchange).map_err(|_| SessionError::VerificationFailed)?;
        let (a, b) = identities(expected_client, expected_device);
        let (exchange, outbound) = Spake2::<Ed25519Group>::start_a(&Password::new(secret), &a, &b);
        let shared = exchange.finish(&peer).map_err(|_| SessionError::VerificationFailed)?;
        let mut reply = SessionReply { version: 1, session: hello.session.clone(), challenge, exchange: STANDARD.encode(outbound), confirmation: String::new() };
        let keys = derive(&hello, &reply, &shared)?;
        reply.confirmation = confirm(&keys.desktop_proof, &keys.transcript, b"LokiMetris/link/1/desktop-confirm")?;
        Ok((Self { keys }, reply))
    }

    /// 只有对端独立方向确认通过后，桌面才可以发送或读取应用明文。
    pub fn verify(self, confirmation: SessionConfirmation) -> Result<SecureChannel, SessionError> {
        verify(&self.keys.device_proof, &self.keys.transcript, &confirmation.confirmation, b"LokiMetris/link/1/device-confirm")?;
        Ok(SecureChannel::new(self.keys.desktop_data, self.keys.device_data, self.keys.transcript))
    }
}

/// 非零高熵秘密来自配对保存，身份必须是不同的规范UUID。
fn validate_identity(client: &str, device: &str, secret: &[u8; 32]) -> Result<(), SessionError> {
    if !valid_device_id(client) || !valid_device_id(device) || client == device || *secret == [0; 32] {
        return Err(SessionError::Invalid);
    } Ok(())
}

/// 配对和恢复的密码身份域不同，业务恢复不能充当首次配对交换。
fn identities(client: &str, device: &str) -> (Identity, Identity) {
    (Identity::new(format!("LokiMetris/link/1/{SESSION_SUITE}/desktop/{client}").as_bytes()),
     Identity::new(format!("LokiMetris/link/1/{SESSION_SUITE}/device/{device}").as_bytes()))
}

/// 身份、算法、双方新交换与挑战全部参与派生，四个用途不能共用密钥。
fn derive(hello: &SessionHello, reply: &SessionReply, shared: &[u8]) -> Result<SessionKeys, SessionError> {
    let transcript = transcript_hash(&["LokiMetris/link/1", &hello.suite, &hello.client_id, &hello.device_id,
        &hello.session, &reply.challenge, &hello.exchange, &reply.exchange]);
    let hkdf = Hkdf::<Sha256>::new(Some(&transcript), shared);
    let mut keys = SessionKeys { desktop_proof: [0; 32], device_proof: [0; 32], desktop_data: [0; 32], device_data: [0; 32], transcript };
    for (label, key) in [(b"desktop-proof".as_slice(), &mut keys.desktop_proof), (b"device-proof", &mut keys.device_proof),
        (b"desktop-data", &mut keys.desktop_data), (b"device-data", &mut keys.device_data)] {
        let mut info = b"LokiMetris/link/1/".to_vec(); info.extend_from_slice(label);
        hkdf.expand(&info, key).map_err(|_| SessionError::VerificationFailed)?;
    } Ok(keys)
}

/// 每次全新交换的独立确认密钥只使用nonce0一次，不与数据方向共享。
fn confirm(key: &[u8; 32], transcript: &[u8; 32], text: &[u8]) -> Result<String, SessionError> {
    let cipher = ChaCha20Poly1305::new_from_slice(key).map_err(|_| SessionError::VerificationFailed)?;
    let nonce: Nonce = [0; 12].into();
    cipher.encrypt(&nonce, Payload { msg: text, aad: transcript }).map(|value| STANDARD.encode(value)).map_err(|_| SessionError::VerificationFailed)
}

/// 规范编码和AEAD均通过后才比较确认文案；失败不输出任何中间明文。
fn verify(key: &[u8; 32], transcript: &[u8; 32], encoded: &str, text: &[u8]) -> Result<(), SessionError> {
    if encoded.len() > 256 { return Err(SessionError::VerificationFailed); }
    let bytes = STANDARD.decode(encoded).map_err(|_| SessionError::VerificationFailed)?;
    if STANDARD.encode(&bytes) != encoded { return Err(SessionError::VerificationFailed); }
    let cipher = ChaCha20Poly1305::new_from_slice(key).map_err(|_| SessionError::VerificationFailed)?;
    let nonce: Nonce = [0; 12].into();
    let plaintext = cipher.decrypt(&nonce, Payload { msg: &bytes, aad: transcript }).map_err(|_| SessionError::VerificationFailed)?;
    if plaintext != text { return Err(SessionError::VerificationFailed); } Ok(())
}
