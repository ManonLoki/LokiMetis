//! 桌面配对恢复包只加密有界私有材料；恢复码独立保存，不从公开设备身份派生。

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{ChaCha20Poly1305, aead::{Aead, KeyInit, Payload}};
use crate::PairingError;

/// 格式与用途一并认证，移动端材料或未来版本不能被误当作桌面账本。
const HEADER: &[u8] = b"LokiMetris/desktop-pairing/1\n";
/// 原始账本至多64KiB；包只增加固定头、nonce与认证标签。
pub const MAX_PACKAGE_BYTES: usize = 64 * 1024 + HEADER.len() + 12 + 16;

/// 将宿主系统随机源生成的32字节编码为便于单独保存的强恢复码。
pub fn encode_code(key: &[u8; 32]) -> Result<String, PairingError> {
    if *key == [0; 32] { return Err(PairingError::Invalid); }
    Ok(format!("LMR1-{}", URL_SAFE_NO_PAD.encode(key)))
}

/// 只接受规范43字符Base64url密钥，拒绝口令、零密钥和多余字段。
pub fn decode_code(code: &str) -> Result<[u8; 32], PairingError> {
    if code.len() > 64 { return Err(PairingError::Invalid); }
    let code = code.trim();
    let encoded = code.strip_prefix("LMR1-").ok_or(PairingError::Invalid)?;
    if encoded.len() != 43 { return Err(PairingError::Invalid); }
    let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| PairingError::Invalid)?;
    let key: [u8; 32] = bytes.try_into().map_err(|_| PairingError::Invalid)?;
    if encode_code(&key)? != code { return Err(PairingError::Invalid); }
    Ok(key)
}

/// 宿主必须为每次导出生成全新随机密钥及nonce；明文不进入输出或错误。
pub fn seal(bytes: &[u8], key: &[u8; 32], nonce: [u8; 12]) -> Result<Vec<u8>, PairingError> {
    if bytes.is_empty() || bytes.len() > 64 * 1024 || *key == [0; 32] { return Err(PairingError::Invalid); }
    let cipher = ChaCha20Poly1305::new_from_slice(key).map_err(|_| PairingError::Invalid)?;
    let encrypted = cipher.encrypt((&nonce).into(), Payload { msg: bytes, aad: HEADER }).map_err(|_| PairingError::VerificationFailed)?;
    let mut result = Vec::with_capacity(HEADER.len() + 12 + encrypted.len());
    result.extend_from_slice(HEADER); result.extend_from_slice(&nonce); result.extend(encrypted);
    Ok(result)
}

/// 先限制长度和版本，再验证整包AEAD；错误恢复码或任何篡改均不返回材料。
pub fn open(package: &[u8], code: &str) -> Result<Vec<u8>, PairingError> {
    if package.len() > MAX_PACKAGE_BYTES || package.len() <= HEADER.len() + 12 + 16 || !package.starts_with(HEADER) {
        return Err(PairingError::Invalid);
    }
    let key = decode_code(code)?;
    let nonce: &[u8; 12] = package[HEADER.len()..HEADER.len() + 12].try_into().map_err(|_| PairingError::Invalid)?;
    let cipher = ChaCha20Poly1305::new_from_slice(&key).map_err(|_| PairingError::Invalid)?;
    cipher.decrypt(nonce.into(), Payload { msg: &package[HEADER.len() + 12..], aad: HEADER }).map_err(|_| PairingError::VerificationFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 正确码恢复原字节；错误码、头部/nonce/密文篡改和越界均失败。
    #[test]
    fn package_authenticates_entire_content_and_recovery_code() {
        let key = [0xa7; 32]; let code = encode_code(&key).unwrap();
        let package = seal(b"private-material", &key, [9; 12]).unwrap();
        assert_eq!(open(&package, &code).unwrap(), b"private-material");
        assert!(open(&package, &encode_code(&[3; 32]).unwrap()).is_err());
        for index in [0, HEADER.len(), package.len() - 1] {
            let mut changed = package.clone(); changed[index] ^= 1;
            assert!(open(&changed, &code).is_err());
        }
        assert!(open(&vec![0; MAX_PACKAGE_BYTES + 1], &code).is_err());
        assert!(decode_code("123456").is_err()); assert!(encode_code(&[0; 32]).is_err());
    }
}
