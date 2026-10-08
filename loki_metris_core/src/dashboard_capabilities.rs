//! 从统一公开 AI 目录派生看板能力，不另行维护客户端名单。

use crate::{SourceClientKind, public_ai_capabilities, public_dashboard_clients};

/// 返回看板支持的物理扫描客户端；WorkBuddy 只提供独立只读统计，不占扫描槽。
pub fn physical_scan_clients() -> Vec<SourceClientKind> {
    public_dashboard_clients()
        .filter(|client| *client != SourceClientKind::WorkBuddy)
        .collect()
}

/// 返回看板能够映射的公开能力名称，无法映射的类型自动忽略。
pub fn dashboard_capability_names() -> Vec<&'static str> {
    public_ai_capabilities()
        .iter()
        .filter(|capability| capability.dashboard_client.is_some())
        .map(|capability| capability.name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Confidence, CoverageReport, CoverageState, EnabledAgents, LocalIndexState,
        LocalUsageWindow, ProviderKind, SourceProvenance, TimeStandard, TokenUsage, UsageCall,
        WindowBoundaries, belongs_to_window, build_local_windows_with_standard,
        canonicalize_usage_calls, resolve_workbuddy_stats_enabled,
    };
    use jiff::tz::TimeZone;

    /// 物理客户端枚举必须恰好三项，且不得出现 Cursor 或 WorkBuddy 槽。
    #[test]
    fn physical_scan_clients_are_exactly_three_named_agents() {
        assert_eq!(physical_scan_clients().len(), 3);
        assert_eq!(
            physical_scan_clients(),
            vec![
                SourceClientKind::Codex,
                SourceClientKind::ClaudeCode,
                SourceClientKind::GrokBuildCli,
            ]
        );
        assert!(!physical_scan_clients().contains(&SourceClientKind::WorkBuddy));
    }

    /// EnabledAgents 不含 WorkBuddy 槽；WorkBuddy 只能走独立开关。
    #[test]
    fn enabled_agents_exclude_workbuddy_and_workbuddy_is_independent() {
        let enabled = EnabledAgents::empty()
            .with(SourceClientKind::Codex, true)
            .with(SourceClientKind::WorkBuddy, true);
        assert!(enabled.contains(SourceClientKind::Codex));
        assert!(!enabled.contains(SourceClientKind::WorkBuddy));
        assert_eq!(enabled.labels(), vec!["codex"]);
        assert!(!resolve_workbuddy_stats_enabled(None));
        assert!(resolve_workbuddy_stats_enabled(Some(true)));
        assert!(!resolve_workbuddy_stats_enabled(Some(false)));
    }

    /// 可见能力列表必须与源项目指定集合完全一致。
    #[test]
    fn dashboard_capability_set_matches_source_four() {
        let names = dashboard_capability_names();
        assert_eq!(names.len(), 4);
        assert_eq!(names, vec!["Codex", "Claude Code", "Grok", "WorkBuddy"]);
        assert_eq!(physical_scan_clients().len(), 3);
        assert!(!names.contains(&"Cursor"));
    }

    /// 六个日历窗口键均被接受，当地/UTC 标准会改变日界与窗口归属。
    #[test]
    fn overview_windows_accept_six_keys_and_time_standard_shifts_day_boundary() {
        assert_eq!(
            LocalUsageWindow::OVERVIEW_WINDOWS,
            [
                LocalUsageWindow::Today,
                LocalUsageWindow::Yesterday,
                LocalUsageWindow::ThisWeek,
                LocalUsageWindow::LastWeek,
                LocalUsageWindow::ThisMonth,
                LocalUsageWindow::LastMonth,
            ]
        );

        let shanghai = TimeZone::get("Asia/Shanghai").expect("IANA zone exists");
        // 观测 2026-08-19 10:00 +08（UTC 02:00）；调用发生在当天 00:30 +08（UTC 前一日 16:30）。
        let observed = 1_787_104_800_000_i64;
        let occurred_at = 1_787_070_600_000_i64;
        let local_today =
            WindowBoundaries::for_standard(observed, &TimeStandard::Local, &shanghai).today;
        let utc_today =
            WindowBoundaries::for_standard(observed, &TimeStandard::utc(), &shanghai).today;
        assert_ne!(
            local_today, utc_today,
            "local and UTC day starts must differ when the device offset is not zero"
        );

        let call = UsageCall {
            logical_call_id: "boundary".to_owned(),
            occurred_at_epoch_ms: occurred_at,
            model: Some("model-a".to_owned()),
            reasoning_effort: None,
            project_key: None,
            project_label: None,
            thread_key: "thread-boundary".to_owned(),
            thread_label: None,
            usage: TokenUsage::new(6, 0, None, 4, 0, Some(10)).expect("valid usage"),
            adapter_consistency_key: None,
            confidence: Confidence::Exact,
            provenance: vec![SourceProvenance {
                source_id: "source-boundary".to_owned(),
                root_id: "root-boundary".to_owned(),
                relative_label: "session.jsonl".to_owned(),
                archived: false,
            }],
        };
        let canonical = canonicalize_usage_calls(vec![call]);
        let coverage = CoverageReport {
            state: CoverageState::Complete,
            roots_scanned: 1,
            roots_discovered: 1,
            permission_denied_count: 0,
            skipped_count: 0,
            warning_count: 0,
        };
        let local_windows = build_local_windows_with_standard(
            &canonical,
            &coverage,
            LocalIndexState::Ready,
            observed,
            ProviderKind::RolloutJsonl,
            None,
            TimeStandard::Local,
            &shanghai,
        )
        .expect("local windows");
        let utc_windows = build_local_windows_with_standard(
            &canonical,
            &coverage,
            LocalIndexState::Ready,
            observed,
            ProviderKind::RolloutJsonl,
            None,
            TimeStandard::utc(),
            &shanghai,
        )
        .expect("utc windows");

        let local_today_usage = local_windows
            .windows
            .iter()
            .find(|item| item.window == LocalUsageWindow::Today)
            .expect("today");
        let utc_today_usage = utc_windows
            .windows
            .iter()
            .find(|item| item.window == LocalUsageWindow::Today)
            .expect("today");
        assert_eq!(local_windows.windows.len(), 6);
        assert_eq!(utc_windows.windows.len(), 6);
        assert_eq!(local_today_usage.fact.value.call_count, 1);
        assert_eq!(utc_today_usage.fact.value.call_count, 0);
        assert!(
            belongs_to_window(
                occurred_at,
                LocalUsageWindow::Today,
                observed,
                &TimeStandard::Local,
                &shanghai,
            )
            .expect("local membership")
        );
        assert!(
            !belongs_to_window(
                occurred_at,
                LocalUsageWindow::Today,
                observed,
                &TimeStandard::utc(),
                &shanghai,
            )
            .expect("utc membership")
        );
    }
}
