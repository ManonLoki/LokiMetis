//! 只解释 Cursor 的既有 safeStorage 包装；秘密仅在本次读取期间驻留内存。

use loki_metris_core::QuotaFailure;
use tokio::sync::watch;

use super::{credentials::Secret, transport::CurlLimits};

/// Cursor 官方 token codec 的版本前缀，不把密文当作可发送的访问令牌。
const PREFIX: &str = "cursor-safe-storage:v1:";

/// 旧明文登录态原样保留；系统加密态只有受支持平台成功解密后才能访问上游。
pub(super) async fn decode(token: Secret, limits: CurlLimits, cancel: &mut watch::Receiver<bool>) -> Result<Secret, QuotaFailure> {
    if !token.expose().starts_with(PREFIX) {
        return Ok(token);
    }
    #[cfg(target_os = "macos")]
    { decode_macos(token, std::path::Path::new("/usr/bin/security"), limits, cancel).await }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (limits, cancel);
        Err(QuotaFailure::CredentialLocked)
    }
}

/// 精确查询已有 Cursor Keychain 项；独立进程使系统授权等待也受超时、取消和回收约束。
#[cfg(target_os = "macos")]
async fn decode_macos(token: Secret, security: &std::path::Path, limits: CurlLimits, cancel: &mut watch::Receiver<bool>) -> Result<Secret, QuotaFailure> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let encoded = token.expose().strip_prefix(PREFIX).ok_or(QuotaFailure::Malformed)?;
    let cipher = STANDARD.decode(encoded).map_err(|_| QuotaFailure::Malformed)?;
    let bytes = cipher.strip_prefix(b"v10").ok_or(QuotaFailure::CredentialLocked)?;
    if bytes.is_empty() || bytes.len() % 16 != 0 {
        return Err(QuotaFailure::Malformed);
    }
    let args = ["find-generic-password", "-s", "Cursor Safe Storage", "-a", "Cursor Key", "-w"].map(str::to_owned);
    let mut password = super::transport::run_command(security, &args, None,
        CurlLimits { timeout: limits.timeout.min(std::time::Duration::from_secs(8)), max_body: 1024 }, cancel)
        .await.map_err(|_| QuotaFailure::CredentialLocked)?;
    if password.last() == Some(&b'\n') { password.pop(); }
    let result = decrypt_macos(bytes, &password);
    password.fill(0);
    let mut plain = result?;
    let token = std::str::from_utf8(&plain).ok().and_then(Secret::new).ok_or(QuotaFailure::Malformed);
    plain.fill(0);
    token
}

/// 对应 Chromium OSCrypt macOS v10：PBKDF2-SHA1(1003) 和带 PKCS7 的 AES-128-CBC。
/// 来源：https://github.com/chromium/chromium/blob/148.0.7778.280/components/os_crypt/sync/os_crypt_mac.mm
#[cfg(target_os = "macos")]
fn decrypt_macos(cipher: &[u8], password: &[u8]) -> Result<Vec<u8>, QuotaFailure> {
    use std::ffi::{c_char, c_int, c_void};
    #[link(name = "System")]
    unsafe extern "C" {
        /// 调用系统 PBKDF2；调用者保证切片长度与输出空间真实有效。
        fn CCKeyDerivationPBKDF(algorithm: u32, password: *const c_char, password_len: usize,
            salt: *const u8, salt_len: usize, prf: u32, rounds: u32, key: *mut u8, key_len: usize) -> c_int;
        /// 调用系统 AES；调用者保证缓冲互不重叠且输出容量足够。
        fn CCCrypt(operation: u32, algorithm: u32, options: u32, key: *const c_void, key_len: usize,
            iv: *const c_void, input: *const c_void, input_len: usize,
            output: *mut c_void, output_len: usize, moved: *mut usize) -> c_int;
    }
    if password.is_empty() || cipher.is_empty() || cipher.len() % 16 != 0 {
        return Err(QuotaFailure::Malformed);
    }
    let mut key = [0_u8; 16];
    // SAFETY: 所有指针均来自有效切片，长度匹配其存活范围；CommonCrypto 不保留这些缓冲。
    let derived = unsafe { CCKeyDerivationPBKDF(2, password.as_ptr().cast(), password.len(),
        b"saltysalt".as_ptr(), 9, 1, 1003, key.as_mut_ptr(), key.len()) };
    if derived != 0 {
        key.fill(0);
        return Err(QuotaFailure::CredentialLocked);
    }
    let mut output = vec![0_u8; cipher.len()];
    let mut moved = 0;
    // SAFETY: AES-CBC 解密输出不长于密文；IV 和 key 都是 16 字节，输出及 moved 可写且互不重叠。
    let status = unsafe { CCCrypt(1, 0, 0, key.as_ptr().cast(), key.len(), [b' '; 16].as_ptr().cast(),
        cipher.as_ptr().cast(), cipher.len(), output.as_mut_ptr().cast(), output.len(), &mut moved) };
    key.fill(0);
    if status != 0 || moved > output.len() {
        output.fill(0);
        return Err(QuotaFailure::CredentialLocked);
    }
    output.truncate(moved);
    // 自行严格校验 PKCS7，避免系统兼容实现把错误填充当成完整明文返回。
    let padding = output.last().copied().map(usize::from).filter(|padding| (1..=16).contains(padding));
    let Some(padding) = padding.filter(|padding| *padding <= output.len()
        && output[output.len() - padding..].iter().all(|byte| usize::from(*byte) == *padding)) else {
        output.fill(0);
        return Err(QuotaFailure::CredentialLocked);
    };
    let plain_len = output.len() - padding;
    output[plain_len..].fill(0);
    output.truncate(plain_len);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 旧版明文令牌和已有会话无需钥匙串，也不会变成密文网络请求。
    #[tokio::test]
    async fn plaintext_cursor_tokens_do_not_need_system_storage() {
        let (_sender, mut cancel) = watch::channel(false);
        for raw in ["header.payload.signature", "user_fixture::token"] {
            let token = decode(Secret::new(raw).unwrap(), CurlLimits::production(), &mut cancel).await.unwrap();
            assert_eq!(token.expose(), raw);
        }
    }

    /// macOS 解密使用固定合成向量，坏密码、坏填充不产生可用登录态。
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_cipher_decrypts_fixture_and_rejects_wrong_key() {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let cipher = STANDARD.decode("iSWYXaBIRVFFzU6zHsfmkA==").unwrap();
        assert_eq!(decrypt_macos(&cipher, b"fixture-password").unwrap(), b"fixture-token");
        assert!(matches!(decrypt_macos(&cipher, b"wrong-password"), Err(QuotaFailure::CredentialLocked)));
        assert!(matches!(decrypt_macos(&[], b"fixture-password"), Err(QuotaFailure::Malformed)));
    }

    /// 完整包装通过隔离的钥匙串进程夹具解密；密码和密文都不进入参数或任何网络请求。
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn macos_wrapped_token_uses_only_the_fixed_keychain_query() {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let root = tempfile::tempdir().unwrap();
        let program = super::super::tests::install_fixture(root.path());
        super::super::tests::write(&root.path().join("mode.txt"), "keychain");
        super::super::tests::write(&root.path().join("keychain-out.txt"), "fixture-password\n");
        let cipher = STANDARD.decode("iSWYXaBIRVFFzU6zHsfmkA==").unwrap();
        let token = Secret::new(format!("{PREFIX}{}", STANDARD.encode([b"v10".as_slice(), &cipher].concat()))).unwrap();
        let (_sender, mut cancel) = watch::channel(false);
        let plain = decode_macos(token, &program, CurlLimits::production(), &mut cancel).await.unwrap();
        assert_eq!(plain.expose(), "fixture-token");
        let argv = std::fs::read_to_string(root.path().join("argv.txt")).unwrap();
        assert_eq!(argv, "find-generic-password\n-s\nCursor Safe Storage\n-a\nCursor Key\n-w\n");
        assert!(!argv.contains("fixture-password") && !argv.contains(PREFIX));
        assert!(std::fs::read_to_string(root.path().join("stdin.txt")).unwrap().is_empty());
    }

    /// 损坏包装先拒绝；钥匙串拒绝、缺失和超时保持锁定，取消不会留下本次进程。
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn macos_keychain_failures_never_become_network_tokens() {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let root = tempfile::tempdir().unwrap();
        let program = super::super::tests::install_fixture(root.path());
        let (_sender, mut cancel) = watch::channel(false);
        for encoded in ["!bad", "djEw", "djEweA=="] {
            let token = Secret::new(format!("{PREFIX}{encoded}")).unwrap();
            assert!(decode_macos(token, &program, CurlLimits::production(), &mut cancel).await.is_err());
        }
        assert!(!root.path().join("argv.txt").exists());
        let wrapped = format!("{PREFIX}{}", STANDARD.encode([b"v10".as_slice(), &[0; 16]].concat()));
        for mode in ["keychain-denied", "sleep", "huge"] {
            super::super::tests::write(&root.path().join("mode.txt"), mode);
            let result = decode_macos(Secret::new(&wrapped).unwrap(), &program,
                CurlLimits { timeout: std::time::Duration::from_millis(200), max_body: 1024 }, &mut cancel).await;
            assert!(matches!(result, Err(QuotaFailure::CredentialLocked)), "{mode}");
        }
        let missing = root.path().join("missing-security");
        assert!(matches!(decode_macos(Secret::new(wrapped).unwrap(), &missing,
            CurlLimits::production(), &mut cancel).await, Err(QuotaFailure::CredentialLocked)));
    }

    /// 未实现的操作系统密钥后端保持锁定，不将加密包装降级发送。
    #[cfg(not(target_os = "macos"))]
    #[tokio::test]
    async fn unsupported_system_storage_stays_locked() {
        let (_sender, mut cancel) = watch::channel(false);
        let result = decode(Secret::new(format!("{PREFIX}djEw")).unwrap(), CurlLimits::production(), &mut cancel).await;
        assert!(matches!(result, Err(QuotaFailure::CredentialLocked)));
    }
}
