//! 未变文件用可靠宿主戳快速返回；变更文件核验旧完整前缀，正文不会持久化。

use super::{CancellationToken, LocalError, LocalErrorKind};
use crate::file_stamp::FileStamp;
use std::{fs::File, io::{Read, Seek, SeekFrom}};

/// 摘要格式独立版本化，旧64字节身份首次重建一次，无需改变数据库 schema。
#[derive(serde::Serialize, serde::Deserialize)]
struct Fingerprint {
    /// 摘要格式版本。
    version: u8,
    /// 摘要覆盖的字节数。
    length: u64,
    /// 摘要时的文件戳。
    stamp: FileStamp,
    /// 内容摘要。
    digest: u64,
}

/// 核验同一打开文件的旧长度前缀；增长不再自动意味着仅在尾部追加。
pub(super) fn probe(file: &mut File, observed_size: u64, previous: Option<(&str, u64)>, cancellation: &CancellationToken)
    -> Result<(String, bool), LocalError> {
    let metadata = file.metadata()?;
    if metadata.len() != observed_size { return Err(changed()); }
    let stamp = FileStamp::of(file, &metadata);
    // 摘要至少覆盖已消费段；取消时可保留旧可信前缀，下一轮先核验它再消费新字节。
    let old = previous.and_then(|(encoded, length)| serde_json::from_str::<Fingerprint>(encoded).ok()
        .filter(|old| old.version == 2 && old.length >= length));
    if let Some(old) = &old && stamp.is_cacheable() && old.stamp == stamp && old.length == observed_size {
        return Ok((previous.unwrap().0.to_owned(), true));
    }
    file.seek(SeekFrom::Start(0))?;
    let mut hash = 0xcbf29ce484222325_u64;
    let mut prefix = (old.as_ref().is_some_and(|old| old.length == 0)).then_some(hash);
    let mut offset = 0;
    let mut buffer = [0_u8; 64 * 1024];
    while offset < observed_size {
        if cancellation.is_cancelled() {
            return match old {
                Some(_) => Ok((previous.unwrap().0.to_owned(), true)),
                None => Ok((serde_json::to_string(&Fingerprint { version: 2, length: 0, stamp,
                    digest: 0xcbf29ce484222325 }).map_err(|_| changed())?, false)),
            };
        }
        let boundary = old.as_ref().filter(|old| offset < old.length).map_or(observed_size, |old| old.length.min(observed_size));
        let count = (boundary - offset).min(buffer.len() as u64) as usize;
        file.read_exact(&mut buffer[..count])?;
        for byte in &buffer[..count] { hash ^= u64::from(*byte); hash = hash.wrapping_mul(0x100000001b3); }
        offset += count as u64;
        if old.as_ref().is_some_and(|old| old.length == offset) { prefix = Some(hash); }
    }
    let after = FileStamp::of(file, &file.metadata()?);
    if stamp != after { return Err(changed()); }
    let appendable = old.is_some_and(|old| old.length <= observed_size && prefix == Some(old.digest));
    let fingerprint = Fingerprint { version: 2, length: observed_size, stamp, digest: hash };
    let encoded = serde_json::to_string(&fingerprint).map_err(|_| changed())?;
    Ok((encoded, appendable))
}

/// 解析岛交出批次之前复核同一文件，来源变化不能提交混合内容或破坏旧 generation。
pub(super) fn verify(file: &mut File, encoded: &str) -> Result<(), LocalError> {
    let old: Fingerprint = serde_json::from_str(encoded).map_err(|_| changed())?;
    let metadata = file.metadata()?;
    let stamp = FileStamp::of(file, &metadata);
    if stamp.is_cacheable() {
        if stamp != old.stamp { return Err(changed()); }
    } else {
        // 解析可取消，但已消费批次必须完成一致性核验后才能提交。
        let (current, _) = probe(file, old.length, None, &CancellationToken::new())?;
        let current: Fingerprint = serde_json::from_str(&current).map_err(|_| changed())?;
        if current.digest != old.digest { return Err(changed()); }
    }
    Ok(())
}

/// 来源并发变化使用稳定脱敏错误，调用方保留旧 checkpoint 并在下轮重试。
fn changed() -> LocalError { LocalError::new(LocalErrorKind::SourceUnavailable, "source changed during fingerprint or parse") }

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 块边界附近旧完整前缀必须逐字节匹配，同长度替换及更长改写都不能作为追加。
    #[test]
    fn verifies_prefix_at_buffer_boundaries_and_rejects_rewrites() {
        for length in [0, 1, 65_535, 65_536, 65_537] {
            let mut file = tempfile::tempfile().unwrap();
            file.write_all(&vec![b'a'; length]).unwrap();
            let (first, _) = probe(&mut file, length as u64, None, &CancellationToken::new()).unwrap();
            file.seek(SeekFrom::End(0)).unwrap();
            file.write_all(b"suffix").unwrap();
            let (_, appendable) = probe(&mut file, length as u64 + 6, Some((&first, length as u64)), &CancellationToken::new()).unwrap();
            assert!(appendable);
            if length > 0 {
                file.seek(SeekFrom::Start((length - 1) as u64)).unwrap();
                file.write_all(b"b").unwrap();
                let (_, appendable) = probe(&mut file, length as u64 + 6, Some((&first, length as u64)), &CancellationToken::new()).unwrap();
                assert!(!appendable);
            }
        }
    }

    /// 预取消保留原可信戳而不认证新字节；恢复时仍检测旧前缀已改写。
    #[test]
    fn cancellation_preserves_only_the_previous_verified_prefix() {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"original").unwrap();
        let (first, _) = probe(&mut file, 8, None, &CancellationToken::new()).unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(b"replaced-extra").unwrap();
        let cancellation = CancellationToken::new(); cancellation.cancel();
        let (paused, matches) = probe(&mut file, 14, Some((&first, 8)), &cancellation).unwrap();
        assert_eq!(paused, first);
        assert!(matches);
        let (_, matches) = probe(&mut file, 14, Some((&paused, 8)), &CancellationToken::new()).unwrap();
        assert!(!matches);
        let (empty, _) = probe(&mut file, 14, None, &cancellation).unwrap();
        assert_eq!(serde_json::from_str::<Fingerprint>(&empty).unwrap().length, 0);
    }

    /// 摘要后发生原地改写必须拒绝提交；旧格式身份升级只允许重建。
    #[test]
    fn rejects_changes_after_probe_and_unverified_legacy_identities() {
        let mut file = tempfile::tempfile().unwrap(); file.write_all(b"old-body").unwrap();
        let (first, appendable) = probe(&mut file, 8, Some(("legacy-64-byte-hash", 8)), &CancellationToken::new()).unwrap();
        assert!(!appendable);
        file.seek(SeekFrom::Start(0)).unwrap(); file.write_all(b"new-body").unwrap();
        assert!(verify(&mut file, &first).is_err());
        file.set_len(2).unwrap();
        let (_, appendable) = probe(&mut file, 2, Some((&first, 8)), &CancellationToken::new()).unwrap();
        assert!(!appendable);
    }
}
