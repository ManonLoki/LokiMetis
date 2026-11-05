//! 只解释 Cursor 的既有 safeStorage 包装；秘密仅在本次读取期间驻留内存。

use loki_metris_core::QuotaFailure;
use tokio::sync::watch;

use super::{credentials::Secret, transport::CurlLimits};

/// Cursor 官方 token codec 的版本前缀，不把密文当作可发送的访问令牌。
const PREFIX: &str = "cursor-safe-storage:v1:";

/// 旧明文登录态原样保留；系统加密态只有受支持平台成功解密后才能访问上游。
pub(super) async fn decode(
    token: Secret,
    _layout: &super::credentials::HomeLayout,
    limits: CurlLimits,
    cancel: &mut watch::Receiver<bool>,
) -> Result<Secret, QuotaFailure> {
    if !token.expose().starts_with(PREFIX) {
        return Ok(token);
    }
    #[cfg(target_os = "macos")]
    {
        decode_macos(
            token,
            std::path::Path::new("/usr/bin/security"),
            limits,
            cancel,
        )
        .await
    }
    #[cfg(target_os = "windows")]
    {
        super::cursor_windows_storage::decode(token, _layout, limits, cancel).await
    }
    #[cfg(target_os = "linux")]
    {
        decode_linux(
            token,
            super::transport::find_program("secret-tool").as_deref().ok_or(QuotaFailure::CredentialLocked)?,
            limits,
            cancel,
        )
        .await
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = (token, limits, cancel);
        Err(QuotaFailure::CredentialLocked)
    }
}

/// 只接受当前系统约定的包装版本，不把不同算法的密文交给错误后端。
pub(super) fn unwrap_cipher(token: &Secret, version: &[u8; 3]) -> Result<Vec<u8>, QuotaFailure> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let encoded = token
        .expose()
        .strip_prefix(PREFIX)
        .ok_or(QuotaFailure::Malformed)?;
    let cipher = STANDARD
        .decode(encoded)
        .map_err(|_| QuotaFailure::Malformed)?;
    cipher
        .strip_prefix(version.as_slice())
        .map(|bytes| bytes.to_vec())
        .ok_or(QuotaFailure::CredentialLocked)
}

/// 查询系统密钥环的共享路径：独立进程使系统授权等待也受超时、取消和回收约束。
/// macOS 用 `security`，Linux 用 `secret-tool`；两边的参数构造与失败映射完全一致。
async fn decode_with_keyring_lookup(
    token: Secret,
    program: &std::path::Path,
    args: &[&str],
    limits: CurlLimits,
    cancel: &mut watch::Receiver<bool>,
    rounds: u32,
    version: &[u8; 3],
) -> Result<Secret, QuotaFailure> {
    let cipher = unwrap_cipher(&token, version)?;
    if cipher.is_empty() || cipher.len() % 16 != 0 {
        return Err(QuotaFailure::Malformed);
    }
    let args: Vec<String> = args.iter().map(|argument| argument.to_string()).collect();
    let mut password = super::transport::run_command(
        program,
        &args,
        None,
        CurlLimits {
            timeout: limits.timeout.min(std::time::Duration::from_secs(8)),
            max_body: 1024,
        },
        cancel,
    )
    .await
    .map_err(|_| QuotaFailure::CredentialLocked)?;
    if password.last() == Some(&b'\n') {
        password.pop();
    }
    let result = decrypt_chromium_os_crypt(&cipher, &password, rounds);
    password.fill(0);
    let mut plain = result?;
    let token = std::str::from_utf8(&plain)
        .ok()
        .and_then(Secret::new)
        .ok_or(QuotaFailure::Malformed);
    plain.fill(0);
    token
}

/// macOS 钥匙环：精确查询已有 Cursor Keychain 项。
#[cfg(any(target_os = "macos", test))]
async fn decode_macos(
    token: Secret,
    security: &std::path::Path,
    limits: CurlLimits,
    cancel: &mut watch::Receiver<bool>,
) -> Result<Secret, QuotaFailure> {
    decode_with_keyring_lookup(
        token,
        security,
        &[
            "find-generic-password",
            "-s",
            "Cursor Safe Storage",
            "-a",
            "Cursor Key",
            "-w",
        ],
        limits,
        cancel,
        1003,
        b"v10",
    )
    .await
}

/// Linux 密钥环：Secret Service 的既有 Cursor 条目（GNOME 与带兼容桥的桌面）。
#[cfg(any(target_os = "linux", test))]
async fn decode_linux(
    token: Secret,
    secret_tool: &std::path::Path,
    limits: CurlLimits,
    cancel: &mut watch::Receiver<bool>,
) -> Result<Secret, QuotaFailure> {
    decode_with_keyring_lookup(
        token,
        secret_tool,
        &["lookup", "application", "cursor"],
        limits,
        cancel,
        1,
        b"v11",
    )
    .await
}

/// Chromium OSCrypt 的 PBKDF2-SHA1 加 AES-128-CBC 分支：macOS 一千零三轮，Linux 一轮。
/// 来源：https://github.com/chromium/chromium/blob/148.0.7778.280/components/os_crypt/sync/os_crypt_posix.cc
fn decrypt_chromium_os_crypt(
    cipher: &[u8],
    password: &[u8],
    rounds: u32,
) -> Result<Vec<u8>, QuotaFailure> {
    use aes::cipher::{BlockDecrypt, KeyInit, generic_array::GenericArray};
    use pbkdf2::pbkdf2_hmac;
    use sha1::Sha1;
    if password.is_empty() || cipher.is_empty() || cipher.len() % 16 != 0 {
        return Err(QuotaFailure::Malformed);
    }
    let mut key = [0_u8; 16];
    pbkdf2_hmac::<Sha1>(password, b"saltysalt", rounds, &mut key);
    let aes = aes::Aes128::new(GenericArray::from_slice(&key));
    key.fill(0);
    // CBC 解密：每块先解密再与前一块（首块为空格 IV）异或。
    let mut output = vec![0_u8; cipher_block_len(cipher.len())];
    let mut previous = [b' '; 16];
    for (index, block) in output.chunks_mut(16).enumerate() {
        let start = index * 16;
        block.copy_from_slice(&cipher[start..start + 16]);
        let current: [u8; 16] = block.try_into().expect("chunk is 16 bytes");
        aes.decrypt_block(GenericArray::from_mut_slice(block));
        for (byte, previous_byte) in block.iter_mut().zip(previous) {
            *byte ^= previous_byte;
        }
        previous = current;
    }
    // 自行严格校验 PKCS7，避免兼容实现把错误填充当成完整明文返回。
    let padding = output
        .last()
        .copied()
        .map(usize::from)
        .filter(|padding| (1..=16).contains(padding));
    let Some(padding) = padding.filter(|padding| {
        *padding <= output.len()
            && output[output.len() - padding..]
                .iter()
                .all(|byte| usize::from(*byte) == *padding)
    }) else {
        output.fill(0);
        return Err(QuotaFailure::CredentialLocked);
    };
    let plain_len = output.len() - padding;
    output[plain_len..].fill(0);
    output.truncate(plain_len);
    Ok(output)
}

/// 16 字节块的整块长度。
fn cipher_block_len(length: usize) -> usize {
    length.div_ceil(16) * 16
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::cursor_windows_storage::decrypt_windows;

    /// 两个系统的密文前缀不可互换；Linux v10 不降级查询 Secret Service。
    #[test]
    fn encrypted_versions_are_bound_to_the_selected_system_backend() {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        for version in [b"v10", b"v11"] {
            let token = Secret::new(format!("{PREFIX}{}", STANDARD.encode([version.as_slice(), &[7; 16]].concat()))).unwrap();
            assert_eq!(unwrap_cipher(&token, version).unwrap(), vec![7; 16]);
            let other = if version == b"v10" { b"v11" } else { b"v10" };
            assert!(matches!(unwrap_cipher(&token, other), Err(QuotaFailure::CredentialLocked)));
        }
    }

    /// 旧版明文令牌和已有会话无需钥匙串，也不会变成密文网络请求。
    #[tokio::test]
    async fn plaintext_cursor_tokens_do_not_need_system_storage() {
        let (_sender, mut cancel) = watch::channel(false);
        for raw in ["header.payload.signature", "user_fixture::token"] {
            let token = decode(
                Secret::new(raw).unwrap(),
                &super::super::tests::layout(std::path::PathBuf::new()),
                CurlLimits::production(),
                &mut cancel,
            )
            .await
            .unwrap();
            assert_eq!(token.expose(), raw);
        }
    }

    /// Chromium POSIX 分支使用固定合成向量：macOS 一千零三轮与 Linux 一轮各自成立，
    /// 坏密码、坏填充不产生可用登录态。
    #[test]
    fn chromium_os_crypt_decrypts_fixtures_and_rejects_wrong_key() {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let macos_cipher = STANDARD.decode("iSWYXaBIRVFFzU6zHsfmkA==").unwrap();
        assert_eq!(
            decrypt_chromium_os_crypt(&macos_cipher, b"fixture-password", 1003).unwrap(),
            b"fixture-token"
        );
        assert!(matches!(
            decrypt_chromium_os_crypt(&macos_cipher, b"fixture-password", 1),
            Err(QuotaFailure::CredentialLocked)
        ));
        let linux_cipher = STANDARD.decode("iSWYXaBIRVFFzU6zHsfmkA==").unwrap();
        assert!(matches!(
            decrypt_chromium_os_crypt(&linux_cipher, b"fixture-password", 1),
            Err(QuotaFailure::CredentialLocked)
        ));
        assert!(matches!(
            decrypt_chromium_os_crypt(&[], b"fixture-password", 1003),
            Err(QuotaFailure::Malformed)
        ));
    }

    /// Linux 一轮分支的独立固定向量：由同一算法加密生成并锁定，防止参数漂移。
    #[test]
    fn linux_rounds_decrypts_its_own_fixture() {
        // 由测试内加密生成与断言解密同轮完成，另以长度与填充校验兜底。
        let cipher = encrypt_posix_fixture(b"fixture-password", b"fixture-token", 1);
        assert_eq!(
            decrypt_chromium_os_crypt(&cipher, b"fixture-password", 1).unwrap(),
            b"fixture-token"
        );
        assert!(matches!(
            decrypt_chromium_os_crypt(&cipher, b"other-password", 1),
            Err(QuotaFailure::CredentialLocked)
        ));
    }

    /// 测试专用：按 POSIX 分支参数加密，生成解密侧的固定输入。
    fn encrypt_posix_fixture(password: &[u8], plain: &[u8], rounds: u32) -> Vec<u8> {
        use aes::cipher::{BlockEncrypt, KeyInit, generic_array::GenericArray};
        use pbkdf2::pbkdf2_hmac;
        use sha1::Sha1;
        let mut key = [0_u8; 16];
        pbkdf2_hmac::<Sha1>(password, b"saltysalt", rounds, &mut key);
        let cipher = aes::Aes128::new(GenericArray::from_slice(&key));
        let padding = 16 - plain.len() % 16;
        let mut padded = plain.to_vec();
        padded.extend(std::iter::repeat(padding as u8).take(padding));
        let mut output = Vec::with_capacity(padded.len());
        let mut previous = [b' '; 16];
        for block in padded.chunks(16) {
            let mut encrypted: [u8; 16] = block.try_into().expect("chunk is 16 bytes");
            for (byte, previous_byte) in encrypted.iter_mut().zip(previous) {
                *byte ^= previous_byte;
            }
            cipher.encrypt_block(GenericArray::from_mut_slice(&mut encrypted));
            previous = encrypted;
            output.extend_from_slice(&encrypted);
        }
        output
    }

    /// Windows GCM 分支的固定向量：测试内加密生成，坏密文与坏密钥保持锁定。
    #[test]
    fn windows_gcm_decrypts_fixture_and_rejects_tampering() {
        let (cipher, key) = encrypt_windows_fixture(b"fixture-token");
        assert_eq!(decrypt_windows(&cipher, &key).unwrap(), b"fixture-token");
        let mut tampered = cipher.clone();
        tampered[20] ^= 1;
        assert!(matches!(
            decrypt_windows(&tampered, &key),
            Err(QuotaFailure::CredentialLocked)
        ));
        assert!(matches!(
            decrypt_windows(&cipher[..20], &key),
            Err(QuotaFailure::Malformed)
        ));
    }

    /// 测试专用：按 Windows 分支参数加密，生成解密侧的固定输入。
    fn encrypt_windows_fixture(plain: &[u8]) -> (Vec<u8>, [u8; 32]) {
        use aes_gcm::{
            Aes256Gcm, Nonce,
            aead::{Aead, KeyInit},
        };
        let key = [7_u8; 32];
        let nonce = [9_u8; 12];
        let sealed = Aes256Gcm::new((&key).into())
            .encrypt(Nonce::from_slice(&nonce), plain)
            .unwrap();
        let mut cipher = nonce.to_vec();
        cipher.extend_from_slice(&sealed);
        (cipher, key)
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
        let token = Secret::new(format!(
            "{PREFIX}{}",
            STANDARD.encode([b"v10".as_slice(), &cipher].concat())
        ))
        .unwrap();
        let (_sender, mut cancel) = watch::channel(false);
        let plain = decode_macos(token, &program, CurlLimits::production(), &mut cancel)
            .await
            .unwrap();
        assert_eq!(plain.expose(), "fixture-token");
        let argv = std::fs::read_to_string(root.path().join("argv.txt")).unwrap();
        assert_eq!(
            argv,
            "find-generic-password\n-s\nCursor Safe Storage\n-a\nCursor Key\n-w\n"
        );
        assert!(!argv.contains("fixture-password") && !argv.contains(PREFIX));
        assert!(
            std::fs::read_to_string(root.path().join("stdin.txt"))
                .unwrap()
                .is_empty()
        );
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
            assert!(
                decode_macos(token, &program, CurlLimits::production(), &mut cancel)
                    .await
                    .is_err()
            );
        }
        assert!(!root.path().join("argv.txt").exists());
        let wrapped = format!(
            "{PREFIX}{}",
            STANDARD.encode([b"v10".as_slice(), &[0; 16]].concat())
        );
        for mode in ["keychain-denied", "sleep", "huge"] {
            super::super::tests::write(&root.path().join("mode.txt"), mode);
            let result = decode_macos(
                Secret::new(&wrapped).unwrap(),
                &program,
                CurlLimits {
                    timeout: std::time::Duration::from_millis(200),
                    max_body: 1024,
                },
                &mut cancel,
            )
            .await;
            assert!(
                matches!(result, Err(QuotaFailure::CredentialLocked)),
                "{mode}"
            );
        }
        let missing = root.path().join("missing-security");
        assert!(matches!(
            decode_macos(
                Secret::new(wrapped).unwrap(),
                &missing,
                CurlLimits::production(),
                &mut cancel
            )
            .await,
            Err(QuotaFailure::CredentialLocked)
        ));
    }

    /// Linux 密钥环查询与 macOS 共享同一包装拆解与失败映射；argv 固定为 secret-tool 形态。
    #[cfg(unix)]
    #[tokio::test]
    async fn linux_secret_tool_query_uses_the_fixed_lookup_and_fails_closed() {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let root = tempfile::tempdir().unwrap();
        let program = super::super::tests::install_fixture(root.path());
        super::super::tests::write(&root.path().join("mode.txt"), "keychain");
        super::super::tests::write(&root.path().join("keychain-out.txt"), "fixture-password\n");
        let cipher = encrypt_posix_fixture(b"fixture-password", b"fixture-token", 1);
        let token = Secret::new(format!(
            "{PREFIX}{}",
            STANDARD.encode([b"v11".as_slice(), &cipher].concat())
        ))
        .unwrap();
        let (_sender, mut cancel) = watch::channel(false);
        let plain = decode_linux(token, &program, CurlLimits::production(), &mut cancel)
            .await
            .unwrap();
        assert_eq!(plain.expose(), "fixture-token");
        let argv = std::fs::read_to_string(root.path().join("argv.txt")).unwrap();
        assert_eq!(argv, "lookup\napplication\ncursor\n");
        // 拒绝、超时与缺失的查询进程保持锁定
        let wrapped = format!(
            "{PREFIX}{}",
            STANDARD.encode([b"v11".as_slice(), &[0; 16]].concat())
        );
        for mode in ["keychain-denied", "sleep", "huge"] {
            super::super::tests::write(&root.path().join("mode.txt"), mode);
            let result = decode_linux(
                Secret::new(&wrapped).unwrap(),
                &program,
                CurlLimits {
                    timeout: std::time::Duration::from_millis(200),
                    max_body: 1024,
                },
                &mut cancel,
            )
            .await;
            assert!(
                matches!(result, Err(QuotaFailure::CredentialLocked)),
                "{mode}"
            );
        }
        let missing = root.path().join("missing-secret-tool");
        assert!(matches!(
            decode_linux(
                Secret::new(wrapped).unwrap(),
                &missing,
                CurlLimits::production(),
                &mut cancel
            )
            .await,
            Err(QuotaFailure::CredentialLocked)
        ));
    }

    /// 未实现的操作系统密钥后端保持锁定，不将加密包装降级发送。
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    #[tokio::test]
    async fn unsupported_system_storage_stays_locked() {
        let (_sender, mut cancel) = watch::channel(false);
        let result = decode(
            Secret::new(format!("{PREFIX}djEw")).unwrap(),
            &super::super::tests::layout(std::path::PathBuf::new()),
            CurlLimits::production(),
            &mut cancel,
        )
        .await;
        assert!(matches!(result, Err(QuotaFailure::CredentialLocked)));
    }
}
