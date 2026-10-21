//! 真实渲染树覆盖额度隐藏、提供商顺序及进度条长度与颜色。

use super::*;
use gpui_kit::{Hsla, Window, test::TestWindowExt};
use loki_metris_core::QuotaFailure;

/// 为隔离界面测试构造权威窗口，不读取本机登录或联网。
fn quota(provider: QuotaProvider, remaining: u16, weekly: Option<u16>) -> ProviderQuota {
    ProviderQuota {
        provider,
        plan: Some("plus".to_owned()),
        five_hour: Some(QuotaWindow { kind: QuotaWindowKind::FiveHour, used_tenths: 1000 - remaining, reset_at_unix: None }),
        weekly: weekly.map(|remaining| QuotaWindow { kind: QuotaWindowKind::Weekly, used_tenths: 1000 - remaining, reset_at_unix: None }),
    }
}

/// 检查实际绘制的进度条长度和填充颜色，避免只有文字或样式值正确。
fn assert_bar(window: &Window, id: &str, remaining: u16, color: Hsla) {
    let track = window.find(id.to_owned());
    assert_eq!(track.role(), Some(Role::ProgressIndicator));
    assert!(track.label().unwrap_or_default().contains(&percent_text(remaining)));
    let bounds = track.bounds();
    let fill = window.find(format!("{id}-fill")).bounds();
    assert!((f32::from(fill.size.width) - f32::from(bounds.size.width) * f32::from(remaining) / 1000.).abs() < 1.);
    assert_eq!(fill.size.height, px(8.));
    assert_eq!(fill.origin.x, bounds.origin.x);
    if remaining > 0 {
        assert!(window.painted_quads().iter().any(|quad| {
            quad.bounds == fill.scale(window.scale_factor()) && quad.background.as_solid() == Some(color)
        }), "进度条必须实际绘制语义色 {color:?}");
    }
}

/// 百分比与重置提示跟随注入时钟，保持一位小数和中英文占位替换。
#[test]
fn reset_and_remaining_text_follow_the_injected_clock() {
    let t = |key: &str| match key {
        "monitor.quota.resetDays" => "%{count} 天后重置".to_owned(),
        "monitor.quota.resetHours" => "%{count} 小时后重置".to_owned(),
        "monitor.quota.resetSoon" => "即将重置".to_owned(),
        other => other.to_owned(),
    };
    let window = QuotaWindow { kind: QuotaWindowKind::FiveHour, used_tenths: 275, reset_at_unix: Some(86_400 + 3_600) };
    assert_eq!(window_line("5 小时剩余", window, 0, &t), "5 小时剩余 72.5% 1 天后重置");
    assert_eq!(reset_text(10, 0, &t).as_deref(), Some("即将重置"));
    assert_eq!(reset_text(7_200, 0, &t).as_deref(), Some("2 小时后重置"));
}

/// 所有失败阶段、等待、无目标窗口和只有周额度的提供商都不占位。
#[gpui_kit::test]
fn quota_board_hides_unavailable_and_weekly_only_cards(cx: &mut gpui_kit::TestAppContext) {
    let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
    cx.update_window(handle, |_, window, cx| {
        let mut weekly_only = quota(QuotaProvider::Codex, 500, Some(800));
        weekly_only.five_hour = None;
        let mut phases = vec![QuotaCardPhase::Pending, QuotaCardPhase::NoWindow { plan: Some("pro".to_owned()) }, QuotaCardPhase::Ready(weekly_only)];
        phases.extend([QuotaFailure::MissingCredentials, QuotaFailure::CredentialLocked, QuotaFailure::Unauthorized, QuotaFailure::Transport, QuotaFailure::Malformed].map(QuotaCardPhase::Failed));
        for phase in phases {
            shell.update(cx, |this, cx| {
                this.page = crate::app::Page::Monitor;
                this.monitor.status = None;
                this.monitor.quota.cards[0].phase = phase;
                cx.notify();
            });
            window.render_frame(cx);
            for id in ["monitor-quota", "monitor-quota-codex", "monitor-quota-cursor", "monitor-quota-grok", "monitor-quota-workbuddy", "monitor-quota-claude", "monitor-quota-retry"] {
                assert!(window.try_find(id).is_none(), "不可用时不应挂载 {id}");
            }
        }
    }).unwrap();
}

/// 不依赖Hook状态；三提供商按指定顺序，填充比例及红黄绿在明暗主题均真实绘制。
#[gpui_kit::test]
fn quota_board_renders_ordered_three_color_bars_without_relay_status(cx: &mut gpui_kit::TestAppContext) {
    let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
    cx.update_window(handle, |_, window, cx| {
        shell.update(cx, |this, _| {
            this.page = crate::app::Page::Monitor;
            this.monitor.status = None;
            for (card, remaining) in this.monitor.quota.cards.iter_mut().zip([730, 500, 200]) {
                card.phase = QuotaCardPhase::Ready(quota(card.provider, remaining, Some(960)));
            }
        });
        for mode in [gpui_kit::component::ThemeMode::Light, gpui_kit::component::ThemeMode::Dark] {
            gpui_kit::component::Theme::change(mode, Some(window), cx);
            window.render_frame(cx);
            assert!(window.try_find("monitor-received").is_none());
            let codex = window.find("monitor-quota-codex").bounds();
            let cursor = window.find("monitor-quota-cursor").bounds();
            let grok = window.find("monitor-quota-grok").bounds();
            assert_eq!(codex.origin.y, cursor.origin.y);
            assert!(cursor.origin.x > codex.right());
            assert!(grok.origin.y > codex.origin.y);
            assert_eq!(grok.origin.x, codex.origin.x);
            assert!(window.try_find("monitor-quota-workbuddy").is_none());
            assert!(window.try_find("monitor-quota-claude").is_none());
            assert_bar(window, "monitor-quota-codex-five-hour", 730, cx.theme().success);
            assert_bar(window, "monitor-quota-cursor-five-hour", 500, cx.theme().warning);
            assert_bar(window, "monitor-quota-grok-five-hour", 200, cx.theme().danger);
            assert_bar(window, "monitor-quota-codex-weekly", 960, cx.theme().success);
        }
        let before = window.find("monitor-quota-codex").label().map(str::to_owned);
        window.click("monitor-quota-codex", cx);
        window.click("monitor-quota-retry", cx);
        window.render_frame(cx);
        assert_eq!(window.find("monitor-quota-codex").label().map(str::to_owned), before);
    }).unwrap();
}

/// 无周窗口时只显示短窗；已耗尽额度仍显示0%，满额显示100%，失败后撤下旧卡。
#[gpui_kit::test]
fn quota_board_keeps_zero_balance_and_omits_missing_weekly_window(cx: &mut gpui_kit::TestAppContext) {
    let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
    cx.update_window(handle, |_, window, cx| {
        for remaining in [0, 199, 201, 499, 501, 1000] {
            shell.update(cx, |this, cx| {
                this.page = crate::app::Page::Monitor;
                this.monitor.quota.cards[0].phase = QuotaCardPhase::Ready(quota(QuotaProvider::Codex, remaining, None));
                this.monitor.quota.cards[1].phase = QuotaCardPhase::NoWindow { plan: None };
                cx.notify();
            });
            window.render_frame(cx);
            assert!(window.find("monitor-quota-codex").label().unwrap_or_default().contains(&percent_text(remaining)));
            assert!(window.try_find("monitor-quota-codex-weekly").is_none());
            assert!(window.try_find("monitor-quota-cursor").is_none());
            let color = match remaining { 0..=200 => cx.theme().danger, 201..=500 => cx.theme().warning, _ => cx.theme().success };
            assert_bar(window, "monitor-quota-codex-five-hour", remaining, color);
        }
        shell.update(cx, |this, cx| {
            this.monitor.quota.cards[0].phase = QuotaCardPhase::Failed(QuotaFailure::MissingCredentials);
            cx.notify();
        });
        window.render_frame(cx);
        assert!(window.try_find("monitor-quota-codex").is_none());
        assert!(window.try_find("monitor-quota").is_none());
    }).unwrap();
}
