//! 已保存信任与配对后会话的桥接；适配器不接触账本中的持久共享密钥。

#[cfg(test)]
mod tests;
use super::{DeviceBook, PairingKey, valid_device_name};
pub use loki_metris_pairing::session::{DeviceSession, SecureChannel, EncryptedPacket, MAX_ENCRYPTED_FRAME_BYTES, MAX_PLAINTEXT_BYTES,
    SessionConfirmation, SessionHello, SessionReply, SESSION_SUITE};
use loki_metris_pairing::session::AwaitingDeviceProof;
use serde::{Deserialize, Serialize};

/// 桌面数据服务采用独立DNS-SD类型，避免与历史终端监听广告混淆。
pub const HOST_SERVICE_TYPE: &str = "_loki-host._tcp.local.";

/// 每条TCP连接第一帧区分临时配对报到与已保存设备恢复；本帧本身不授信。
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase", deny_unknown_fields)]
pub enum ConnectionIntroduction {
    /// 用户在终端主动打开配对窗口后报到。
    Pairing(PairingOffer),
    /// 已配对终端开始新的认证会话。
    Resume(SessionHello),
}

/// 只在配对窗口内有效的未认证介绍，不含配对码或秘密。
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PairingOffer {
    /// 首版引导版本。
    pub version: u8,
    /// 必须指向当前桌面UUID。
    pub client_id: String,
    /// 终端稳定UUID，完成密码验证前仍不能作为可信身份。
    pub device_id: String,
    /// 终端配对页面显示名称。
    pub name: String,
}

impl PairingOffer {
    /// 语义校验只允许当前桌面的合法介绍；网络端点由实际socket另行提供。
    pub fn validate_for(&self, client_id: &str) -> Result<(), LinkError> {
        if self.version != 1 || !super::valid_device_id(client_id) || self.client_id != client_id || self.device_id == client_id
            || !super::valid_device_id(&self.device_id) || !valid_device_name(&self.name) { return Err(LinkError::Invalid); }
        Ok(())
    }
}

/// 首版连接控制消息；每条都必须位于已认证、有序的加密帧内。
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum LinkControl {
    /// 仅用于确认应用会话可通信，不更新设备身份或统计数据。
    Ping { /// 发起端的请求编号，回应必须原样返回。
        nonce: u64 },
    /// 回应一次Ping，不能当作设备物理在线证明。
    Pong { /// 原Ping编号。
        nonce: u64 },
    /// 应用进入后台前尽力告知暂停；系统直接挂起时仍依赖超时。
    Pause {},
}

/// 局域网联动失败采用稳定类别，不返回设备秘密或原始网络内容。
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LinkError {
    /// 元信息、时间或消息格式不合法。
    #[error("device_link_invalid")]
    Invalid,
    /// 对端未保存，或本次认证使用的凭据已经被移除或替换。
    #[error("device_link_untrusted")]
    Untrusted,
    /// 密钥确认、数据认证或序号验证失败。
    #[error("device_link_authentication_failed")]
    Authentication,
    /// 当前连接已经过期或关闭，必须重新认证。
    #[error("device_link_expired")]
    Expired,
    /// 旧连接不能更新新连接状态。
    #[error("device_link_stale")]
    Stale,
    /// 设备数量或连接代次达到限制。
    #[error("device_link_limit")]
    Limit,
}

/// 直连展示终端的平台；Watch仍由iPhone协作，不冒充独立TCP对端。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DevicePlatform {
    /// Android手机或平板。
    Android,
    /// iPhone或iPad。
    Ios,
    /// macOS展示终端。
    Macos,
    /// Windows展示终端。
    Windows,
    /// Linux展示终端。
    Linux,
}

/// 只有当前认证快照广告此能力时，终端才可发送独立形态报告，保持旧端兼容。
pub const DEVICE_FORM_FACTOR_CAPABILITY: &str = "deviceFormFactorV1";

/// 终端从系统取得的展示形态，不由设备名称或网络地址推断。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeviceFormFactor {
    /// 手机尺寸类终端。
    Phone,
    /// 平板尺寸类终端。
    Tablet,
}

/// 仅在加密会话内接收的展示元信息；IP由实际socket取得，不允许在此自报覆盖。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceInfo {
    /// 必须与完成本次会话认证的终端身份相同。
    pub device_id: String,
    /// 应用内展示名称，不能承诺系统总能返回用户定义的设备名。
    pub name: String,
    /// 决定终端展示和生命周期说明的平台信息。
    pub platform: DevicePlatform,
    /// 终端应用版本，仅作有界展示，不参与本机版本判断。
    pub app_version: String,
}

impl DeviceInfo {
    /// 元信息不能改变认证身份，名称和版本均限制长度与控制字符。
    pub fn validate_for(&self, device_id: &str) -> Result<(), LinkError> {
        if self.device_id != device_id || !valid_device_name(&self.name) || self.app_version.trim().is_empty()
            || self.app_version.len() > 64 || self.app_version.chars().any(char::is_control) {
            return Err(LinkError::Invalid);
        } Ok(())
    }
}

/// 桌面等待终端确认的不可伪造状态；认证前不创建在线记录。
pub struct PendingLinkSession { client_id: String, device_id: String, credential: PairingKey, waiting: AwaitingDeviceProof }
/// 已双向认证会话独占通道与凭据绑定；不能克隆或序列化到页面。
pub struct AuthenticatedLinkSession { client_id: String, device_id: String, credential: PairingKey, channel: SecureChannel, presence_claimed: bool }
/// 在线记录保留本次凭据绑定，避免移除后同ID重新配对继承旧在线状态。
pub(super) struct PresenceTrust { client_id: String, device_id: String, credential: PairingKey }
impl PresenceTrust {
    /// 只有同一桌面、同一设备及同一保存密钥仍存在时才投影旧会话状态。
    pub(super) fn authorized_by(&self, book: &DeviceBook) -> bool {
        book.client_id == self.client_id && book.devices.iter().any(|saved| saved.id == self.device_id && saved.key.0 == self.credential.0)
    }
}

impl DeviceBook {
    /// 只按账本精确身份查找密钥并建立恢复交换；发现广告不能授予此资格。
    pub fn begin_link_session(&self, hello: SessionHello, challenge: String) -> Result<(PendingLinkSession, SessionReply), LinkError> {
        if hello.client_id != self.client_id { return Err(LinkError::Untrusted); }
        let saved = self.devices.iter().find(|device| device.id == hello.device_id).ok_or(LinkError::Untrusted)?;
        let (waiting, reply) = AwaitingDeviceProof::accept(hello, &self.client_id, &saved.id, &saved.key.0, challenge).map_err(|_| LinkError::Authentication)?;
        Ok((PendingLinkSession { client_id: self.client_id.clone(), device_id: saved.id.clone(), credential: saved.key.clone(), waiting }, reply))
    }
}

impl PendingLinkSession {
    /// 终端确认通过后才产生可供presence接受的会话证明。
    pub fn verify(self, confirmation: SessionConfirmation) -> Result<AuthenticatedLinkSession, LinkError> {
        let channel = self.waiting.verify(confirmation).map_err(|_| LinkError::Authentication)?;
        Ok(AuthenticatedLinkSession { client_id: self.client_id, device_id: self.device_id, credential: self.credential, channel, presence_claimed: false })
    }
}

impl AuthenticatedLinkSession {
    /// 返回本次密码学认证的身份，不能由后续JSON元信息改写。
    pub fn device_id(&self) -> &str { &self.device_id }

    /// 凭据在握手期间被移除或替换时，旧会话即使密码确认成功也不能成为当前在线设备。
    pub(super) fn authorized_by(&self, book: &DeviceBook) -> bool {
        book.client_id == self.client_id && book.devices.iter().any(|saved| saved.id == self.device_id && saved.key.0 == self.credential.0)
    }

    /// 每次真实认证只能绑定一次在线租约；旧会话超时或密码认证失败后不能再次上线。
    pub(super) fn claim_presence(&mut self, book: &DeviceBook) -> Result<PresenceTrust, LinkError> {
        if !self.authorized_by(book) { return Err(LinkError::Untrusted); }
        if self.presence_claimed || !self.channel.is_open() { return Err(LinkError::Expired); }
        self.presence_claimed = true;
        Ok(PresenceTrust { client_id: self.client_id.clone(), device_id: self.device_id.clone(), credential: self.credential.clone() })
    }

    /// 加密有界应用消息，不向适配器暴露任何会话密钥。
    pub fn seal(&mut self, plaintext: &[u8]) -> Result<EncryptedPacket, LinkError> {
        self.channel.seal(plaintext).map_err(|_| LinkError::Authentication)
    }

    /// 返回通过认证的明文；无效密文会永久关闭共享库内的双向通道。
    pub fn open(&mut self, packet: &EncryptedPacket) -> Result<Vec<u8>, LinkError> {
        self.channel.open(packet).map_err(|_| LinkError::Authentication)
    }
}
pub mod data;
pub mod assets;
