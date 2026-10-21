//! 从本机已有记录里找 5 小时、周或月窗口。找不到就返回空，调用方再走远程。

use std::{io::{Read, Seek, SeekFrom}, path::{Path, PathBuf}, time::Duration};

use loki_metris_core::{ProviderQuota, QuotaProvider, parse_provider_quota};
use tokio::sync::watch;

use super::{
    credentials::{self, HomeLayout},
    transport::{self, CurlLimits},
};

pub(crate) const TAIL_BYTES: u64 = 1024 * 1024;
const MAX_WHOLE_FILE: u64 = 256 * 1024;

/// 本地只要解释出一种目标窗口就返回它。读取失败、没有文件或只有其它周期都视为未获取。
pub(crate) async fn read_local_quota(
    provider: QuotaProvider,
    layout: &HomeLayout,
    sqlite: Option<&Path>,
    limits: CurlLimits,
    cancel: &mut watch::Receiver<bool>,
) -> Option<ProviderQuota> {
    if *cancel.borrow() {
        return None;
    }
    match provider {
        QuotaProvider::Codex => newest_window(provider, &codex_session_roots(layout)),
        QuotaProvider::Claude => claude_window(layout, unix_now()),
        QuotaProvider::Grok => file_window(provider, &grok_log(layout)),
        QuotaProvider::Cursor => cursor_window(layout, sqlite, limits, cancel).await,
        QuotaProvider::WorkBuddy => newest_window(provider, &workbuddy_usage_dirs(layout)),
    }
}

/// statusLine 记录只在 Claude Code 运行时刷新，已过重置时刻的窗口不再展示。
pub(crate) fn claude_window(layout: &HomeLayout, now_unix: i64) -> Option<ProviderQuota> {
    let mut quota = file_window(QuotaProvider::Claude, layout.claude_record.as_deref()?)?;
    quota.drop_expired_windows(now_unix);
    quota.has_usage_window().then_some(quota)
}

fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|duration| duration.as_secs() as i64).unwrap_or(0)
}

fn codex_session_roots(layout: &HomeLayout) -> Vec<PathBuf> {
    let root = layout.codex_home.clone().unwrap_or_else(|| layout.home.join(".codex"));
    vec![root.join("sessions"), root.join("archived_sessions")]
}

fn grok_log(layout: &HomeLayout) -> PathBuf {
    layout.grok_home.clone().unwrap_or_else(|| layout.home.join(".grok")).join("logs").join("unified.jsonl")
}

fn workbuddy_usage_dirs(layout: &HomeLayout) -> Vec<PathBuf> {
    vec![layout.home.join(".codebuddy").join("usage-data"), layout.home.join(".workbuddy").join("usage-data")]
}

fn newest_window(provider: QuotaProvider, roots: &[PathBuf]) -> Option<ProviderQuota> {
    let mut files = Vec::new();
    for root in roots {
        collect_records(root, 0, &mut files);
    }
    files.sort_by(|left, right| right.0.cmp(&left.0));
    files.into_iter().find_map(|(_, path)| file_window(provider, &path))
}

fn file_window(provider: QuotaProvider, path: &Path) -> Option<ProviderQuota> {
    let text = read_record(path)?;
    quota_in_text(provider, &text)
}

fn collect_records(dir: &Path, depth: usize, out: &mut Vec<(u64, PathBuf)>) {
    if depth > 4 {
        return;
    }
    let Ok(metadata) = std::fs::symlink_metadata(dir) else { return };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = std::fs::symlink_metadata(&path) else { continue };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_records(&path, depth + 1, out);
        } else if metadata.is_file() && is_record_name(&path) {
            let modified = metadata.modified().ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok()).map(|time| time.as_secs()).unwrap_or(0);
            out.push((modified, path));
        }
    }
}

fn is_record_name(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else { return false };
    name.ends_with(".jsonl") || name.ends_with(".json")
}

fn read_record(path: &Path) -> Option<String> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return None;
    }
    let mut file = std::fs::File::open(path).ok()?;
    let size = metadata.len();
    let tail = if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") { TAIL_BYTES.min(size) } else { MAX_WHOLE_FILE.min(size) };
    if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") && size > MAX_WHOLE_FILE {
        return None;
    }
    file.seek(SeekFrom::Start(size - tail)).ok()?;
    let mut buffer = Vec::new();
    file.take(tail).read_to_end(&mut buffer).ok()?;
    // 切点可能落在 UTF-8 续字节上。先按字节丢掉第一个换行之前的残段，再解码其后的完整行。
    if size > tail {
        let Some(split) = buffer.iter().position(|byte| *byte == b'\n') else { return None };
        buffer.drain(..split + 1);
    }
    String::from_utf8(buffer).ok()
}

fn quota_in_text(provider: QuotaProvider, text: &str) -> Option<ProviderQuota> {
    if let Some(quota) = window_in_document(provider, text.trim()) {
        return Some(quota);
    }
    let mut found = None;
    for line in text.lines() {
        if let Some(quota) = window_in_document(provider, line.trim()) {
            found = Some(quota);
        }
    }
    found
}

fn window_in_document(provider: QuotaProvider, text: &str) -> Option<ProviderQuota> {
    if !text.starts_with('{') {
        return None;
    }
    let quota = parse_provider_quota(provider, text).ok()?;
    quota.has_usage_window().then_some(quota)
}

async fn cursor_window(layout: &HomeLayout, sqlite: Option<&Path>, limits: CurlLimits, cancel: &mut watch::Receiver<bool>) -> Option<ProviderQuota> {
    let database = credentials::cursor_database(layout);
    credentials::ensure_regular_file(&database).ok()?;
    let program = sqlite?;
    let args = transport::cursor_query_args(&database, transport::CURSOR_USAGE_SQL).ok()?;
    let sqlite_limits = CurlLimits { timeout: limits.timeout.min(Duration::from_secs(8)), max_body: 64 * 1024 };
    let output = transport::run_command(program, &args, None, sqlite_limits, cancel).await.ok()?;
    let text = String::from_utf8(output).ok()?;
    quota_in_text(QuotaProvider::Cursor, text.trim())
}
