//! 配对后以高熵持久密钥建立全新双向认证通道；与四位码配对使用不同协议域。
//! 宿主拥有网络、帧长、超时、并发与系统随机UUID；本模块不执行任何I/O。

mod channel;
mod handshake;
#[cfg(test)]
mod tests;

pub use channel::SecureChannel;
pub use handshake::{AwaitingDeviceProof, DeviceSession};
use serde::{Deserialize, Serialize};

/// 业务会话套件独立于首次配对，不能在同名协议下静默替换算法。
pub const SESSION_SUITE: &str = "spake2-0.4-ed25519-hkdfsha256-chacha20poly1305-link-v1";
/// 单包明文硬上限；大图片由上层有界分块，不能扩大本限制。
pub const MAX_PLAINTEXT_BYTES: usize = 64 * 1024;
/// JSON帧在反序列化前的传输上限，包含Base64膨胀和固定字段。
pub const MAX_ENCRYPTED_FRAME_BYTES: usize = 96 * 1024;
/// 单方向最多发送2^32包，达到后必须重新建立具有全新交换的会话。
pub const MAX_CHANNEL_PACKETS: u64 = 1 << 32;

/// 会话失败只暴露稳定类别；不得携带密钥、明文或原始输入。
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SessionError {
    /// 本地参数、身份或有界消息格式不合法。
    #[error("device_session_invalid")]
    Invalid,
    /// 对端确认、消息认证、方向或序号错误。
    #[error("device_session_verification_failed")]
    VerificationFailed,
    /// 会话已因失败或序号耗尽关闭，不允许继续使用任一方向。
    #[error("device_session_closed")]
    Closed,
}

/// 终端主动连接后发出的未认证引导；桌面必须按两个身份查找已保存凭据。
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionHello {
    /// 固定版本1。
    pub version: u8,
    /// 固定SESSION_SUITE，不接受算法降级。
    pub suite: String,
    /// 预期桌面的稳定配对身份。
    pub client_id: String,
    /// 发起终端的稳定配对身份。
    pub device_id: String,
    /// 由终端以系统随机源新生成的UUID。
    pub session: String,
    /// 终端B角色全新SPAKE2消息的规范Base64编码。
    pub exchange: String,
}

/// 桌面A角色的全新交换与密钥确认；终端验证前不接受业务数据。
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionReply {
    /// 固定版本1。
    pub version: u8,
    /// 回显终端本次session。
    pub session: String,
    /// 由桌面以系统随机源新生成的UUID。
    pub challenge: String,
    /// 桌面A角色全新SPAKE2消息的规范Base64编码。
    pub exchange: String,
    /// 独立桌面确认密钥认证的固定文案。
    pub confirmation: String,
}

/// 终端验证桌面后返回的独立方向密钥确认。
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionConfirmation {
    /// 不含配对凭据或应用数据的认证确认。
    pub confirmation: String,
}

/// 双向通道的数据包；序号与密文均被AEAD认证，不能重排、重放或反射。
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncryptedPacket {
    /// 每方向从0开始严格递增，不因重发或断线回退。
    pub sequence: u64,
    /// 含16字节tag的规范Base64密文。
    pub ciphertext: String,
}
