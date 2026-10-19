//! 品牌材质接入真实看板后，卡片留白与子控件仍保持各自输入所有权。

use super::*;
use gpui_kit::{AppContext, Bounds, Pixels, TestAppContext, Window, point, test::TestWindowExt};
use loki_metris_core::{
    Completeness, Confidence, Freshness, LocalIndexState, LocalRecordsSummary, LocalUsageWindow, MetricFact, MetricScope, ProviderKind, TimeStandard,
    UsageChartDimension, WindowUsage, build_usage_chart_with_standard, canonicalize_usage_calls,
    empty_coverage, empty_local_usage_aggregate_for_provider,
};

/// 使用最小可信事实构造概览；没有服务、用户目录或额外后台任务。
fn overview() -> UsageData {
    let mut aggregate = empty_local_usage_aggregate_for_provider(ProviderKind::RolloutJsonl);
    aggregate.call_count = 1;
    aggregate.tokens.input_tokens = 1200;
    aggregate.tokens.output_tokens = 34;
    aggregate.tokens.total_tokens = 1234;
    let windows = LocalUsageWindow::OVERVIEW_WINDOWS
        .into_iter()
        .map(|window| WindowUsage {
            window,
            fact: MetricFact::new(
                aggregate.clone(), ProviderKind::RolloutJsonl, MetricScope::DeviceObserved, 0,
                Freshness::Fresh, Completeness::Complete, Confidence::Exact, None,
            ),
        })
        .collect();
    UsageData {
        records: LocalRecordsSummary { index_state: LocalIndexState::Ready, windows },
        workbuddy: None,
        workbuddy_unavailable: false,
    }
}

/// 从真实半透明底色检查材质；设备像素取整允许一个物理像素的边缘差异。
fn painted_brand_surface(window: &Window, id: &'static str) -> Bounds<Pixels> {
    let bounds = window.find(id).bounds();
    let expected = bounds.scale(window.scale_factor());
    let material = window.painted_quads().into_iter().find(|quad| {
        (quad.bounds.origin.x - expected.origin.x).0.abs() <= 1.
            && (quad.bounds.origin.y - expected.origin.y).0.abs() <= 1.
            && (quad.bounds.size.width - expected.size.width).0.abs() <= 1.
            && (quad.bounds.size.height - expected.size.height).0.abs() <= 1.
            && quad.background.as_solid().is_some_and(|color| (color.a - 0.90).abs() < 1e-6)
    }).expect("目标卡片必须绘制真实品牌底色");
    assert_eq!(material.corner_radii.top_left, px(20.).scale(window.scale_factor()));
    bounds
}

/// 真实Hero卡的留白只产生材质反馈，复制仅由复制按钮提交且不影响窗口选择。
#[gpui_kit::test]
fn overview_brand_surface_keeps_copy_on_its_button(cx: &mut TestAppContext) {
    let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 1200.);
    shell.update(cx, |shell, cx| {
        shell.dashboard.data = Some(overview());
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        for id in ["hero-dashboard.calls", "hero-dashboard.tokens", "overview-token-composition", "overview-coverage"] {
            let bounds = painted_brand_surface(window, id);
            if id.starts_with("hero-") { assert_eq!(bounds.size.height, px(176.)); }
        }
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("before".into()));
        window.click_at("hero-dashboard.tokens", point(px(8.), px(8.)), cx);
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("before"));
        assert!(!shell.read(cx).dashboard.copied);
        assert_eq!(shell.read(cx).dashboard.ranges[1], 0);
        window.click("copy-total", cx);
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("1,234"));
        assert!(shell.read(cx).dashboard.copied);
        assert_eq!(shell.read(cx).dashboard.ranges[1], 0);
    }).expect("验证真实Hero材质及复制事件所有权");
}

/// 聚光父层不能改变曲线选择或拦截图表hover；原生Checkbox保持独立动作。
#[gpui_kit::test]
fn chart_brand_surface_preserves_series_and_canvas_input(cx: &mut TestAppContext) {
    let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 2200.);
    let chart = build_usage_chart_with_standard(
        &canonicalize_usage_calls(Vec::new()), &std::collections::BTreeMap::new(),
        LocalIndexState::Ready, &empty_coverage(), LocalUsageWindow::Today, UsageChartDimension::Model,
        1_786_000_000_000, ProviderKind::RolloutJsonl, None, TimeStandard::Local, &jiff::tz::TimeZone::UTC,
    ).unwrap();
    shell.update(cx, |shell, cx| {
        shell.dashboard.route = Route::Charts;
        shell.dashboard.chart.data = Some(chart);
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        for id in ["chart-token-trend", "chart-calls-trend", "chart-distribution"] {
            painted_brand_surface(window, id);
        }
        let initial = shell.read(cx).dashboard.chart.visible[1];
        window.click_at("chart-token-trend", point(px(8.), px(8.)), cx);
        assert_eq!(shell.read(cx).dashboard.chart.visible[1], initial);
        window.click(("chart-series", 1usize), cx);
        assert_eq!(shell.read(cx).dashboard.chart.visible[1], [true, false, false, false, true, false]);
        let plot = window.find("token-trend").bounds();
        let scale = f32::from(plot.size.width) / 920.;
        window.simulate_mouse_move(plot.origin + point(px(485. * scale), px(100. * scale)), cx);
        window.render_frame(cx);
        assert_eq!(shell.read(cx).dashboard.chart.hover[0].get(), Some(12));
        assert_eq!(shell.read(cx).dashboard.chart.visible[1], [true, false, false, false, true, false]);
    }).expect("验证真实图卡材质及Checkbox和canvas输入");
}
