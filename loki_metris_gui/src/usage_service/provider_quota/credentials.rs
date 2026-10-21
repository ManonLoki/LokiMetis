//! 只读本机登录材料。不写回、不跟随文件本身的符号链接，也不把正文写入日志。

use std::{fs::File, io::Read, path::{Path, PathBuf}};

use loki_metris_core::QuotaFailure;
use serde_json::Value;

/// 登录文件上限。超过后拒绝，避免把数据库或意外大文件读进内存。
const MAX_AUTH_BYTES: usize = 256 * 1024;

/// 请求期间持有的登录秘密。调试输出只显示已隐藏，不显示正文。
pub(crate) struct Secret(String);

impl Secret {
    /// 校验后保存。含空白、引号、反斜杠或控制字符的值不能放进请求头。
    pub(crate) fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        token_safe(&value).then_some(Self(value))
    }

    /// 仅在组装当次请求头时取出正文。
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Secret([redacted])")
    }
}

/// Codex 订阅登录。
pub(crate) struct CodexLogin {
    /// 访问令牌。
    pub token: Secret,
    /// ChatGPT 账户标识。
    pub account_id: String,
}

/// Grok Build 会话。
pub(crate) struct GrokLogin {
    /// 会话令牌。
    pub token: Secret,
}

/// WorkBuddy 明文登录。加密信封不会进入此结构。
pub(crate) struct WorkBuddyLogin {
    /// 访问令牌。
    pub token: Secret,
    /// 可选用户标识，只用于请求头。
    pub user_id: Option<String>,
    /// 登录域。空字符串表示国内站。
    pub domain: String,
    /// 可选企业标识。
    pub enterprise_id: Option<String>,
}

/// 本次读取使用的目录锚点。测试传入隔离目录，不改进程环境。
pub(crate) struct HomeLayout {
    /// 用户主目录。
    pub home: PathBuf,
    /// 显式 Codex 家目录；空则使用主目录下的 `.codex`。
    pub codex_home: Option<PathBuf>,
    /// 显式 Grok 家目录；空则使用主目录下的 `.grok`。
    pub grok_home: Option<PathBuf>,
    /// Windows 的 Roaming 目录。其它系统用主目录下的固定位置，因此这里只在 Windows 上被读取。
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub app_data: Option<PathBuf>,    /// Claude Code statusLine 写下的额度记录；无法确定私有目录时为空。
    pub claude_record: Option<PathBuf>,
}

impl HomeLayout {
    /// 按当前进程的绝对环境变量覆盖默认家目录。
    pub(crate) fn from_process(home: PathBuf) -> Self {
        Self {
            home,
            codex_home: absolute_env("CODEX_HOME"),
            grok_home: absolute_env("GROK_HOME"),
            app_data: absolute_env("APPDATA"),
            claude_record: crate::monitor::relay::claude_statusline_quota_path().ok(),
        }
    }
}

/// 读取 Codex `auth.json` 中的访问令牌和账户标识。
pub(crate) fn read_codex(layout: &HomeLayout) -> Result<CodexLogin, QuotaFailure> {
    let path = layout.codex_home.clone().unwrap_or_else(|| layout.home.join(".codex")).join("auth.json");
    let text = read_regular_text(&path)?;
    let value: Value = serde_json::from_str(&text).map_err(|_| QuotaFailure::Malformed)?;
    let tokens = value.get("tokens").and_then(Value::as_object).ok_or(QuotaFailure::MissingCredentials)?;
    let token = tokens.get("access_token").and_then(Value::as_str).and_then(Secret::new).ok_or(QuotaFailure::MissingCredentials)?;
    let account_id = tokens.get("account_id").and_then(Value::as_str).filter(|value| header_id_safe(value)).ok_or(QuotaFailure::MissingCredentials)?;
    Ok(CodexLogin { token, account_id: account_id.to_owned() })
}

/// 读取 Grok `auth.json` 里最新的可用会话 `key`。
pub(crate) fn read_grok(layout: &HomeLayout, now_unix: i64) -> Result<GrokLogin, QuotaFailure> {
    let path = layout.grok_home.clone().unwrap_or_else(|| layout.home.join(".grok")).join("auth.json");
    let text = read_regular_text(&path)?;
    let value: Value = serde_json::from_str(&text).map_err(|_| QuotaFailure::Malformed)?;
    let mut found = Vec::new();
    collect_grok_keys(&value, &mut found);
    let token = choose_grok_key(found, now_unix).ok_or(QuotaFailure::MissingCredentials)?;
    Ok(GrokLogin { token: Secret::new(token).ok_or(QuotaFailure::MissingCredentials)? })
}

/// Cursor 状态库路径。只定位，不打开数据库文件。
pub(crate) fn cursor_database(layout: &HomeLayout) -> PathBuf {
    #[cfg(target_os = "macos")]
    let root = layout.home.join("Library/Application Support");
    #[cfg(target_os = "windows")]
    let root = layout.app_data.clone().unwrap_or_else(|| layout.home.join("AppData").join("Roaming"));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let root = layout.home.join(".config");
    root.join("Cursor").join("User").join("globalStorage").join("state.vscdb")
}

/// 确认 Cursor 数据库是普通文件。符号链接和缺失都按没有登录态处理。
pub(crate) fn ensure_regular_file(path: &Path) -> Result<(), QuotaFailure> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(QuotaFailure::MissingCredentials),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(QuotaFailure::MissingCredentials),
        Err(_) => Err(QuotaFailure::Transport),
    }
}

/// 从 sqlite 的单行输出取出 Cursor 访问令牌。
pub(crate) fn cursor_token_from_output(text: &str) -> Result<Secret, QuotaFailure> {
    let token = text.trim();
    if token.is_empty() {
        return Err(QuotaFailure::MissingCredentials);
    }
    if token.lines().count() != 1 {
        return Err(QuotaFailure::Malformed);
    }
    Secret::new(token).ok_or(QuotaFailure::Malformed)
}

/// WorkBuddy 国内版与国际版登录文件。只返回候选路径，不探测是否存在。
pub(crate) fn workbuddy_auth_files(layout: &HomeLayout) -> [PathBuf; 2] {
    let root = workbuddy_auth_root(layout);
    [root.join("workbuddy-desktop.info"), root.join("workbuddy-desktop-ai.info")]
}

/// 读取第一份明文 WorkBuddy 登录。只发现加密信封时返回 [`QuotaFailure::CredentialLocked`]。
pub(crate) fn read_workbuddy(layout: &HomeLayout) -> Result<WorkBuddyLogin, QuotaFailure> {
    let mut locked = false;
    for path in workbuddy_auth_files(layout) {
        match read_regular_text(&path) {
            Err(QuotaFailure::MissingCredentials) => continue,
            Err(error) => return Err(error),
            Ok(text) => match parse_workbuddy(&text) {
                Ok(login) => return Ok(login),
                Err(QuotaFailure::CredentialLocked) => locked = true,
                Err(QuotaFailure::MissingCredentials) => {}
                Err(error) => return Err(error),
            },
        }
    }
    Err(if locked { QuotaFailure::CredentialLocked } else { QuotaFailure::MissingCredentials })
}

fn workbuddy_auth_root(layout: &HomeLayout) -> PathBuf {
    #[cfg(target_os = "macos")]
    let root = layout.home.join("Library/Application Support");
    #[cfg(target_os = "windows")]
    let root = layout.app_data.clone().unwrap_or_else(|| layout.home.join("AppData").join("Roaming"));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let root = layout.home.join(".config");
    root.join("CodeBuddyExtension").join("Data").join("Public").join("auth")
}

fn parse_workbuddy(text: &str) -> Result<WorkBuddyLogin, QuotaFailure> {
    let value: Value = serde_json::from_str(text).map_err(|_| QuotaFailure::Malformed)?;
    let auth = value.get("auth").unwrap_or(&value);
    let token_value = auth.get("accessToken").or_else(|| auth.get("access_token")).ok_or(QuotaFailure::MissingCredentials)?;
    if token_value.is_null() {
        return Err(QuotaFailure::MissingCredentials);
    }
    if !token_value.is_string() {
        return Err(QuotaFailure::CredentialLocked);
    }
    let token = token_value.as_str().and_then(Secret::new).ok_or(QuotaFailure::MissingCredentials)?;
    let domain = auth.get("domain").and_then(Value::as_str).unwrap_or("");
    let domain = if domain_safe(domain) { domain.trim().to_owned() } else { String::new() };
    Ok(WorkBuddyLogin {
        token,
        user_id: first_id(auth, &["uid", "userId", "user_id"]),
        domain,
        enterprise_id: first_id(auth, &["enterpriseId", "enterprise_id"]),
    })
}

fn collect_grok_keys(value: &Value, found: &mut Vec<(String, Option<i64>)>) {
    let Some(object) = value.as_object() else { return };
    if let Some(token) = object.get("key").and_then(Value::as_str).filter(|token| token_safe(token)) {
        let expires = object.get("expires_at").or_else(|| object.get("expiresAt")).and_then(expires_unix);
        found.push((token.to_owned(), expires));
    }
    for child in object.values() {
        if child.is_object() {
            collect_grok_keys(child, found);
        }
    }
}

fn choose_grok_key(mut found: Vec<(String, Option<i64>)>, now_unix: i64) -> Option<String> {
    found.sort_by(|(left_token, left_expiry), (right_token, right_expiry)| {
        let left_open = left_expiry.is_none_or(|expires| expires > now_unix);
        let right_open = right_expiry.is_none_or(|expires| expires > now_unix);
        right_open.cmp(&left_open).then(right_expiry.cmp(left_expiry)).then(left_token.cmp(right_token))
    });
    found.into_iter().next().map(|(token, _)| token)
}

fn expires_unix(value: &Value) -> Option<i64> {
    if let Some(number) = value.as_i64() {
        return Some(if number.abs() > 1_000_000_000_000 { number / 1000 } else { number });
    }
    value.as_str().and_then(|text| text.parse::<jiff::Timestamp>().ok()).map(|timestamp| timestamp.as_second())
}

fn first_id(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| value.get(*key).and_then(Value::as_str).filter(|text| header_id_safe(text)).map(str::to_owned))
}

fn read_regular_text(path: &Path) -> Result<String, QuotaFailure> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Err(QuotaFailure::MissingCredentials),
        Err(_) => return Err(QuotaFailure::Transport),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_AUTH_BYTES as u64 {
        return Err(if metadata.is_file() && !metadata.file_type().is_symlink() { QuotaFailure::Malformed } else { QuotaFailure::MissingCredentials });
    }
    let mut file = File::open(path).map_err(io_failure)?;
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let read = file.read(&mut chunk).map_err(io_failure)?;
        if read == 0 {
            break;
        }
        if buffer.len().saturating_add(read) > MAX_AUTH_BYTES {
            return Err(QuotaFailure::Malformed);
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    String::from_utf8(buffer).map_err(|_| QuotaFailure::Malformed)
}

fn io_failure(error: std::io::Error) -> QuotaFailure {
    if error.kind() == std::io::ErrorKind::NotFound { QuotaFailure::MissingCredentials } else { QuotaFailure::Transport }
}

fn absolute_env(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).filter(|path| path.is_absolute())
}

fn token_safe(value: &str) -> bool {
    (8..=8192).contains(&value.len())
        && value.chars().all(|ch| ch.is_ascii() && !ch.is_ascii_control() && !ch.is_ascii_whitespace() && ch != '"' && ch != '\\' && ch != ';')
}

fn header_id_safe(value: &str) -> bool {
    (1..=128).contains(&value.len()) && value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

fn domain_safe(value: &str) -> bool {
    value.len() <= 128 && value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-'))
}
