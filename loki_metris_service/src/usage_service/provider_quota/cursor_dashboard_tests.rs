//! 新联名套餐的协议回归；全部响应和登录态均为隔离合成夹具。

use super::*;

/// 明确周套餐保留真实百分数和毫秒重置时间，只发两次固定 RPC，不再请求旧月账单。
#[cfg(unix)]
#[test]
fn cursor_dashboard_reads_shared_weekly_pool_without_legacy_usage() {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let root = tempfile::tempdir().unwrap();
        let program = tests::install_fixture(root.path());
        tests::write(&root.path().join("mode.txt"), "status");
        tests::write(&root.path().join("plan-info.txt"), r#"{"planInfo":{"planName":"Plus","includedUsagePeriod":"INCLUDED_USAGE_PERIOD_WEEKLY","includedUsageResetsAt":"1793668310736"},"isMerged":true}
__LM_HTTP_200"#);
        tests::write(&root.path().join("period-usage.txt"), r#"{"billingCycleStart":"1793063510736","billingCycleEnd":"1793668310736","planUsage":{"totalPercentUsed":68.84615384615384},"enabled":true}
__LM_HTTP_200"#);
        let token = credentials::Secret::new(cursor_session_tests::jwt(serde_json::json!({"sub":"user_fixture"}))).unwrap();
        let (_sender, mut cancel) = tokio::sync::watch::channel(false);
        let response = cursor_dashboard::fetch(&token, Some(&program), transport::CurlLimits::production(), &mut cancel).await.unwrap();
        let QuotaCardPhase::Ready(quota) = fetch::phase_from_http(QuotaProvider::Cursor, response) else { panic!("shared weekly quota must be ready") };
        assert_eq!(quota.plan.as_deref(), Some("Plus"));
        assert_eq!(quota.weekly.unwrap().used_tenths, 688);
        assert_eq!(quota.weekly.unwrap().reset_at_unix, Some(1_793_668_310));
        assert!(quota.monthly.is_none());
        let requests = std::fs::read_to_string(root.path().join("requests.txt")).unwrap();
        assert!(requests.contains("DashboardService/GetPlanInfo"));
        assert!(requests.contains("DashboardService/GetCurrentPeriodUsage"));
        assert!(requests.contains("Authorization: Bearer "));
        assert!(requests.contains("Connect-Protocol-Version: 1"));
        assert!(!requests.contains("usage-summary") && !requests.contains("WorkosCursorSessionToken"));
        assert!(!std::fs::read_to_string(root.path().join("argv.txt")).unwrap().contains(token.expose()));
    });
}

/// 新接口拒绝认证或返回坏正文时不回退旧月额度，避免掩盖真实失败。
#[cfg(unix)]
#[test]
fn cursor_dashboard_does_not_mask_auth_or_protocol_failures() {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        for (body, expected) in [("{}\n__LM_HTTP_401", QuotaFailure::Unauthorized), ("not-json\n__LM_HTTP_200", QuotaFailure::Malformed), ("{}\n__LM_HTTP_200", QuotaFailure::Malformed)] {
            let root = tempfile::tempdir().unwrap();
            let program = tests::install_fixture(root.path());
            tests::write(&root.path().join("mode.txt"), "status");
            tests::write(&root.path().join("plan-info.txt"), body);
            let token = credentials::Secret::new(cursor_session_tests::jwt(serde_json::json!({"sub":"user_fixture"}))).unwrap();
            let (_sender, mut cancel) = tokio::sync::watch::channel(false);
            let phase = match cursor_dashboard::fetch(&token, Some(&program), transport::CurlLimits::production(), &mut cancel).await {
                Ok(response) => fetch::phase_from_http(QuotaProvider::Cursor, response),
                Err(failure) => QuotaCardPhase::Failed(failure),
            };
            assert_eq!(phase, QuotaCardPhase::Failed(expected));
            let requests = std::fs::read_to_string(root.path().join("requests.txt")).unwrap();
            assert!(!requests.contains("usage-summary") && !requests.contains("GetCurrentPeriodUsage"));
        }
    });
}

/// 周套餐已经识别后，即使用量接口不支持也不回退可能属于旧月套餐的百分数。
#[cfg(unix)]
#[test]
fn cursor_dashboard_weekly_plan_does_not_fall_back_after_usage_failure() {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let root = tempfile::tempdir().unwrap();
        let program = tests::install_fixture(root.path());
        tests::write(&root.path().join("mode.txt"), "status");
        tests::write(&root.path().join("plan-info.txt"), r#"{"planInfo":{"planName":"Plus","includedUsagePeriod":2,"includedUsageResetsAt":"1793668310736"},"isMerged":true}
__LM_HTTP_200"#);
        let token = credentials::Secret::new(cursor_session_tests::jwt(serde_json::json!({"sub":"user_fixture"}))).unwrap();
        for status in [401, 403, 404, 405, 501] {
            tests::write(&root.path().join("period-usage.txt"), &format!("{{}}\n__LM_HTTP_{status}"));
            let (_sender, mut cancel) = tokio::sync::watch::channel(false);
            let response = cursor_dashboard::fetch(&token, Some(&program), transport::CurlLimits::production(), &mut cancel).await.unwrap();
            assert_eq!(response.status, status);
        }
        let requests = std::fs::read_to_string(root.path().join("requests.txt")).unwrap();
        assert!(!requests.contains("usage-summary"));
    });
}
