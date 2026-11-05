//! Windows Cursor 的 Local State、DPAPI 内存所有权与 AES-GCM 解密驱动。

use loki_metris_core::QuotaFailure;
#[cfg(target_os = "windows")]
use super::{credentials::Secret, transport::CurlLimits};
use super::credentials::HomeLayout;

/// Cursor 与 state.vscdb 共用 Roaming 用户数据根；不查询其它浏览器目录。
fn local_state_path(layout: &HomeLayout) -> std::path::PathBuf {
    layout.app_data.clone().unwrap_or_else(|| layout.home.join("AppData").join("Roaming"))
        .join("Cursor").join("Local State")
}

/// Chromium 的 Local State 使用 snake_case 键，包装必须带 DPAPI 标识。
fn wrapped_key(state: &serde_json::Value) -> Result<Vec<u8>, QuotaFailure> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let encoded = state["os_crypt"]["encrypted_key"].as_str().ok_or(QuotaFailure::Malformed)?;
    let wrapped = STANDARD.decode(encoded).map_err(|_| QuotaFailure::Malformed)?;
    wrapped.strip_prefix(b"DPAPI").filter(|bytes| !bytes.is_empty())
        .map(<[u8]>::to_vec).ok_or(QuotaFailure::Malformed)
}

/// 在已拥有的后台读取任务中执行不弹窗的本机 DPAPI，不创建可被遗弃的阻塞任务。
/// OS 调用不能强制中断；在调用前后检查取消及预算，超时结果不会进入网络。
#[cfg(target_os = "windows")]
pub(super) async fn decode(
    token: Secret,
    layout: &HomeLayout,
    limits: CurlLimits,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<Secret, QuotaFailure> {
    let started = std::time::Instant::now();
    if *cancel.borrow() || cancel.has_changed().is_err() {
        return Err(QuotaFailure::CredentialLocked);
    }
    let cipher = super::cursor_safe_storage::unwrap_cipher(&token, b"v10")?;
    if cipher.len() < 28 {
        return Err(QuotaFailure::Malformed);
    }
    let state = read_local_state(&local_state_path(layout))?;
    let mut key = dpapi_unprotect(&wrapped_key(&state)?)?;
    let result = decrypt_windows(&cipher, &key);
    key.fill(0);
    let mut plain = result?;
    let result = if *cancel.borrow() || cancel.has_changed().is_err() || started.elapsed() >= limits.timeout {
        Err(QuotaFailure::CredentialLocked)
    } else {
        std::str::from_utf8(&plain).ok().and_then(Secret::new).ok_or(QuotaFailure::Malformed)
    };
    plain.fill(0);
    result
}

/// 只读取常规且有界的 Local State；读取期间增长同样不能突破上限。
#[cfg(target_os = "windows")]
fn read_local_state(path: &std::path::Path) -> Result<serde_json::Value, QuotaFailure> {
    use std::io::Read;
    const MAX_BYTES: u64 = 256 * 1024;
    let metadata = std::fs::symlink_metadata(path).map_err(|_| QuotaFailure::CredentialLocked)?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err(QuotaFailure::CredentialLocked);
    }
    let file = std::fs::File::open(path).map_err(|_| QuotaFailure::CredentialLocked)?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes).map_err(|_| QuotaFailure::CredentialLocked)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(QuotaFailure::CredentialLocked);
    }
    serde_json::from_slice(&bytes).map_err(|_| QuotaFailure::Malformed)
}

/// 独占 Windows 分配的输出，所有返回路径先清零再释放，借用不逃逸。
#[cfg(target_os = "windows")]
struct DpapiOutput(windows::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB);

#[cfg(target_os = "windows")]
impl Drop for DpapiOutput {
    /// DPAPI 返回的有效缓冲仍由本守卫拥有，清零完成后才调用 LocalFree。
    fn drop(&mut self) {
        if self.0.pbData.is_null() { return; }
        // SAFETY: 成功的 DPAPI 输出按 cbData 分配，守卫是其唯一所有者且只释放一次。
        unsafe {
            for index in 0..self.0.cbData as usize {
                std::ptr::write_volatile(self.0.pbData.add(index), 0);
            }
            let _ = windows::Win32::Foundation::LocalFree(Some(
                windows::Win32::Foundation::HLOCAL(self.0.pbData.cast()),
            ));
        }
    }
}

/// 解开本用户的 AES 密钥；复制在清零及 LocalFree 之前完成，不允许系统提示框。
#[cfg(target_os = "windows")]
fn dpapi_unprotect(wrapped: &[u8]) -> Result<[u8; 32], QuotaFailure> {
    use windows::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData};
    let length = u32::try_from(wrapped.len()).map_err(|_| QuotaFailure::Malformed)?;
    if length == 0 { return Err(QuotaFailure::Malformed); }
    let input = CRYPT_INTEGER_BLOB { cbData: length, pbData: wrapped.as_ptr().cast_mut() };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: 输入切片覆盖同步调用，系统输出交给唯一 RAII 守卫管理。
    unsafe { CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output) }
        .map_err(|_| QuotaFailure::CredentialLocked)?;
    let output = DpapiOutput(output);
    if output.0.pbData.is_null() || output.0.cbData != 32 { return Err(QuotaFailure::Malformed); }
    let mut key = [0_u8; 32];
    // SAFETY: 输出仍然存活且长度已验证为32，复制不形成逃逸借用。
    unsafe { std::ptr::copy_nonoverlapping(output.0.pbData, key.as_mut_ptr(), key.len()); }
    Ok(key)
}

/// Windows v10 为 nonce12 加带16字节认证标签的正文，不要求块对齐。
pub(super) fn decrypt_windows(cipher: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, QuotaFailure> {
    use aes_gcm::{Aes256Gcm, Nonce, aead::{Aead, KeyInit}};
    if cipher.len() < 28 { return Err(QuotaFailure::Malformed); }
    let (nonce, sealed) = cipher.split_at(12);
    Aes256Gcm::new(key.into()).decrypt(Nonce::from_slice(nonce), sealed)
        .map_err(|_| QuotaFailure::CredentialLocked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::credentials::Secret;

    /// 同时覆盖可解密的短明文、凭据下界和不同的非块对齐长度。
    const PLAINTEXT_LENGTHS: [usize; 8] = [1, 7, 8, 13, 16, 17, 32, 49];

    /// 生成相同的真实GCM包装供纯逻辑与Windows DPAPI完整路径复用。
    fn token_fixture(plain: &str, key: &[u8; 32]) -> Secret {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        use aes_gcm::{Aes256Gcm, Nonce, aead::{Aead, KeyInit}};
        let nonce = [9_u8; 12];
        let sealed = Aes256Gcm::new(key.into()).encrypt(Nonce::from_slice(&nonce), plain.as_bytes()).expect("seal fixture");
        Secret::new(format!("cursor-safe-storage:v1:{}", STANDARD.encode([b"v10".as_slice(), &nonce, &sealed].concat()))).expect("wrapped token")
    }

    /// GCM可解单字节不代表它是合法凭据；跨平台提前验证同一组Windows用例边界。
    #[test]
    fn windows_gcm_plaintext_and_credential_bounds_are_distinct() {
        let key = [7_u8; 32];
        for length in PLAINTEXT_LENGTHS {
            let plain = "a".repeat(length);
            let cipher = super::super::cursor_safe_storage::unwrap_cipher(&token_fixture(&plain, &key), b"v10").expect("cipher envelope");
            assert_eq!(decrypt_windows(&cipher, &key).expect("GCM plaintext"), plain.as_bytes());
            assert_eq!(Secret::new(&plain).is_some(), length >= 8, "credential length {length}");
        }
    }

    /// 本机目录锚点与官方 Local State 字段一致，旧错误字段及空包装均拒绝。
    #[test]
    fn windows_local_state_uses_roaming_root_and_chromium_keys() {
        let mut layout = super::super::tests::layout(std::path::PathBuf::from("fixture-home"));
        layout.app_data = Some(std::path::PathBuf::from("fixture-roaming"));
        assert_eq!(local_state_path(&layout), std::path::Path::new("fixture-roaming").join("Cursor/Local State"));
        let state = serde_json::json!({"os_crypt": {"encrypted_key": "RFBBUElmaXh0dXJl"}});
        assert_eq!(wrapped_key(&state).expect("wrapped key"), b"fixture");
        assert!(wrapped_key(&serde_json::json!({"osCrypt": {"encryptedKey": "RFBBUElmaXh0dXJl"}})).is_err());
        assert!(wrapped_key(&serde_json::json!({"os_crypt": {"encrypted_key": "RFBBUEk="}})).is_err());
    }

    /// 真实 Windows DPAPI 包装往返后通过完整 Local State 与 GCM 路径，覆盖非对齐令牌。
    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn windows_dpapi_and_local_state_decode_complete_wrapped_tokens() {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        use windows::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData};
        let key = [7_u8; 32];
        let input = CRYPT_INTEGER_BLOB { cbData: 32, pbData: key.as_ptr().cast_mut() };
        let mut output = CRYPT_INTEGER_BLOB::default();
        // SAFETY: 测试密钥覆盖同步调用，生成缓冲交给同一 RAII 守卫。
        unsafe { CryptProtectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output) }.expect("protect test key");
        let output = DpapiOutput(output);
        // SAFETY: 成功保护产生的缓冲在守卫释放之前复制。
        let protected = unsafe { std::slice::from_raw_parts(output.0.pbData, output.0.cbData as usize) }.to_vec();
        let root = tempfile::tempdir().expect("fixture directory");
        let mut layout = super::super::tests::layout(root.path().to_owned());
        layout.app_data = Some(root.path().to_owned());
        std::fs::create_dir_all(root.path().join("Cursor")).expect("Cursor directory");
        let wrapped = STANDARD.encode([b"DPAPI".as_slice(), &protected].concat());
        std::fs::write(local_state_path(&layout), serde_json::json!({"os_crypt": {"encrypted_key": wrapped}}).to_string()).expect("Local State");
        let (_sender, mut cancel) = tokio::sync::watch::channel(false);
        for length in PLAINTEXT_LENGTHS {
            let plain = "a".repeat(length);
            let result = decode(token_fixture(&plain, &key), &layout, CurlLimits::production(), &mut cancel).await;
            if length < 8 {
                assert!(matches!(result, Err(QuotaFailure::Malformed)), "short credentials stay rejected");
            } else {
                assert_eq!(result.expect("decoded token").expose(), plain);
            }
        }
    }
}
