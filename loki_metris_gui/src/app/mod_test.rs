//! 使用 Kit 真实布局树验证侧栏和内容坐标，偏好写入只在内存中发生。

use super::*;
use gpui_kit::{AnyWindowHandle, AppContext, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions, point, size, test::TestWindowExt};

/// 测试退出时回收偏好工作线程，不读取或写入真实用户目录。
pub(super) struct FinishWriter(crate::preferences::Writer);

impl Drop for FinishWriter {
    /// 测试失败或正常结束都等待当前测试拥有的内存队列收尾。
    fn drop(&mut self) { self.0.finish(); }
}

/// 通过生产窗口入口建立固定大小、固定语言和隔离内存偏好的壳层。
pub(super) fn open_shell(cx: &mut TestAppContext, width: f32, height: f32) -> (AnyWindowHandle, Entity<Shell>, FinishWriter) {
    cx.update(gpui_kit::init);
    let (_, writer) = crate::preferences::load_from(None);
    let finish = FinishWriter(writer.clone());
    let preferences = Preferences { language: LanguageChoice::English, theme: ThemeChoice::Light, ..Preferences::default() };
    let (handle, shell) = cx.update(|cx| {
        gpui_kit::open_window(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), size(px(width), px(height))))),
            ..Default::default()
        }, cx, |window, cx| cx.new(|cx| Shell::new(preferences, writer, window, cx))).expect("创建真实布局测试窗口")
    });
    (handle, shell, finish)
}

/// 原生标题栏高度不能进入下一次创建的内容尺寸，装饰变化同样不会累计。
#[test]
fn saved_window_size_uses_content_viewport_without_native_decorations() {
    let first = window_geometry(Bounds::new(point(px(36.), px(58.)), size(px(1440.), px(933.))), size(px(1440.), px(900.)));
    assert_eq!(first, [36., 58., 1440., 900.]);
    let next = window_geometry(Bounds::new(point(px(first[0]), px(first[1])), size(px(first[2]), px(first[3] + 41.))), size(px(first[2]), px(first[3])));
    assert_eq!(next, first);
}

/// 窗口变化后直接退出应用也必须刷新最新快照，不依赖窗口关闭回调。
#[gpui_kit::test]
fn resized_window_is_saved_when_quitting_without_window_close(cx: &mut TestAppContext) {
    let root = std::fs::canonicalize(std::env::temp_dir()).expect("读取真实临时目录");
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("读取测试时间").as_nanos();
    let directory = root.join(format!("gpui-window-quit-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&directory).expect("创建隔离偏好目录");
    let file = directory.join("preferences.txt");
    let (preferences, writer) = crate::preferences::load_from(Some(file.clone()));
    let quit_writer = writer.clone();
    cx.update(gpui_kit::init);
    let (handle, shell) = cx.update(|cx| {
        gpui_kit::open_window(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), size(px(1440.), px(900.))))),
            ..Default::default()
        }, cx, |window, cx| cx.new(|cx| Shell::new(preferences, writer, window, cx))).expect("创建隔离测试窗口")
    });
    cx.simulate_window_resize(handle, size(px(1200.), px(800.)));
    cx.update_window(handle, |_, _, cx| {
        assert_eq!(shell.read(cx).preferences.bounds, Some([0., 0., 1200., 800.]));
    }).expect("分发原生窗口尺寸变化");
    quit_writer.finish();
    let saved = std::fs::read_to_string(file).expect("退出已刷新最后快照");
    assert!(saved.lines().any(|line| line == "bounds=0,0,1200,800"), "直接退出不能刷新旧窗口快照：{saved}");
    std::fs::remove_dir_all(directory).expect("清理隔离偏好目录");
}

/// 固定紧凑侧栏与主区共享高度和边界，Logo 周围不代理导航点击。
#[gpui_kit::test]
fn compact_sidebar_owns_navigation_and_keeps_fixed_bounds(cx: &mut TestAppContext) {
    let (handle, shell, _finish) = open_shell(cx, 1440., 900.);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let sidebar = window.find("sidebar").bounds();
        let main = window.find("main-content").bounds();
        assert_eq!(sidebar.size.width, px(80.));
        assert_eq!(main.origin.x, sidebar.right());
        assert_eq!(main.origin.y, sidebar.origin.y);
        assert_eq!(main.right(), px(1440.));
        assert!(window.try_find("sidebar-toggle").is_none());
        window.click("sidebar-logo", cx);
        assert_eq!(shell.read(cx).page, Page::Dashboard);
        window.click("nav-settings", cx);
        assert_eq!(shell.read(cx).page, Page::Settings);
        window.click("nav-dashboard", cx);
        assert_eq!(shell.read(cx).page, Page::Dashboard);
    }).expect("更新真实布局测试窗口");
}

/// 复制精确带分组整数并只改变按钮反馈；离开页面后仍保留当前客户端窗口。
#[gpui_kit::test]
fn overview_copies_exact_value_and_preserves_window_across_navigation(cx: &mut TestAppContext) {
    use loki_metris_core::{LocalIndexState, LocalRecordsSummary, LocalUsageWindow, MetricFact,
        MetricScope, ProviderKind, Freshness, Completeness, Confidence, WindowUsage,
        empty_local_usage_aggregate_for_provider};
    let (handle, shell, _finish) = open_shell(cx, 1440., 900.);
    let mut aggregate = empty_local_usage_aggregate_for_provider(ProviderKind::RolloutJsonl);
    aggregate.call_count = 1;
    aggregate.tokens.input_tokens = 436_273_700;
    aggregate.tokens.output_tokens = 80;
    aggregate.tokens.total_tokens = 436_273_780;
    let windows = LocalUsageWindow::OVERVIEW_WINDOWS.into_iter().map(|window| WindowUsage {
        window, fact: MetricFact::new(aggregate.clone(), ProviderKind::RolloutJsonl,
            MetricScope::DeviceObserved, 0, Freshness::Fresh, Completeness::Complete, Confidence::Exact, None),
    }).collect();
    shell.update(cx, |shell, cx| {
        shell.dashboard.data = Some(crate::usage_service::UsageData {
            records: LocalRecordsSummary { index_state: LocalIndexState::Ready, windows },
            roots: Vec::new(), observed_at_epoch_ms: 0, workbuddy: None, workbuddy_read_failed: false,
        });
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("usage-window", 1usize), cx);
        assert_eq!(shell.read(cx).dashboard.ranges[1], 1);
        window.click("copy-total", cx);
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("436,273,780"));
        assert!(shell.read(cx).dashboard.copied);
        window.click("nav-settings", cx);
        window.click("nav-dashboard", cx);
        assert_eq!(shell.read(cx).dashboard.ranges[1], 1);
    }).expect("验证真实事件与复制结果");
}
