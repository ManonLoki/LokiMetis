//! 读取 Cursor 官方客户端的套餐和当前周期用量；只组合协议字段，周期解释交给 core。

use std::path::Path;

use loki_metris_core::QuotaFailure;
use serde_json::Value;
use tokio::sync::watch;

use super::{credentials::{self, Secret}, fetch, transport::{CurlLimits, HttpRequest, HttpResponse}};

/// 使用官方 DashboardService；套餐入口不支持 RPC 时回退，已识别新套餐后不混入旧摘要。
pub(super) async fn fetch(token: &Secret, curl: Option<&Path>, limits: CurlLimits, cancel: &mut watch::Receiver<bool>) -> Result<HttpResponse, QuotaFailure> {
    let info = fetch::exchange(curl, request(token, "GetPlanInfo")?, limits, cancel).await?;
    if matches!(info.status, 404 | 405 | 501) {
        return fetch::exchange(curl, fetch::cursor_request(token)?, limits, cancel).await;
    }
    if info.status != 200 { return Ok(info); }
    let info: Value = serde_json::from_str(&info.body).map_err(|_| QuotaFailure::Malformed)?;
    let info = info.get("planInfo").filter(|info| info.is_object()).ok_or(QuotaFailure::Malformed)?;
    let usage = fetch::exchange(curl, request(token, "GetCurrentPeriodUsage")?, limits, cancel).await?;
    if usage.status != 200 { return Ok(usage); }
    let mut body: Value = serde_json::from_str(&usage.body).map_err(|_| QuotaFailure::Malformed)?;
    body.as_object_mut().ok_or(QuotaFailure::Malformed)?.insert("planInfo".to_owned(), info.clone());
    Ok(HttpResponse { status: 200, body: body.to_string() })
}

/// 固定的 Connect unary JSON 请求；JWT 仅放标准输入的 Bearer 头，已有会话先规范化再取令牌。
fn request(token: &Secret, method: &str) -> Result<HttpRequest, QuotaFailure> {
    let session = credentials::cursor_session_cookie(token)?;
    let (_, bearer) = session.expose().split_once("%3A%3A").ok_or(QuotaFailure::Malformed)?;
    let url = match method {
        "GetPlanInfo" => "https://api2.cursor.sh/aiserver.v1.DashboardService/GetPlanInfo",
        "GetCurrentPeriodUsage" => "https://api2.cursor.sh/aiserver.v1.DashboardService/GetCurrentPeriodUsage",
        _ => return Err(QuotaFailure::Transport),
    };
    Ok(HttpRequest {
        method: "POST", url: url.to_owned(),
        headers: vec![
            ("Authorization", format!("Bearer {bearer}")),
            ("Content-Type", "application/json".to_owned()),
            ("Connect-Protocol-Version", "1".to_owned()),
        ],
        body: Some("{}".to_owned()),
    })
}
