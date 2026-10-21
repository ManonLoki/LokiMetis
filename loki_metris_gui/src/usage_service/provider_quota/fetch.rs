//! 把本机登录态换成带官方客户端标识的用量请求，再用 core 解释正文。

use std::path::Path;

use loki_metris_core::{QuotaFailure, QuotaProvider, parse_provider_quota};
use tokio::sync::watch;

use super::{
    QuotaCardPhase,
    credentials::{self, CodexLogin, GrokLogin, HomeLayout, Secret, WorkBuddyLogin},
    transport::{self, CurlLimits, HttpRequest, HttpResponse},
};

/// 账单查询的套餐结束时间范围。
pub(crate) struct TimeRange {
    /// 范围起点，`YYYY-MM-DD HH:MM:SS`。
    pub begin: String,
    /// 范围终点。
    pub end: String,
}

/// 读取一个提供商。缺少 curl 或 sqlite3 时返回传输失败，不改用其它程序。
pub(crate) async fn fetch_provider(
    provider: QuotaProvider,
    layout: &HomeLayout,
    curl: Option<&Path>,
    sqlite: Option<&Path>,
    limits: CurlLimits,
    now_unix: i64,
    range: &TimeRange,
    cancel: &mut watch::Receiver<bool>,
) -> QuotaCardPhase {
    if *cancel.borrow() {
        return QuotaCardPhase::Failed(QuotaFailure::Transport);
    }
    if let Some(quota) = super::local::read_local_quota(provider, layout, sqlite, limits, cancel).await {
        return QuotaCardPhase::Ready(quota);
    }
    let result = match provider {
        QuotaProvider::Codex => fetch_codex(layout, curl, limits, cancel).await,
        // Claude 没有远程读取：statusLine 记录缺失或已过期就视为本机没有额度来源。
        QuotaProvider::Claude => Err(QuotaFailure::MissingCredentials),
        QuotaProvider::Cursor => fetch_cursor(layout, curl, sqlite, limits, cancel).await,
        QuotaProvider::Grok => fetch_grok(layout, curl, limits, now_unix, cancel).await,
        QuotaProvider::WorkBuddy => fetch_workbuddy(layout, curl, limits, range, cancel).await,
    };
    match result {
        Ok(response) => phase_from_http(provider, response),
        Err(failure) => QuotaCardPhase::Failed(failure),
    }
}

/// 用本地时区生成 WorkBuddy 套餐查询范围。
pub(crate) fn local_package_range() -> TimeRange {
    let now = jiff::Zoned::now();
    let end = now.checked_add(jiff::Span::new().days(365 * 101)).unwrap_or_else(|_| now.clone());
    TimeRange { begin: format_zoned(&now), end: format_zoned(&end) }
}

/// 将认证失败、传输失败及成功正文映射为额度卡阶段，窗口解释统一交给 core。
pub(crate) fn phase_from_http(provider: QuotaProvider, response: HttpResponse) -> QuotaCardPhase {
    if response.status == 401 || response.status == 403 {
        return QuotaCardPhase::Failed(QuotaFailure::Unauthorized);
    }
    if response.status != 200 {
        return QuotaCardPhase::Failed(QuotaFailure::Transport);
    }
    match parse_provider_quota(provider, &response.body) {
        Ok(quota) if quota.has_usage_window() => QuotaCardPhase::Ready(quota),
        Ok(quota) => QuotaCardPhase::NoWindow { plan: quota.plan },
        Err(failure) => QuotaCardPhase::Failed(failure),
    }
}

/// 只读 Codex 登录材料后访问固定用量入口，失败不改写本机登录态。
async fn fetch_codex(layout: &HomeLayout, curl: Option<&Path>, limits: CurlLimits, cancel: &mut watch::Receiver<bool>) -> Result<HttpResponse, QuotaFailure> {
    let login = credentials::read_codex(layout)?;
    exchange(curl, codex_request(&login)?, limits, cancel).await
}

/// 按当前时间选择 Grok 会话材料，再通过受限传输读取账单额度。
async fn fetch_grok(layout: &HomeLayout, curl: Option<&Path>, limits: CurlLimits, now_unix: i64, cancel: &mut watch::Receiver<bool>) -> Result<HttpResponse, QuotaFailure> {
    let login = credentials::read_grok(layout, now_unix)?;
    exchange(curl, grok_request(&login)?, limits, cancel).await
}

/// 以有界只读查询取得 Cursor 令牌，再访问固定额度入口。
async fn fetch_cursor(layout: &HomeLayout, curl: Option<&Path>, sqlite: Option<&Path>, limits: CurlLimits, cancel: &mut watch::Receiver<bool>) -> Result<HttpResponse, QuotaFailure> {
    let database = credentials::cursor_database(layout);
    credentials::ensure_regular_file(&database)?;
    let program = sqlite.ok_or(QuotaFailure::Transport)?;
    let args = transport::cursor_command_args(&database)?;
    let sqlite_limits = CurlLimits { timeout: DurationCap::min(limits.timeout, std::time::Duration::from_secs(8)), max_body: 8 * 1024 };
    let output = transport::run_command(program, &args, None, sqlite_limits, cancel).await?;
    let text = String::from_utf8(output).map_err(|_| QuotaFailure::Malformed)?;
    let token = credentials::cursor_token_from_output(&text)?;
    exchange(curl, cursor_request(&token)?, limits, cancel).await
}

/// 读取可用明文登录材料并发起对应站点的受限账单查询。
async fn fetch_workbuddy(layout: &HomeLayout, curl: Option<&Path>, limits: CurlLimits, range: &TimeRange, cancel: &mut watch::Receiver<bool>) -> Result<HttpResponse, QuotaFailure> {
    let login = credentials::read_workbuddy(layout)?;
    exchange(curl, workbuddy_request(&login, range)?, limits, cancel).await
}

/// 通过受限 curl 配置执行单次请求，再拆分 HTTP 状态与正文。
async fn exchange(curl: Option<&Path>, request: HttpRequest, limits: CurlLimits, cancel: &mut watch::Receiver<bool>) -> Result<HttpResponse, QuotaFailure> {
    let program = curl.ok_or(QuotaFailure::Transport)?;
    let exchange = transport::build_curl_exchange(&request, &limits)?;
    let bytes = transport::run_command(program, &exchange.args, Some(exchange.config), limits, cancel).await?;
    transport::split_http(&bytes)
}

/// 构造固定 Codex 用量请求，仅在请求头中携带令牌与账户身份。
fn codex_request(login: &CodexLogin) -> Result<HttpRequest, QuotaFailure> {
    Ok(HttpRequest {
        method: "GET",
        url: "https://chatgpt.com/backend-api/wham/usage".to_owned(),
        headers: vec![
            ("Authorization", format!("Bearer {}", login.token.expose())),
            ("ChatGPT-Account-Id", login.account_id.clone()),
            ("User-Agent", format!("codex_cli_rs/0.98.0 ({}; {})", std::env::consts::OS, std::env::consts::ARCH)),
            ("originator", "codex_cli_rs".to_owned()),
            ("Accept", "application/json".to_owned()),
        ],
        body: None,
    })
}

/// 构造固定 Cursor 用量请求，将令牌编码为会话 Cookie。
fn cursor_request(token: &Secret) -> Result<HttpRequest, QuotaFailure> {
    Ok(HttpRequest {
        method: "GET",
        url: "https://cursor.com/api/usage-summary".to_owned(),
        headers: vec![
            ("Cookie", format!("WorkosCursorSessionToken={}", token.expose().replace("::", "%3A%3A"))),
            ("User-Agent", cursor_user_agent().to_owned()),
            ("Accept", "application/json".to_owned()),
            ("Origin", "https://cursor.com".to_owned()),
            ("Referer", "https://cursor.com/dashboard/usage".to_owned()),
        ],
        body: None,
    })
}

/// 构造固定 Grok 额度请求并附加官方客户端身份头。
fn grok_request(login: &GrokLogin) -> Result<HttpRequest, QuotaFailure> {
    Ok(HttpRequest {
        method: "GET",
        url: "https://cli-chat-proxy.grok.com/v1/billing?format=credits".to_owned(),
        headers: vec![
            ("Authorization", format!("Bearer {}", login.token.expose())),
            ("User-Agent", "grok-cli/1.0.62".to_owned()),
            ("x-xai-token-auth", "xai-grok-cli".to_owned()),
            ("x-grok-client-surface", "grok-build".to_owned()),
            ("x-grok-client-version", "1.0.62".to_owned()),
            ("Accept", "application/json".to_owned()),
        ],
        body: None,
    })
}

/// 按登录域和企业身份选择固定账单入口，生成所需身份头与查询正文。
fn workbuddy_request(login: &WorkBuddyLogin, range: &TimeRange) -> Result<HttpRequest, QuotaFailure> {
    let global = is_global_domain(&login.domain);
    let enterprise = !global && login.enterprise_id.is_some();
    let origin = if global { "https://www.workbuddy.ai" } else { "https://www.codebuddy.cn" };
    let (url, body) = if enterprise {
        ("https://www.codebuddy.cn/v2/billing/meter/get-enterprise-user-usage".to_owned(), "{}".to_owned())
    } else {
        (format!("{origin}/v2/billing/meter/get-user-resource"), personal_body(range))
    };
    let mut headers = vec![
        ("Authorization", format!("Bearer {}", login.token.expose())),
        ("Accept", "application/json".to_owned()),
        ("Content-Type", "application/json".to_owned()),
        ("User-Agent", "WorkBuddy/5.8.4".to_owned()),
        ("X-Product", "SaaS".to_owned()),
        ("X-IDE-Type", "WorkBuddy".to_owned()),
        ("X-IDE-Name", "WorkBuddy".to_owned()),
        ("X-IDE-Version", "5.8.4".to_owned()),
        ("X-Requested-With", "XMLHttpRequest".to_owned()),
        ("Origin", origin.to_owned()),
        ("Referer", format!("{origin}/")),
    ];
    if let Some(user_id) = &login.user_id {
        headers.push(("X-User-Id", user_id.clone()));
    }
    if let Some(enterprise_id) = &login.enterprise_id {
        headers.push(("X-Enterprise-Id", enterprise_id.clone()));
        headers.push(("X-Tenant-Id", enterprise_id.clone()));
    }
    if !login.domain.is_empty() {
        headers.push(("X-Domain", login.domain.clone()));
    }
    Ok(HttpRequest { method: "POST", url, headers, body: Some(body) })
}

/// 生成个人套餐的固定分页与时间范围正文，不接受任意查询字段。
fn personal_body(range: &TimeRange) -> String {
    serde_json::json!({
        "PageNumber": 1,
        "PageSize": 100,
        "ProductCode": "p_tcaca",
        "Status": [0, 3],
        "PackageEndTimeRangeBegin": range.begin,
        "PackageEndTimeRangeEnd": range.end,
    }).to_string()
}

/// 仅将 WorkBuddy 国际站域名及其子域识别为国际登录域。
fn is_global_domain(domain: &str) -> bool {
    let domain = domain.trim().to_ascii_lowercase();
    domain == "workbuddy.ai" || domain.ends_with(".workbuddy.ai")
}

/// 按目标平台返回 Cursor 用量请求使用的固定浏览器标识。
fn cursor_user_agent() -> &'static str {
    #[cfg(target_os = "windows")]
    { "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36" }
    #[cfg(target_os = "macos")]
    { "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36" }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    { "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36" }
}

/// 将带时区时间格式化为账单接口要求的本地日期时间字符串。
fn format_zoned(zoned: &jiff::Zoned) -> String {
    let date = zoned.date();
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", date.year(), date.month(), date.day(), zoned.hour(), zoned.minute(), zoned.second())
}

/// 为 Cursor 登录库查询提供独立的超时上限选择。
struct DurationCap;

impl DurationCap {
    /// 选择调用方预算与 Cursor 查询上限中较短的时长。
    fn min(left: std::time::Duration, right: std::time::Duration) -> std::time::Duration {
        left.min(right)
    }
}
