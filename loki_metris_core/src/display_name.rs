//! 受控项目路径末段与短线程标题的展示名 sanitizer。

/// 展示名最大字符数（按 Unicode 标量计）。
pub const SAFE_DISPLAY_NAME_MAX_CHARS: usize = 128;

/// 从路径或路径种子提取可持久化的末段展示名；拒绝绝对路径形态的完整值。
pub fn safe_path_basename(seed: &str) -> Option<String> {
    let trimmed = seed.trim();
    if trimmed.is_empty() {
        return None;
    }
    let last = trimmed
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(trimmed);
    sanitize_display_name(last)
}

/// 将 Grok `sessions` 下 URL 编码工作目录还原为受控末段，不保留完整路径。
pub fn grok_project_display_label(directory_name: &str) -> Option<String> {
    let trimmed = directory_name.trim();
    if trimmed.is_empty() {
        return None;
    }
    let decoded = percent_decode(trimmed);
    safe_path_basename(&decoded).or_else(|| sanitize_display_name(trimmed))
}

/// 只解码 `%HH` 字节，不把 `+` 当空格，避免把路径编码误读成 query。
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
        {
            out.push((high << 4) | low);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 把单个 ASCII 十六进制字符转成 0–15。
fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// 将 Claude `projects` 目录名转为受控末段：普通名直接消毒；以 `-` 开头的编码路径先按 Claude 规则还原再取末段。
pub fn claude_project_display_label(directory_name: &str) -> Option<String> {
    let trimmed = directory_name.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('-') && trimmed.contains('-') {
        let decoded = trimmed.replace('-', "/");
        return safe_path_basename(&decoded);
    }
    sanitize_display_name(trimmed)
}

/// 消毒可选短线程标题；不含路径分隔符与控制字符。
pub fn safe_thread_title(value: &str) -> Option<String> {
    sanitize_display_name(value.trim())
}

/// 清理展示名中的路径、控制字符与空白，只保留安全的单行标签。
fn sanitize_display_name(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value.chars().count() > SAFE_DISPLAY_NAME_MAX_CHARS
        || value.chars().any(char::is_control)
        || value.contains('/')
        || value.contains('\\')
    {
        None
    } else {
        Some(value.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 验证展示名只保留绝对路径末段而不泄露父目录。
    fn basename_strips_absolute_paths() {
        assert_eq!(
            safe_path_basename("/private/secret/my-app"),
            Some("my-app".to_owned())
        );
        assert_eq!(
            safe_path_basename(r"C:\Users\x\repo"),
            Some("repo".to_owned())
        );
        assert_eq!(safe_path_basename("plain"), Some("plain".to_owned()));
        assert_eq!(safe_path_basename("/"), None);
        assert_eq!(safe_path_basename(""), None);
    }

    #[test]
    /// 验证 Claude 编码目录只提取最后一个安全项目段。
    fn claude_encoded_directory_uses_final_segment() {
        assert_eq!(
            claude_project_display_label(
                "-Users-manon-Documents-ym-work-ai-bifang_codex_usage_collect"
            ),
            Some("bifang_codex_usage_collect".to_owned())
        );
        assert_eq!(
            claude_project_display_label("project-a"),
            Some("project-a".to_owned())
        );
    }

    /// 验证 Grok URL 编码工作目录只展示末段，不回传完整路径。
    #[test]
    fn grok_encoded_cwd_uses_final_segment() {
        assert_eq!(
            grok_project_display_label("%2FUsers%2Fme%2Fbifang_codex_usage_collect"),
            Some("bifang_codex_usage_collect".to_owned())
        );
        assert_eq!(
            grok_project_display_label("plain-project"),
            Some("plain-project".to_owned())
        );
    }

    #[test]
    /// 验证线程标题拒绝路径形态与控制字符。
    fn thread_title_rejects_paths_and_controls() {
        assert_eq!(safe_thread_title("迁移看板"), Some("迁移看板".to_owned()));
        assert_eq!(safe_thread_title("/secret/name"), None);
        assert_eq!(safe_thread_title("bad\nname"), None);
        assert_eq!(safe_thread_title(""), None);
    }
}
