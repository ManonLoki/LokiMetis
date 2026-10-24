//! Cursor 账户读取驱动：复用受限凭据和 HTTPS 传输，仅在内存缓存非敏感统计。

#[cfg(test)]
mod tests;

use std::{path::Path, sync::{Arc, Mutex}, time::Duration};
use jiff::tz::TimeZone;
use loki_metris_core::{AiTool, LocalUsageWindow, QuotaFailure, ScanCancellation, ScanIntervalMinutes,
    cursor_usage::{CursorUsageError as Error, CursorUsagePages, CursorUsageRange, CursorUsageSummary, PAGE_SIZE}};
use tokio::sync::watch;
use super::{ServiceContext, provider_quota::{credentials::{self, HomeLayout}, fetch, transport::{self, CurlLimits}}};

/// 一个查询窗口的最近终态；配置控制采集间隔，页面重读复用此终态。
struct Entry {
    window: LocalUsageWindow,
    day: String,
    zone: TimeZone,
    start_ms: i64,
    /// 写入时按当时配置冻结的到期时刻；保存新配置不提前打断已经开始的等待。
    expires: std::time::Instant,
    result: Result<Arc<CursorUsageSummary>, Error>,
}

/// 单一账户的六个内存窗口；账户切换立即清除旧账户所有缓存。
#[derive(Default)]
pub(super) struct Reader {
    cache: Mutex<(Option<String>, Vec<Entry>)>,
}

/// 查询邮箱和后台 owner 共同拥有读取，退出或换页都会合作回收子进程。
pub(super) async fn load(context: Arc<ServiceContext>, window: LocalUsageWindow,
    cancellation: ScanCancellation) -> Result<Arc<CursorUsageSummary>, Error>
{
    let settings = context.agents.snapshot().await.map_err(|_| Error::Transport)?;
    if !settings.enabled_ai_tools.contains(&AiTool::Cursor) { return Err(Error::Credentials) }
    let interval = context.settings.scan_interval().await.map_err(|_| Error::Transport)?;
    // 常规服务单元测试与 GUI 测试（test-support）没有读取真实家目录的隐式入口，驱动测试显式注入隔离布局。
    if cfg!(any(test, feature = "test-support")) { return Err(Error::Credentials) }
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let state = context.clone();
    context.background.spawn("cursor-token-usage", move |mut shutdown| async move {
        let (cancel, mut watch) = watch::channel(false);
        let work = async {
            let home = loki_metris_host::local_index::current_user_home().ok_or(Error::Credentials)?;
            let layout = HomeLayout::from_process(home);
            let curl = transport::find_program(if cfg!(windows) { "curl.exe" } else { "curl" });
            let sqlite = transport::find_program(if cfg!(windows) { "sqlite3.exe" } else { "sqlite3" });
            state.cursor_usage.read(&layout, curl.as_deref(), sqlite.as_deref(), window, interval,
                super::current_epoch_ms(), &TimeZone::system(), &mut watch).await
        };
        tokio::pin!(work);
        let cancelled = async {
            while !cancellation.is_cancelled() { tokio::time::sleep(Duration::from_millis(25)).await; }
        };
        let result = tokio::select! {
            result = &mut work => result,
            _ = shutdown.cancelled() => { let _ = cancel.send(true); let _ = work.await; Err(Error::Cancelled) },
            _ = cancelled => { let _ = cancel.send(true); let _ = work.await; Err(Error::Cancelled) },
            _ = tokio::time::sleep(Duration::from_secs(60)) => { let _ = cancel.send(true); let _ = work.await; Err(Error::Transport) },
        };
        let _ = sender.send(result);
    }).map_err(|_| Error::Transport)?;
    receiver.await.map_err(|_| Error::Transport)?
}

impl Reader {
    /// 账户与日期边界仍实时核对；成功或失败按已冻结的配置间隔复用，不保存原始正文。
    async fn read(&self, layout: &HomeLayout, curl: Option<&Path>, sqlite: Option<&Path>,
        window: LocalUsageWindow, interval: ScanIntervalMinutes,
        now: i64, zone: &TimeZone, cancel: &mut watch::Receiver<bool>)
        -> Result<Arc<CursorUsageSummary>, Error>
    {
        if *cancel.borrow() { return Err(Error::Cancelled) }
        let limits = CurlLimits { timeout: Duration::from_secs(15), max_body: 1024 * 1024 };
        let token = fetch::read_cursor_token(layout, sqlite, limits, cancel).await.map_err(failure)?;
        let session = credentials::cursor_session_cookie(&token).map_err(failure)?;
        let user = session.expose().split_once("%3A%3A").ok_or(Error::Malformed)?.0;
        let account = loki_metris_core::stable_id("cursor-account", user);
        let range = CursorUsageRange::for_window(window, now, zone)?;
        let day = loki_metris_core::civil_date_for_timestamp(now, &loki_metris_core::TimeStandard::Local, zone)
            .ok_or(Error::Malformed)?.to_string();
        {
            let mut cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if cache.0.as_ref() != Some(&account) { *cache = (Some(account.clone()), Vec::new()); }
            if let Some(entry) = cache.1.iter().find(|entry| entry.window == window && entry.day == day
                && &entry.zone == zone && entry.start_ms == range.start_ms
                && std::time::Instant::now() < entry.expires) {
                return entry.result.clone();
            }
        }
        let result = read_pages(&token, curl, range, now, zone, limits, cancel).await.map(Arc::new);
        if !*cancel.borrow() {
            let mut cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if cache.0.as_ref() == Some(&account) {
                cache.1.retain(|entry| entry.window != window);
                cache.1.push(Entry { window, day, zone: zone.clone(), start_ms: range.start_ms,
                    expires: std::time::Instant::now() + interval.duration(), result: result.clone() });
            }
        }
        result
    }
}

/// 顺序执行 core 要求的有限页面，任一失败都不生成部分统计。
async fn read_pages(token: &credentials::Secret, curl: Option<&Path>, range: CursorUsageRange, now: i64,
    zone: &TimeZone, limits: CurlLimits, cancel: &mut watch::Receiver<bool>) -> Result<CursorUsageSummary, Error>
{
    let mut pages = CursorUsagePages::new(range)?;
    while let Some(page) = pages.next_page() {
        if *cancel.borrow() { return Err(Error::Cancelled) }
        let mut request = fetch::cursor_request(token).map_err(failure)?;
        request.method = "POST";
        request.url = "https://cursor.com/api/dashboard/get-filtered-usage-events".to_owned();
        request.headers.push(("Content-Type", "application/json".to_owned()));
        request.body = Some(serde_json::json!({ "page": page, "pageSize": PAGE_SIZE,
            "startDate": range.start_ms.to_string(), "endDate": (range.end_ms - 1).to_string() }).to_string());
        let response = fetch::exchange(curl, request, limits, cancel).await.map_err(failure)?;
        match response.status {
            200 => pages.push(page, &response.body)?,
            401 | 403 => return Err(Error::Unauthorized),
            _ => return Err(Error::Transport),
        }
    }
    if *cancel.borrow() { return Err(Error::Cancelled) }
    pages.finish(now, zone)
}

/// 沿用额度传输的脱敏失败分类，凭据内容从不进入错误。
fn failure(error: QuotaFailure) -> Error {
    match error {
        QuotaFailure::MissingCredentials | QuotaFailure::CredentialLocked => Error::Credentials,
        QuotaFailure::Unauthorized => Error::Unauthorized,
        QuotaFailure::Malformed => Error::Malformed,
        QuotaFailure::Transport => Error::Transport,
    }
}
