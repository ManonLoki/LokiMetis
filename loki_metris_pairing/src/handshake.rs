//! 两端共享相同的身份编码、transcript和方向确认，内部密码状态不跨FFI暴露。

use crate::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chacha20poly1305::{ChaCha20Poly1305, Nonce, aead::{Aead, KeyInit, Payload}};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use spake2::{Ed25519Group, Identity, Password, Spake2};

/// 发送端单次交换状态，不暴露SPAKE2对象或中间秘密。
pub struct SenderSession { hello: PairingHello, exchange: Spake2<Ed25519Group> }
/// 发送端已经发出确认但仍未取得B回执，尚不能保存配对。
pub struct AwaitingAck { hello: PairingHello, reply: PairingReply, keys: ConfirmationKeys }
/// 接收端只保留当前轮的认证上下文；窗口期限和总尝试预算由宿主统一拥有。
pub struct ReceiverSession { hello: PairingHello, reply: PairingReply, keys: ConfirmationKeys }
/// 每个方向只发送一次确认，持久密钥单独派生，不能复用固定nonce发业务数据。
struct ConfirmationKeys { sender: [u8; 32], receiver: [u8; 32], persistent: [u8; 32], transcript: [u8; 32] }

/// 身份绑定协议、套件、方向与稳定UUID，防止跨服务或反射使用。
fn identities(client: &str, device: &str) -> (Identity, Identity) {
    (Identity::new(format!("LokiMetris/pair/1/{PAIRING_SUITE}/sender/{client}").as_bytes()),
     Identity::new(format!("LokiMetris/pair/1/{PAIRING_SUITE}/receiver/{device}").as_bytes()))
}

impl SenderSession {
    /// 每次用户尝试建立全新随机交换；系统随机源失败不能回退为固定随机数。
    pub fn start(client: &str, device: &str, code: &str, session: String) -> Result<(Self, PairingHello), PairingError> {
        if !valid_device_id(client) || !valid_device_id(device) || client == device || !valid_pairing_code(code) || !valid_device_id(&session) { return Err(PairingError::Invalid); }
        let (a, b) = identities(client, device);
        let (exchange, outbound) = Spake2::<Ed25519Group>::start_a(&Password::new(code.as_bytes()), &a, &b);
        let hello = PairingHello { version: 1, suite: PAIRING_SUITE.into(), client_id: client.into(), device_id: device.into(), session, exchange: STANDARD.encode(outbound) };
        Ok((Self { hello: hello.clone(), exchange }, hello))
    }

    /// 计算出密钥仍不代表密码正确，只允许生成一次A确认并继续等待B回执。
    pub fn accept(self, reply: PairingReply) -> Result<(AwaitingAck, PairingConfirmation), PairingError> {
        if reply.version != 1 || reply.session != self.hello.session || reply.device_id != self.hello.device_id
            || !valid_device_id(&reply.nonce) || !valid_device_name(&reply.name) { return Err(PairingError::VerificationFailed); }
        let shared = self.exchange.finish(&exchange_bytes(&reply.exchange)?).map_err(|_| PairingError::VerificationFailed)?;
        let keys = derive_keys(&self.hello, &reply, &shared)?;
        let confirmation = PairingConfirmation { confirmation: seal(&keys.sender, &keys.transcript, b"LokiMetris/sender-confirm/1")? };
        Ok((AwaitingAck { hello: self.hello, reply, keys }, confirmation))
    }
}

impl AwaitingAck {
    /// 认证B最终回执后才输出可保存材料；旧会话回执和方向反射失败关闭。
    pub fn verify(self, ack: PairingAck) -> Result<PairingMaterial, PairingError> {
        open(&self.keys.receiver, &self.keys.transcript, &ack.confirmation, b"LokiMetris/receiver-saved/1")?;
        Ok(PairingMaterial { client_id: self.hello.client_id, device_id: self.reply.device_id, name: self.reply.name, secret: self.keys.persistent })
    }
}

impl ReceiverSession {
    /// 宿主先预约当前码的全局尝试预算，再调用B交换；目标、套件和角色均严格复核。
    pub fn start(hello: PairingHello, own_id: &str, name: &str, code: &str, nonce: String) -> Result<(Self, PairingReply), PairingError> {
        if hello.version != 1 || hello.suite != PAIRING_SUITE || hello.device_id != own_id || hello.client_id == own_id
            || !valid_device_id(&hello.client_id) || !valid_device_id(own_id) || !valid_device_id(&hello.session)
            || !valid_pairing_code(code) || !valid_device_id(&nonce) || !valid_device_name(name) { return Err(PairingError::Invalid); }
        let (a, b) = identities(&hello.client_id, own_id);
        let (exchange, outbound) = Spake2::<Ed25519Group>::start_b(&Password::new(code.as_bytes()), &a, &b);
        let reply = PairingReply { version: 1, session: hello.session.clone(), device_id: own_id.into(), name: name.into(), nonce, exchange: STANDARD.encode(outbound) };
        let shared = exchange.finish(&exchange_bytes(&hello.exchange)?).map_err(|_| PairingError::VerificationFailed)?;
        let keys = derive_keys(&hello, &reply, &shared)?;
        Ok((Self { hello, reply: reply.clone(), keys }, reply))
    }

    /// 先验证A再调用一次私有保存；回调返回成功才构造B回执，失败绝不假成功。
    /// 宿主必须在回调中再次原子检查60秒窗口/取消状态，并完成真实持久化。
    pub fn confirm_and_commit(self, confirmation: PairingConfirmation, commit: impl FnOnce(&PairingMaterial) -> Result<(), ()>) -> Result<PairingAck, PairingError> {
        open(&self.keys.sender, &self.keys.transcript, &confirmation.confirmation, b"LokiMetris/sender-confirm/1")?;
        let material = PairingMaterial { client_id: self.hello.client_id, device_id: self.reply.device_id, name: self.reply.name, secret: self.keys.persistent };
        commit(&material).map_err(|_| PairingError::CommitFailed)?;
        Ok(PairingAck { confirmation: seal(&self.keys.receiver, &self.keys.transcript, b"LokiMetris/receiver-saved/1")? })
    }
}

/// 消息必须是规范Base64的33字节，先限制分配和异常群元素长度。
fn exchange_bytes(encoded: &str) -> Result<Vec<u8>, PairingError> {
    if encoded.len() != 44 { return Err(PairingError::VerificationFailed); }
    let bytes = STANDARD.decode(encoded).map_err(|_| PairingError::VerificationFailed)?;
    if bytes.len() != 33 || STANDARD.encode(&bytes) != encoded { return Err(PairingError::VerificationFailed); } Ok(bytes)
}

/// 固定顺序长度前缀UTF-8字段不依赖JSON键顺序；方向与名称全部参与认证。
fn derive_keys(hello: &PairingHello, reply: &PairingReply, shared: &[u8]) -> Result<ConfirmationKeys, PairingError> {
    let mut digest = Sha256::new();
    for field in ["LokiMetris/pair/1", &hello.suite, &hello.client_id, &hello.device_id, &hello.session, &hello.exchange, &reply.nonce, &reply.exchange, &reply.name] {
        digest.update((field.len() as u32).to_be_bytes()); digest.update(field.as_bytes());
    }
    let transcript: [u8; 32] = digest.finalize().into(); let hkdf = Hkdf::<Sha256>::new(Some(&transcript), shared);
    let mut keys = ConfirmationKeys { sender: [0; 32], receiver: [0; 32], persistent: [0; 32], transcript };
    hkdf.expand(b"LokiMetris/pair/1/sender-confirm", &mut keys.sender).map_err(|_| PairingError::VerificationFailed)?;
    hkdf.expand(b"LokiMetris/pair/1/receiver-confirm", &mut keys.receiver).map_err(|_| PairingError::VerificationFailed)?;
    hkdf.expand(b"LokiMetris/pair/1/persistent", &mut keys.persistent).map_err(|_| PairingError::VerificationFailed)?; Ok(keys)
}

/// 每个新会话、每个独立方向key仅加密一次固定确认，nonce0不得复用于其它消息。
fn seal(key: &[u8; 32], transcript: &[u8; 32], message: &[u8]) -> Result<String, PairingError> {
    let cipher = ChaCha20Poly1305::new_from_slice(key).map_err(|_| PairingError::VerificationFailed)?; let nonce: Nonce = [0; 12].into();
    cipher.encrypt(&nonce, Payload { msg: message, aad: transcript }).map(|bytes| STANDARD.encode(bytes)).map_err(|_| PairingError::VerificationFailed)
}

/// AEAD认证先于固定文案比较，错误统一脱敏且不向调用方返回解密内容。
fn open(key: &[u8; 32], transcript: &[u8; 32], encoded: &str, expected: &[u8]) -> Result<(), PairingError> {
    if encoded.len() > 256 { return Err(PairingError::VerificationFailed); }
    let cipher = ChaCha20Poly1305::new_from_slice(key).map_err(|_| PairingError::VerificationFailed)?;
    let bytes = STANDARD.decode(encoded).map_err(|_| PairingError::VerificationFailed)?; let nonce: Nonce = [0; 12].into();
    let message = cipher.decrypt(&nonce, Payload { msg: &bytes, aad: transcript }).map_err(|_| PairingError::VerificationFailed)?;
    if message != expected { return Err(PairingError::VerificationFailed); } Ok(())
}
