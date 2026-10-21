//! 桌面与移动端共用的轻量SPAKE2协议；不依赖GUI、数据库、网络或异步运行时。
//! 本库采用RustCrypto spake2 0.4.0的Ed25519变体，不声称与任意RFC9382实现互通。

#![deny(missing_docs)]
#![forbid(unsafe_code)]

mod handshake;
pub mod session;
pub mod window;
pub mod assets;
pub mod widget;
#[cfg(test)]
mod tests;
pub use handshake::{AwaitingAck, ReceiverSession, PendingReceiverCommit, SenderSession};
use serde::{Deserialize, Serialize};

/// 精确算法套件是互操作合同，不能只按“SPAKE2”名称选择另一个变体。
pub const PAIRING_SUITE: &str = "spake2-0.4-ed25519-hkdfsha256-chacha20poly1305-v1";

/// 稳定脱敏错误可直接跨JNI/C桥映射，不包含配对码、共享密钥或用户输入。
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PairingError {
    /// 身份、版本、套件或消息格式非法。
    #[error("pairing_invalid")]
    Invalid,
    /// 密码交换、认证确认或会话绑定失败。
    #[error("pairing_verification_failed")]
    VerificationFailed,
    /// 接收端的保存回调失败，不得发送成功回执。
    #[error("pairing_commit_failed")]
    CommitFailed,
}

/// 发送端固定为SPAKE2角色A，所有身份与会话字段参与认证。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairingHello {
    /// 固定协议版本1。
    pub version: u8,
    /// 必须精确等于PAIRING_SUITE。
    pub suite: String,
    /// 发送端稳定UUID。
    pub client_id: String,
    /// 用户选中的接收端UUID。
    pub device_id: String,
    /// 每次尝试全新生成的UUID。
    pub session: String,
    /// 标准带padding Base64编码的33字节SPAKE2 A消息。
    pub exchange: String,
}

/// 接收端固定为角色B，其名称与随机nonce在最终确认前仍不可信。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairingReply {
    /// 必须与Hello相同。
    pub version: u8,
    /// 回显本轮会话UUID。
    pub session: String,
    /// 必须与本机接收端身份相同。
    pub device_id: String,
    /// 接收端实际名称，双向确认后才可信。
    pub name: String,
    /// 接收端每轮新生成的随机UUID。
    pub nonce: String,
    /// 标准带padding Base64编码的33字节SPAKE2 B消息。
    pub exchange: String,
}

/// A方向的认证确认；它本身不表示接收端已保存。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairingConfirmation {
    /// A方向密钥认证加密的固定确认文案。
    pub confirmation: String,
}

/// B只在验证A并完成存储回调后产生的认证回执。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairingAck {
    /// B方向密钥认证加密的固定保存确认文案。
    pub confirmation: String,
}

/// 认证后的配对材料；不实现Debug/Serialize，秘密只显式交给私有存储。
pub struct PairingMaterial {
    client_id: String,
    device_id: String,
    name: String,
    secret: [u8; 32],
}

impl PairingMaterial {
    /// 返回被同一次密码交换认证的发送端身份。
    pub fn client_id(&self) -> &str { &self.client_id }
    /// 返回被同一次密码交换认证的接收端身份。
    pub fn device_id(&self) -> &str { &self.device_id }
    /// 返回认证后的接收端名称。
    pub fn device_name(&self) -> &str { &self.name }
    /// 仅用于私有持久化；不得输出日志、页面或跨信任边界的调试对象。
    pub fn secret_bytes(&self) -> &[u8; 32] { &self.secret }
}

/// 四位ASCII十进制码保留前导零，不接受空格或Unicode数字。
pub fn valid_pairing_code(code: &str) -> bool { code.len() == 4 && code.bytes().all(|byte| byte.is_ascii_digit()) }

/// 身份采用规范小写非零UUID，不能使用IP或名称代替。
pub fn valid_device_id(value: &str) -> bool {
    value.len() == 36 && value.bytes().enumerate().all(|(index, byte)| {
        if [8, 13, 18, 23].contains(&index) { byte == b'-' }
        else { byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte) }
    }) && value != "00000000-0000-0000-0000-000000000000"
}

/// 名称只作展示，拒绝控制字符、空白和超过160 UTF-8字节的输入。
pub fn valid_device_name(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 160 && !value.chars().any(char::is_control)
}
