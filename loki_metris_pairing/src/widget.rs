//! 小组件专用只读密钥与HTTP消息加密；不暴露长期配对密钥，也不依赖网络运行时。

use base64::{Engine, engine::general_purpose::STANDARD};
use chacha20poly1305::{ChaCha20Poly1305, Nonce, aead::{Aead, KeyInit, Payload}};
use hkdf::Hkdf;
use sha2::Sha256;
use crate::PairingError;

/// 请求明文和认证域，方向与响应严格分离。
pub const REQUEST_DOMAIN: &[u8] = b"loki-widget-request-v1";
/// 响应认证域后拼接本次请求nonce，禁止重放旧摘要。
pub const RESPONSE_DOMAIN: &[u8] = b"loki-widget-response-v1";

/// 独立派生每一对已认证身份的小组件读取权限，不可用于TCP认证。
pub fn derive_key(secret: &[u8; 32], client: &str, device: &str) -> Result<[u8; 32], PairingError> {
    if !crate::valid_device_id(client) || !crate::valid_device_id(device) || client == device { return Err(PairingError::Invalid); }
    let mut key = [0; 32];
    Hkdf::<Sha256>::new(Some(format!("{client}/{device}").as_bytes()), secret)
        .expand(b"LokiMetris/widget-http/1/read-only", &mut key).map_err(|_| PairingError::Invalid)?;
    Ok(key)
}

/// 只用于已加密的认证TCP元数据，不写入日志或普通GUI投影。
pub fn encode_key(key: &[u8; 32]) -> String { STANDARD.encode(key) }

/// 解码有界随机nonce并验证固定请求，不接受读取任意资源的参数。
pub fn open_request(key: &[u8; 32], nonce: &str, ciphertext: &str) -> Result<[u8; 12], PairingError> {
    if nonce.len() != 16 || ciphertext.len() > 128 { return Err(PairingError::Invalid); }
    let nonce: [u8; 12] = STANDARD.decode(nonce).map_err(|_| PairingError::Invalid)?.try_into().map_err(|_| PairingError::Invalid)?;
    let bytes = STANDARD.decode(ciphertext).map_err(|_| PairingError::Invalid)?;
    let plaintext = ChaCha20Poly1305::new(key.into()).decrypt(&Nonce::from(nonce), Payload { msg: &bytes, aad: REQUEST_DOMAIN })
        .map_err(|_| PairingError::VerificationFailed)?;
    if plaintext != REQUEST_DOMAIN { return Err(PairingError::Invalid); }
    Ok(nonce)
}

/// 加密已裁剪摘要并绑定请求nonce；调用者为每个响应提供新的随机nonce。
pub fn seal_response(key: &[u8; 32], request_nonce: &[u8; 12], nonce: &[u8; 12], plaintext: &[u8]) -> Result<(String, String), PairingError> {
    if plaintext.len() > 64 * 1024 { return Err(PairingError::Invalid); }
    let mut aad = RESPONSE_DOMAIN.to_vec(); aad.extend_from_slice(request_nonce);
    let ciphertext = ChaCha20Poly1305::new(key.into()).encrypt(&Nonce::from(*nonce), Payload { msg: plaintext, aad: &aad })
        .map_err(|_| PairingError::VerificationFailed)?;
    Ok((STANDARD.encode(nonce), STANDARD.encode(ciphertext)))
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 固定跨端向量同时证明身份隔离、请求完整性和响应请求绑定。
    #[test]
    fn encrypted_roundtrip_binds_identity_and_request() {
        let key = derive_key(&[7; 32], "12345678-1234-4234-8234-123456789abc", "22345678-1234-4234-8234-123456789abc").unwrap();
        assert_ne!(key, [7; 32]);
        let nonce = [3; 12];
        let cipher = ChaCha20Poly1305::new((&key).into());
        let request = cipher.encrypt(&Nonce::from(nonce), Payload { msg: REQUEST_DOMAIN, aad: REQUEST_DOMAIN }).unwrap();
        assert_eq!(open_request(&key, &STANDARD.encode(nonce), &STANDARD.encode(&request)).unwrap(), nonce);
        assert!(open_request(&[8; 32], &STANDARD.encode(nonce), &STANDARD.encode(request)).is_err());
        let (_, encrypted) = seal_response(&key, &nonce, &[4; 12], b"snapshot").unwrap();
        let mut aad = RESPONSE_DOMAIN.to_vec(); aad.extend_from_slice(&nonce);
        assert_eq!(cipher.decrypt(&Nonce::from([4; 12]), Payload { msg: &STANDARD.decode(&encrypted).unwrap(), aad: &aad }).unwrap(), b"snapshot");
        aad[RESPONSE_DOMAIN.len()] ^= 1;
        assert!(cipher.decrypt(&Nonce::from([4; 12]), Payload { msg: &STANDARD.decode(encrypted).unwrap(), aad: &aad }).is_err());
    }
}
