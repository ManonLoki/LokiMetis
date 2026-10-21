//! 局域网设备身份、配对凭据与保存规则；发现广告本身不授予信任。

mod pairing;
pub mod link;
pub mod presence;
pub mod companion;
pub mod widget;
#[cfg(test)]
mod tests;

pub use pairing::{AwaitingPairingAck, PairingSession};
pub use loki_metris_pairing::{PAIRING_SUITE, PairingAck, PairingConfirmation, PairingHello, PairingReply,
    valid_device_id, valid_device_name, valid_pairing_code};
#[cfg(any(test, feature = "test-support"))]
pub use pairing::pairing_test_reply;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// 仅浏览本产品接收端的 DNS-SD 服务，不能把打印机等普通广告当作可配对设备。
pub const DEVICE_SERVICE_TYPE: &str = "_loki-metris._tcp.local.";
/// 首版配对协议；未知版本不能回退为未认证连接。
pub const DEVICE_PROTOCOL_VERSION: u8 = 1;
/// 保存设备的数量上限，同时限制本地文件大小和页面工作集。
pub const MAX_SAVED_DEVICES: usize = 64;
/// 一轮发现允许展示的候选上限。
pub const MAX_DISCOVERED_DEVICES: usize = 128;

/// 领域错误只暴露稳定类别，不包含地址、配对码或共享密钥。
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DeviceError {
    /// 广告、身份、端点或协议格式非法。
    #[error("device_invalid")]
    Invalid,
    /// 密码交换或双向确认失败。
    #[error("device_pairing_failed")]
    PairingFailed,
    /// 已存在的设备不能被发现广告或重新配对静默覆盖。
    #[error("device_already_saved")]
    AlreadySaved,
    /// 保存列表已经达到有界容量。
    #[error("device_limit")]
    Limit,
    /// 目标已从保存列表移除。
    #[error("device_not_saved")]
    NotSaved,
}

/// 局域网端点只接受私网、链路本地及ULA；IPv6链路本地必须携带接口作用域。
pub fn is_lan_endpoint(endpoint: SocketAddr) -> bool {
    if endpoint.port() == 0 { return false; }
    match endpoint {
        SocketAddr::V4(value) => value.ip().is_private() || value.ip().is_link_local(),
        SocketAddr::V6(value) => value.ip().is_unique_local()
            || (value.ip().is_unicast_link_local() && value.scope_id() != 0),
    }
}

/// 未认证的发现候选只存在内存中；配对时固定本轮端点，不能被后到广告替换。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredDevice {
    /// 接收端稳定身份。
    pub id: String,
    /// 接收端宣告的展示名称，不代表已经认证。
    pub name: String,
    /// 当前发现的局域网连接地址，最多八个。
    pub endpoints: Vec<SocketAddr>,
    /// 只有明确宣告pair=1的接收端才提供约定的SPAKE2配对；广播本身仍不授信。
    pub pairing_available: bool,
}

impl DiscoveredDevice {
    /// 验证广告必填身份、名称和有界局域网端点；不把它加入保存列表。
    pub fn new(id: String, name: String, mut endpoints: Vec<SocketAddr>) -> Result<Self, DeviceError> {
        if !valid_device_id(&id) || !valid_device_name(&name) { return Err(DeviceError::Invalid); }
        endpoints.retain(|value| is_lan_endpoint(*value));
        endpoints.sort(); endpoints.dedup(); endpoints.truncate(8);
        if endpoints.is_empty() { return Err(DeviceError::Invalid); }
        Ok(Self { id, name, endpoints, pairing_available: false })
    }

    /// 仅解释发现协议的能力声明，不代表已经认证或配对。
    pub fn with_pairing_protocol(mut self, advertised: Option<&str>) -> Self {
        self.pairing_available = advertised == Some("1"); self
    }
}

/// GUI只取得保存身份与名称，不能从页面快照读出配对凭据。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedDeviceSummary {
    /// 已完成双向验证的接收端身份。
    pub id: String,
    /// 配对时选中的名称；新广告不能改写它。
    pub name: String,
}

/// 共享密钥仅在领域与私有存储之间流动；Debug始终脱敏。
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct PairingKey([u8; 32]);
impl std::fmt::Debug for PairingKey {
    /// 调试输出不泄露设备共享密钥。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PairingKey([redacted])")
    }
}

/// 只有已验证接收端最终回执的配对会话能构造此凭据。
pub struct VerifiedPairing {
    client_id: String,
    device_id: String,
    name: String,
    key: PairingKey,
}

/// 本地单条保存记录；端点不持久化，避免IP变化或复用绕过身份绑定。
#[derive(Clone, Debug, Serialize, Deserialize)]
struct SavedDevice {
    id: String,
    name: String,
    key: PairingKey,
}

/// 设备权威列表在完整校验后加载；损坏文件不能按空列表覆盖。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceBook {
    schema_version: u8,
    client_id: String,
    devices: Vec<SavedDevice>,
}

impl DeviceBook {
    /// 首次创建空账本只分配本机身份，不保存任何发现候选。
    pub fn new(client_id: String) -> Result<Self, DeviceError> {
        if !valid_device_id(&client_id) { return Err(DeviceError::Invalid); }
        Ok(Self { schema_version: 1, client_id, devices: Vec::new() })
    }

    /// 返回绑定密码交换的本机稳定身份。
    pub fn client_id(&self) -> &str { &self.client_id }

    /// 返回无凭据的展示列表；调用方不能篡改权威记录。
    pub fn summaries(&self) -> Vec<SavedDeviceSummary> {
        self.devices.iter().map(|device| SavedDeviceSummary { id: device.id.clone(), name: device.name.clone() }).collect()
    }

    /// 从私有文件解码并验证整个快照，拒绝重复身份、容量和零密钥。
    pub fn decode(bytes: &[u8]) -> Result<Self, DeviceError> {
        if bytes.len() > 64 * 1024 { return Err(DeviceError::Invalid); }
        let value: Self = serde_json::from_slice(bytes).map_err(|_| DeviceError::Invalid)?;
        if value.schema_version != 1 || !valid_device_id(&value.client_id) || value.devices.len() > MAX_SAVED_DEVICES {
            return Err(DeviceError::Invalid);
        }
        let mut ids = std::collections::BTreeSet::new();
        for device in &value.devices {
            if !valid_device_id(&device.id) || device.id == value.client_id || !valid_device_name(&device.name)
                || !ids.insert(&device.id) || device.key.0 == [0; 32] { return Err(DeviceError::Invalid); }
        }
        Ok(value)
    }

    /// 编码只能交给私有原子存储，不能记录日志或返回页面。
    pub fn encode_private(&self) -> Result<Vec<u8>, DeviceError> {
        serde_json::to_vec_pretty(self).map_err(|_| DeviceError::Invalid)
    }

    /// 消费双向配对证明生成新快照；调用方真实落盘成功后才能替换旧账本。
    pub fn paired(&self, proof: VerifiedPairing) -> Result<Self, DeviceError> {
        if proof.client_id != self.client_id || proof.device_id == self.client_id { return Err(DeviceError::Invalid); }
        if self.devices.iter().any(|device| device.id == proof.device_id) { return Err(DeviceError::AlreadySaved); }
        if self.devices.len() >= MAX_SAVED_DEVICES { return Err(DeviceError::Limit); }
        let mut value = self.clone();
        value.devices.push(SavedDevice { id: proof.device_id, name: proof.name, key: proof.key });
        Ok(value)
    }

    /// 忘记设备只删除本机信任；接收端撤销在双方管理协议确定后另行实现。
    pub fn without(&self, id: &str) -> Result<Self, DeviceError> {
        if !self.devices.iter().any(|device| device.id == id) { return Err(DeviceError::NotSaved); }
        let mut value = self.clone(); value.devices.retain(|device| device.id != id); Ok(value)
    }
}
