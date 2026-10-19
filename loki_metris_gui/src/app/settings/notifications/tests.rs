//! 使用真实布局、点击与键盘验证折叠，不启动原生服务或访问用户文件。
use super::*;
use crate::native::notifications::{Error, State};
use gpui_kit::{
    AppContext, Entity, Render, Subscription, TestAppContext, Window, test::TestWindowExt,
};
use loki_metris_core::usage_milestones::NotificationPermission;

/// 默认关闭时恢复单行卡片；详情点击、键盘和页面往返只改变会话展示。
#[gpui_kit::test]
fn notification_details_are_compact_controlled_and_keyboard_accessible(cx: &mut TestAppContext) {
    let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
    cx.update_window(handle, |_, window, cx| {
        shell.update(cx, |this, cx| this.navigate(crate::app::Page::Settings, cx));
        window.render_frame(cx);
        assert!(
            window.find("notification-header").bounds().size.height <= px(26.),
            "标题与开关保持一行"
        );
        assert_eq!(
            window.find("notification-details-toggle").expanded(),
            Some(false)
        );
        assert_eq!(
            window.find("notification-details-toggle").label(),
            Some("Show system notification details")
        );
        assert!(window.try_find("notification-details").is_none());
        assert!(window.try_find("notification-retry").is_none());
        window.click("notification-switch", cx);
        assert!(
            !shell.read(cx).notification_details_expanded,
            "开关不能代理详情点击"
        );
        window.click("notification-details-toggle", cx);
        assert!(shell.read(cx).notification_details_expanded);
        assert_eq!(
            window.find("notification-details-toggle").focused(),
            Some(true)
        );
        assert_eq!(
            window.find("notification-details").role(),
            Some(Role::Region)
        );
        assert_eq!(
            window.find("notification-details-toggle").expanded(),
            Some(true)
        );
    })
    .unwrap();
    // 真实 Button 按下/抬起路径必须各切换一次，不能重复绑定键盘事件。
    cx.update_window(handle, |_, window, cx| {
        window.press("enter", cx);
        assert!(!shell.read(cx).notification_details_expanded);
        window.press("space", cx);
        assert!(shell.read(cx).notification_details_expanded);
        shell.update(cx, |this, cx| {
            this.navigate(crate::app::Page::Dashboard, cx);
            this.locale = "zh-CN";
            this.navigate(crate::app::Page::Settings, cx);
            cx.notify();
        });
        window.render_frame(cx);
        assert_eq!(
            window.find("notification-details-toggle").label(),
            Some("收起系统通知详情")
        );
        assert_eq!(
            window.find("notification-details").label(),
            Some("系统通知详情")
        );
        window.click("notification-details-toggle", cx);
        assert!(window.try_find("notification-details").is_none());
        assert!(window.find("notification-header").bounds().size.height <= px(26.));
        assert!(!shell.read(cx).preferences.system_notification);
    })
    .unwrap();
}

/// 隔离状态夹具复用生产卡片和 Shell 回调，不安装托盘或通知 owner。
struct CardProbe {
    /// 根视图。
    shell: Entity<Shell>,
    /// 注入的通知状态。
    state: State,
    /// 根视图变化订阅。
    _subscription: Subscription,
}
impl Render for CardProbe {
    /// 把通知卡片放进一个空容器渲染，供测试读取它的真实布局与交互。
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().flex_col().items_start().child(
            self.shell.update(cx, |shell, cx| {
                shell.settings_notification_card(&self.state, cx)
            }),
        )
    }
}

/// 折叠不能隐藏未知结果、存储故障或外部撤权，恢复入口继续可见。
#[gpui_kit::test]
fn notification_collapsed_card_keeps_actionable_failures_visible(cx: &mut TestAppContext) {
    let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
    let probe = cx
        .update_window(handle, |_, window, cx| {
            window.replace_root(cx, |_, cx| CardProbe {
                _subscription: cx.observe(&shell, |_, _, cx| cx.notify()),
                shell: shell.clone(),
                state: State::default(),
            })
        })
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.find("notification-card").bounds().size.height <= px(72.),
            "独立正常关闭卡片恢复单行"
        );
    })
    .unwrap();
    for case in 0..5 {
        let mut state = State {
            enabled: true,
            host_available: true,
            permission: NotificationPermission::Allowed,
            ..State::default()
        };
        let now = "2026-10-11T04:00:00Z"
            .parse::<jiff::Timestamp>()
            .unwrap()
            .as_millisecond();
        state.progress.snapshot = Some(loki_metris_core::TodayUsageSnapshot {
            day: loki_metris_core::UsageDay::capture(now, &jiff::tz::TimeZone::UTC).unwrap(),
            source: "all".into(),
            total_tokens: 350_000_000,
            observed_ms: now,
            availability: TodayAvailability::Available,
            completeness: Completeness::Complete,
            confidence: Confidence::Exact,
        });
        let (warning, retry_label) = match case {
            0 => {
                state.progress.unknown_id = Some("isolated-unknown".into());
                (
                    "notification-feedback",
                    "Retry unknown requests (may duplicate)",
                )
            }
            1 => {
                state.permission = NotificationPermission::Denied;
                (
                    "notification.denied_intent_saved",
                    "Refresh permission and retry",
                )
            }
            2 => {
                state.progress.error = Some("notification_history_write_failed");
                state.busy = true;
                ("notification-feedback", "Refresh permission and retry")
            }
            3 => {
                state.error = Some(Error::Save);
                state.enabled = false;
                (
                    "settings.system_notification_error",
                    "Refresh permission and retry",
                )
            }
            _ => {
                state.progress.issue = Some(MilestoneIssue::RetryExhausted);
                ("notification-feedback", "Refresh permission and retry")
            }
        };
        probe.update(cx, |probe, cx| {
            probe.state = state;
            cx.notify();
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("notification-details").is_none());
            assert!(
                window.try_find(warning).is_some(),
                "故障必须留在详情外：{case}"
            );
            if matches!(case, 0 | 2 | 4) {
                let key = match case {
                    0 => "notification.unknown",
                    2 => "notification.storage",
                    _ => "notification.rejected",
                };
                let expected = rust_i18n::t!(key, locale = "en-US").to_string();
                assert_eq!(
                    window.find("notification-feedback").label(),
                    Some(expected.as_str())
                );
            }
            assert_eq!(window.find("notification-retry").label(), Some(retry_label));
            assert!(window.try_find("notification-settings").is_some());
        })
        .unwrap();
    }
}
