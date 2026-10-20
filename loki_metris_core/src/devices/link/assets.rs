//! 内容寻址图片与严格顺序的增量接收；文件原子提交由平台私有存储适配器负责。

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use super::LinkError;
use crate::ImageFormat;

/// 与本机图库相同的单图上限。
pub const MAX_ASSET_BYTES: usize = 32 * 1024 * 1024;
/// Base64后仍远小于64KiB加密明文上限。
pub const ASSET_CHUNK_BYTES: usize = 16 * 1024;

/// 内容与MIME共同决定缓存合法性；文件名只用摘要，绝不接受远端路径。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetDescriptor { /// 小写SHA-256摘要。
    pub hash: String, /// 魔数确定的MIME。
    pub mime: String, /// 原始正文长度。
    pub length: u64 }
impl AssetDescriptor {
    /// 只为允许的非空有界图片生成摘要。
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, LinkError> {
        if bytes.is_empty() || bytes.len() > MAX_ASSET_BYTES { return Err(LinkError::Limit); }
        let format = ImageFormat::from_bytes(bytes).ok_or(LinkError::Invalid)?;
        Ok(Self { hash: digest(bytes), mime: format.mime_type().into(), length: bytes.len() as u64 })
    }
    /// 任何持久缓存复用前均验证完整摘要、魔数及长度。
    pub fn matches(&self, bytes: &[u8]) -> bool { Self::from_bytes(bytes).as_ref() == Ok(self) }
    /// 缓存索引及首个分块开始前先验证长度和固定摘要字符集。
    pub fn validate(&self) -> Result<(), LinkError> {
        if self.length == 0 || self.length > MAX_ASSET_BYTES as u64 || self.hash.len() != 64
            || !self.hash.bytes().all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
            || !matches!(self.mime.as_str(), "image/png" | "image/jpeg" | "image/gif") { return Err(LinkError::Invalid); }
        Ok(())
    }
    /// 请求只允许固定块边界，最后一块按实际长度收尾。
    pub fn chunk(&self, bytes: &[u8], offset: u64) -> Result<AssetChunk, LinkError> {
        self.validate()?;
        if bytes.len() as u64 != self.length || offset >= self.length || offset % ASSET_CHUNK_BYTES as u64 != 0 { return Err(LinkError::Invalid); }
        let start = offset as usize; let end = (start + ASSET_CHUNK_BYTES).min(bytes.len());
        Ok(AssetChunk { hash: self.hash.clone(), offset, data: STANDARD.encode(&bytes[start..end]) })
    }
}
/// 块从认证通道传输，但仍须验证顺序与最终完整内容摘要。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetChunk { /// 当前清单摘要。
    pub hash: String, /// 原始字节偏移。
    pub offset: u64, /// 标准带填充Base64。
    pub data: String }

/// 未校验正文只存一份有界临时缓冲；失败即清空且不可继续。
pub struct AssetReceiver { descriptor: AssetDescriptor, bytes: Vec<u8>, failed: bool }
impl AssetReceiver {
    /// 延迟分配实际正文，不按远端声明一次预分配32MiB。
    pub fn new(descriptor: AssetDescriptor) -> Result<Self, LinkError> {
        descriptor.validate()?; Ok(Self { descriptor, bytes: Vec::new(), failed: false })
    }
    /// 下一次只请求此偏移，缓存命中则根本不建立接收器。
    pub fn offset(&self) -> u64 { self.bytes.len() as u64 }
    /// 拒绝乱序、重放、非规范编码、短中间块和超量数据；失败后不能恢复。
    pub fn accept(&mut self, chunk: &AssetChunk) -> Result<bool, LinkError> {
        let result = self.accept_inner(chunk);
        if result.is_err() { self.failed = true; self.bytes.clear(); }
        result
    }
    /// 解码前检查编码上限，再按剩余长度检查严格块长。
    fn accept_inner(&mut self, chunk: &AssetChunk) -> Result<bool, LinkError> {
        if self.failed || chunk.hash != self.descriptor.hash || chunk.offset != self.offset()
            || chunk.data.len() > ASSET_CHUNK_BYTES.div_ceil(3) * 4 { return Err(LinkError::Invalid); }
        let decoded = STANDARD.decode(&chunk.data).map_err(|_| LinkError::Invalid)?;
        let remaining = self.descriptor.length as usize - self.bytes.len();
        if decoded.len() != remaining.min(ASSET_CHUNK_BYTES) || decoded.is_empty() || STANDARD.encode(&decoded) != chunk.data { return Err(LinkError::Invalid); }
        self.bytes.extend_from_slice(&decoded);
        let complete = self.bytes.len() as u64 == self.descriptor.length;
        if complete && !self.descriptor.matches(&self.bytes) { return Err(LinkError::Authentication); }
        Ok(complete)
    }
    /// 只有完整校验通过的正文才能交宿主原子保存；失败不泄漏部分正文为缓存命中。
    pub fn finish(self) -> Result<(AssetDescriptor, Vec<u8>), LinkError> {
        if self.failed || !self.descriptor.matches(&self.bytes) { return Err(LinkError::Invalid); }
        Ok((self.descriptor, self.bytes))
    }
}
/// 摘要固定小写十六进制，跨端无需平台哈希差异。
fn digest(bytes: &[u8]) -> String { Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect() }

#[cfg(test)]
mod tests {
    use super::*;
    /// 多块图片校验后可复用；篡改及乱序不能提交部分资源。
    #[test]
    fn chunks_require_exact_order_and_full_digest_before_commit() {
        let mut bytes = b"GIF89a".to_vec(); bytes.resize(ASSET_CHUNK_BYTES * 2 + 3, 7);
        let descriptor = AssetDescriptor::from_bytes(&bytes).unwrap();
        let mut receiver = AssetReceiver::new(descriptor.clone()).unwrap();
        while receiver.offset() < bytes.len() as u64 { let chunk = descriptor.chunk(&bytes, receiver.offset()).unwrap(); receiver.accept(&chunk).unwrap(); }
        assert_eq!(receiver.finish().unwrap().1, bytes);
        assert!(!descriptor.matches(b"GIF89abad"));
        let mut receiver = AssetReceiver::new(descriptor.clone()).unwrap();
        assert!(receiver.accept(&descriptor.chunk(&bytes, ASSET_CHUNK_BYTES as u64).unwrap()).is_err());
        assert!(receiver.accept(&descriptor.chunk(&bytes, 0).unwrap()).is_err()); assert!(receiver.finish().is_err());
        let mut bad = bytes.clone(); *bad.last_mut().unwrap() ^= 1;
        let mut receiver = AssetReceiver::new(descriptor.clone()).unwrap();
        let mut failed = false;
        while receiver.offset() < bad.len() as u64 { let chunk = descriptor.chunk(&bad, receiver.offset()).unwrap(); if receiver.accept(&chunk).is_err() { failed = true; break; } }
        assert!(failed); assert!(receiver.finish().is_err());
    }
}
