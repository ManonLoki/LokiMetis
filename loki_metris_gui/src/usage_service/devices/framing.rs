//! 有界且可取消后续读的长度前缀JSON；取消select分支不会丢失半帧进度。

use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// 读进度由连接owner持有，避免心跳/发送分支取消read_exact后破坏流边界。
pub(super) struct FrameReader {
    header: [u8; 4], header_read: usize, payload: Vec<u8>, payload_read: usize, limit: usize,
}

impl FrameReader {
    /// 每个连接设置固定上限；握手与已认证密文分别采用各自协议限制。
    pub(super) fn new(limit: usize) -> Self {
        Self { header: [0; 4], header_read: 0, payload: Vec::new(), payload_read: 0, limit }
    }

    /// 读取长度前先限界再分配；每次已读字节立即提交进度，可安全被select取消。
    pub(super) async fn read<T: DeserializeOwned>(&mut self, reader: &mut (impl AsyncRead + Unpin)) -> Result<T, &'static str> {
        while self.header_read < 4 {
            let count = reader.read(&mut self.header[self.header_read..]).await.map_err(|_| "device_connection_failed")?;
            if count == 0 { return Err("device_connection_failed"); }
            self.header_read += count;
        }
        if self.payload.is_empty() {
            let length = u32::from_be_bytes(self.header) as usize;
            if length == 0 || length > self.limit { return Err("device_protocol_invalid"); }
            self.payload.resize(length, 0);
        }
        while self.payload_read < self.payload.len() {
            let count = reader.read(&mut self.payload[self.payload_read..]).await.map_err(|_| "device_connection_failed")?;
            if count == 0 { return Err("device_connection_failed"); }
            self.payload_read += count;
        }
        let result = serde_json::from_slice(&self.payload).map_err(|_| "device_protocol_invalid");
        self.header_read = 0; self.payload_read = 0; self.payload.clear(); result
    }
}

/// 有界JSON与网络序长度一起写入；写失败或取消后连接owner必须关闭socket。
pub(super) async fn write_frame(writer: &mut (impl AsyncWrite + Unpin), value: &impl Serialize, limit: usize) -> Result<(), &'static str> {
    let bytes = serde_json::to_vec(value).map_err(|_| "device_protocol_invalid")?;
    if bytes.is_empty() || bytes.len() > limit { return Err("device_protocol_invalid"); }
    writer.write_u32(bytes.len() as u32).await.map_err(|_| "device_connection_failed")?;
    writer.write_all(&bytes).await.map_err(|_| "device_connection_failed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{future::{Future, poll_fn}, task::Poll};

    /// 半个头或半个载荷的read被取消后，同一framer继续读仍得到完整且唯一的一帧。
    #[tokio::test]
    async fn cancelled_read_preserves_partial_header_and_payload() {
        let payload = br#"{"value":42}"#;
        let mut frame = (payload.len() as u32).to_be_bytes().to_vec(); frame.extend_from_slice(payload);
        for cut in [2, 7] {
            let (mut writer, mut reader) = tokio::io::duplex(128); let mut framer = FrameReader::new(128);
            writer.write_all(&frame[..cut]).await.unwrap();
            let mut reading = Box::pin(framer.read::<serde_json::Value>(&mut reader));
            poll_fn(|cx| { assert!(reading.as_mut().poll(cx).is_pending()); Poll::Ready(()) }).await;
            drop(reading);
            writer.write_all(&frame[cut..]).await.unwrap();
            let value: serde_json::Value = framer.read(&mut reader).await.unwrap();
            assert_eq!(value["value"], 42);
            write_frame(&mut writer, &serde_json::json!({"next":true}), 128).await.unwrap();
            let next: serde_json::Value = framer.read(&mut reader).await.unwrap(); assert_eq!(next["next"], true);
        }
    }

    /// 超限长度在收到头部时立即拒绝，不分配攻击者指定的载荷缓冲区。
    #[tokio::test]
    async fn length_limit_is_checked_before_payload_allocation() {
        for length in [0, 4097, u32::MAX] {
            let (mut writer, mut reader) = tokio::io::duplex(16); let mut framer = FrameReader::new(4096);
            writer.write_u32(length).await.unwrap();
            assert_eq!(framer.read::<serde_json::Value>(&mut reader).await, Err("device_protocol_invalid"));
            assert_eq!(framer.payload.capacity(), 0);
        }
    }
}
