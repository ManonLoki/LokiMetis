//! 已保存设备的小组件读取权限；解除配对后不能创建新的读取会话。

use super::{DeviceBook, DeviceError};
use serde::{Serialize, Deserialize};

/// 只经认证TCP下发的小组件服务能力，不包含长期配对凭据。
#[derive(Clone, Serialize, Deserialize)]
pub struct WidgetEndpoint {
    /// 当前固定协议版本。
    pub version: u8,
    /// 与当前认证服务同一局域网监听端口。
    pub port: u16,
    /// 专用只读密钥，标准带padding Base64。
    pub key: String,
}

/// 已核验请求的响应加密权限；不实现Debug或序列化。
pub struct WidgetRequest {
    key: [u8; 32],
    nonce: [u8; 12],
}
impl WidgetRequest {
    /// 响应只能加密不超过64KiB的摘要，并绑定原请求。
    pub fn seal(&self, nonce: &[u8; 12], bytes: &[u8]) -> Result<(String, String), DeviceError> {
        loki_metris_pairing::widget::seal_response(&self.key, &self.nonce, nonce, bytes).map_err(|_| DeviceError::Invalid)
    }
}
impl DeviceBook {
    /// 权限必须从当前账本取得，旧密钥不能绕过解除配对。
    fn widget_key(&self, device: &str) -> Result<[u8; 32], DeviceError> {
        let saved = self.devices.iter().find(|saved| saved.id == device).ok_or(DeviceError::NotSaved)?;
        loki_metris_pairing::widget::derive_key(&saved.key.0, &self.client_id, device).map_err(|_| DeviceError::Invalid)
    }
    /// 只给已保存的精确设备发放当前端口的读取能力。
    pub fn widget_endpoint(&self, device: &str, port: u16) -> Result<WidgetEndpoint, DeviceError> {
        if port == 0 { return Err(DeviceError::Invalid); }
        Ok(WidgetEndpoint { version: 1, port, key: loki_metris_pairing::widget::encode_key(&self.widget_key(device)?) })
    }
    /// 先绑定桌面和设备身份，再验证固定读取请求的AEAD完整性。
    pub fn widget_request(&self, client: &str, device: &str, nonce: &str, ciphertext: &str) -> Result<WidgetRequest, DeviceError> {
        if client != self.client_id { return Err(DeviceError::Invalid); }
        let key = self.widget_key(device)?;
        let nonce = loki_metris_pairing::widget::open_request(&key, nonce, ciphertext).map_err(|_| DeviceError::Invalid)?;
        Ok(WidgetRequest { key, nonce })
    }
}

#[cfg(test)]
mod tests {
    /// CryptoKit固定向量跨语言验证；解除配对或错误身份均不能继续读取。
    #[test]
    fn widget_permission_matches_swift_vector_and_is_revoked_with_pairing() {
        let (book, _, _) = crate::devices::tests::link_fixture();
        let device = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
        let endpoint = book.widget_endpoint(device, 1234).unwrap();
        assert_eq!(endpoint.key, "lRknhcPotblkxFclfQ/VvPvqBMbhA3J+2WPJ+fIB+eo=");
        let nonce = "AwMDAwMDAwMDAwMD";
        let ciphertext = "Xb0+fryh6yKmj9ynUJN1ibves2wo6z40FJxtUvoAbZtNdXluBiI=";
        assert!(book.widget_request(book.client_id(), device, nonce, ciphertext).is_ok());
        assert!(book.widget_request(device, device, nonce, ciphertext).is_err());
        let revoked = book.without(device).unwrap();
        assert!(revoked.widget_endpoint(device, 1234).is_err());
        assert!(revoked.widget_request(book.client_id(), device, nonce, ciphertext).is_err());
    }
}
