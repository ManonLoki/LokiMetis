//! 严格有序的双向AEAD通道；收到无效帧后永久关闭，不能跳过错误继续解包。

use super::{EncryptedPacket, MAX_CHANNEL_PACKETS, MAX_PLAINTEXT_BYTES, SessionError};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chacha20poly1305::{ChaCha20Poly1305, Nonce, aead::{Aead, KeyInit, Payload}};

/// 密钥与序号具有唯一所有权；不实现Clone、Debug或Serialize，避免nonce状态复制。
pub struct SecureChannel {
    send_key: [u8; 32], receive_key: [u8; 32], transcript: [u8; 32],
    sent: u64, received: u64, closed: bool,
}

impl SecureChannel {
    /// 仅通过已认证握手构造通道；每方向使用独立派生密钥。
    pub(super) fn new(send_key: [u8; 32], receive_key: [u8; 32], transcript: [u8; 32]) -> Self {
        Self { send_key, receive_key, transcript, sent: 0, received: 0, closed: false }
    }

    /// 仅披露通道是否已永久关闭，不允许调用方重置密钥、序号或关闭状态。
    pub fn is_open(&self) -> bool { !self.closed }

    /// 加密一条有界应用消息；空包和超限包不消耗nonce，耗尽或加密失败关闭整个会话。
    pub fn seal(&mut self, plaintext: &[u8]) -> Result<EncryptedPacket, SessionError> {
        if self.closed { return Err(SessionError::Closed); }
        if plaintext.is_empty() || plaintext.len() > MAX_PLAINTEXT_BYTES { return Err(SessionError::Invalid); }
        if self.sent >= MAX_CHANNEL_PACKETS { self.closed = true; return Err(SessionError::Closed); }
        let sequence = self.sent;
        let cipher = ChaCha20Poly1305::new((&self.send_key).into());
        let result = cipher.encrypt(&nonce(sequence), Payload { msg: plaintext, aad: &self.aad(sequence) });
        match result {
            Ok(bytes) => { self.sent += 1; Ok(EncryptedPacket { sequence, ciphertext: STANDARD.encode(bytes) }) },
            Err(_) => { self.closed = true; Err(SessionError::VerificationFailed) },
        }
    }

    /// 先验证有界规范编码、期望序号与认证再推进；任何接收失败永久撤销两个方向。
    pub fn open(&mut self, packet: &EncryptedPacket) -> Result<Vec<u8>, SessionError> {
        if self.closed { return Err(SessionError::Closed); }
        let result = self.decrypt(packet);
        if result.is_ok() { self.received += 1; } else { self.closed = true; }
        result
    }

    /// 解密不改变状态；传输层仍须在JSON反序列化前限制整帧长度。
    fn decrypt(&self, packet: &EncryptedPacket) -> Result<Vec<u8>, SessionError> {
        let maximum = (MAX_PLAINTEXT_BYTES + 16).div_ceil(3) * 4;
        if self.received >= MAX_CHANNEL_PACKETS || packet.sequence != self.received
            || packet.ciphertext.len() > maximum { return Err(SessionError::VerificationFailed); }
        let bytes = STANDARD.decode(&packet.ciphertext).map_err(|_| SessionError::VerificationFailed)?;
        if bytes.len() <= 16 || bytes.len() > MAX_PLAINTEXT_BYTES + 16 || STANDARD.encode(&bytes) != packet.ciphertext {
            return Err(SessionError::VerificationFailed);
        }
        let cipher = ChaCha20Poly1305::new_from_slice(&self.receive_key).map_err(|_| SessionError::VerificationFailed)?;
        cipher.decrypt(&nonce(packet.sequence), Payload { msg: &bytes, aad: &self.aad(packet.sequence) }).map_err(|_| SessionError::VerificationFailed)
    }

    /// 会话摘要和精确序号共同认证，方向隔离由不同的派生密钥保证。
    fn aad(&self, sequence: u64) -> [u8; 40] {
        let mut aad = [0; 40]; aad[..32].copy_from_slice(&self.transcript); aad[32..].copy_from_slice(&sequence.to_be_bytes()); aad
    }
}

/// 方向独立密钥下从0递增的96位nonce；只在重新随机握手后才允许归零。
fn nonce(sequence: u64) -> Nonce {
    let mut bytes = [0; 12]; bytes[4..].copy_from_slice(&sequence.to_be_bytes()); bytes.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 最后一枚nonce可使用一次，随后永久关闭而不是回绕到0。
    #[test]
    fn sequence_exhaustion_never_reuses_a_nonce() {
        let mut sender = SecureChannel::new([1; 32], [2; 32], [3; 32]);
        let mut receiver = SecureChannel::new([2; 32], [1; 32], [3; 32]);
        sender.sent = MAX_CHANNEL_PACKETS - 1; receiver.received = MAX_CHANNEL_PACKETS - 1;
        let last = sender.seal(b"last").unwrap();
        assert_eq!(last.sequence, MAX_CHANNEL_PACKETS - 1);
        assert_eq!(receiver.open(&last).unwrap(), b"last");
        assert!(matches!(sender.seal(b"wrap"), Err(SessionError::Closed)));
        assert_eq!(sender.open(&last), Err(SessionError::Closed));
        assert_eq!(receiver.open(&last), Err(SessionError::VerificationFailed));
        assert!(matches!(receiver.seal(b"late"), Err(SessionError::Closed)));
    }
}
