//! 把平台原生路径无损编码为跨进程稳定的内部键，并生成内容无关的稳定 ID。

use std::path::Path;

/// 把平台原生路径无损编码为仅用于本机稳定 ID 的文本，避免有损转换合并不同来源。
// 绝大多数路径本身就是合法 UTF-8，直接原样返回最简单也最省事；只有当
// 路径包含非法 UTF-8 字节（这在 Unix 上是允许的，极少见但可能发生）时，
// 才逐字节转成十六进制文本存进一个专用命名空间前缀（LOSSLESS_PATH_PREFIX）
// 下——用 `\u{1f}`（不可打印控制字符）开头，确保这个前缀不可能出现在
// 正常路径文本里，从而两类编码方式产生的键永远不会互相冲突或被误判为
// 同一个路径。“无损”是关键要求：如果用有损转换（比如把非法字节替换成
// 占位符）生成 ID，两个不同的原始路径可能被错误地映射成同一个键。
pub fn path_key(path: &Path) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;

        let bytes = path.as_os_str().as_bytes();
        if let Ok(text) = std::str::from_utf8(bytes)
            && !text.starts_with(LOSSLESS_PATH_PREFIX)
        {
            return text.to_owned();
        }
        encode_path_bytes("unix", bytes)
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;

        if let Some(text) = path.to_str() {
            let normalized = text.replace('\\', "/");
            if !normalized.starts_with(LOSSLESS_PATH_PREFIX) {
                return normalized;
            }
        }
        let mut key = format!("{LOSSLESS_PATH_PREFIX}windows:");
        for unit in path.as_os_str().encode_wide() {
            push_hex_byte(&mut key, (unit >> 8) as u8);
            push_hex_byte(&mut key, unit as u8);
        }
        key
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        let text = path.to_string_lossy();
        if !text.starts_with(LOSSLESS_PATH_PREFIX) {
            return text.into_owned();
        }
        encode_path_bytes("other", text.as_bytes())
    }
}

/// 保留正常 UTF-8 路径的旧稳定键，只为需要转义的原生路径建立独立命名空间。
const LOSSLESS_PATH_PREFIX: &str = "\u{1f}codex-path:";

/// 把不能直接保留的原生路径字节编码到不会与普通路径重叠的命名空间。
#[cfg(not(target_os = "windows"))]
fn encode_path_bytes(platform: &str, bytes: &[u8]) -> String {
    let mut key = format!("{LOSSLESS_PATH_PREFIX}{platform}:");
    for byte in bytes {
        push_hex_byte(&mut key, *byte);
    }
    key
}

/// 以固定小写十六进制追加一个原生路径字节。
fn push_hex_byte(output: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    output.push(char::from(HEX[usize::from(byte >> 4)]));
    output.push(char::from(HEX[usize::from(byte & 0x0f)]));
}

/// 使用固定 FNV-1a 算法生成内容无关、跨进程稳定的短 ID。
// 这是全模块共用的“匿名化 ID 生成器”：namespace 区分 ID 用途
// （"root"/"source"/"thread"/"project"/"scan"/"call" 等，见各调用点），
// 避免不同用途但巧合内容相同的值哈希出同一个 ID；输出格式固定为
// `{namespace}-{16位十六进制哈希}`，同一输入永远得到同一输出（稳定），
// 但不能从输出反推原始 value（单向），是本产品“索引不保存真实路径/
// session id，只保存可去重比较的匿名指纹”这条隐私原则的底层工具函数。
pub fn stable_id(namespace: &str, value: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in namespace.bytes().chain([0]).chain(value.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{namespace}-{hash:016x}")
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::ffi::OsString;
    #[cfg(unix)]
    use std::os::unix::ffi::OsStringExt;
    #[cfg(unix)]
    use std::path::PathBuf;

    use super::*;

    /// 验证普通 UTF-8 路径保持既有稳定键，避免升级后无故重建 checkpoint。
    #[test]
    fn path_key_preserves_existing_utf8_key() {
        assert_eq!(
            path_key(Path::new("sessions/rollout-known.jsonl")),
            "sessions/rollout-known.jsonl"
        );
    }

    /// 验证 Unix 反斜杠文件名不会与目录分隔路径共享同一个内部键。
    #[cfg(unix)]
    #[test]
    fn path_key_preserves_backslash_and_separator_distinction() {
        assert_ne!(
            path_key(Path::new("area\\rollout")),
            path_key(Path::new("area/rollout"))
        );
    }

    /// 验证非 UTF-8 原生路径字节不会经有损字符串替换后碰撞。
    // `OsString::from_vec` 是 Unix 专属 API：可以构造出包含任意字节
    // （包括不构成合法 UTF-8 序列的字节，如这里的 0x80/0x81）的路径，这是
    // 真实文件系统里合法但 Rust `String` 无法直接表示的边界情况；
    // 两个仅相差最后一字节的“畸形”路径必须编码出不同的键，验证的正是
    // 上面“无损十六进制编码”分支没有丢信息。
    #[cfg(unix)]
    #[test]
    fn path_key_preserves_non_utf8_bytes() {
        let first = PathBuf::from(OsString::from_vec(vec![b'a', 0x80]));
        let second = PathBuf::from(OsString::from_vec(vec![b'a', 0x81]));

        assert_ne!(path_key(&first), path_key(&second));
    }

    /// 验证同一输入始终得到同一稳定 ID，且格式带命名空间前缀。
    #[test]
    fn stable_id_is_deterministic_and_namespaced() {
        let first = stable_id("root", "/tmp/example");
        let second = stable_id("root", "/tmp/example");
        assert_eq!(first, second);
        assert!(first.starts_with("root-"));
        assert_ne!(stable_id("source", "/tmp/example"), first);
    }
}
