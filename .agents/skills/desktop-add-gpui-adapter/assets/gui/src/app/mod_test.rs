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

/// 详细菜单左对齐；两档按钮中心都锚定边界和 Logo，伸出的右半边可点击且父级不代理。
#[gpui_kit::test]
fn sidebar_alignment_and_toggle_share_the_fixed_shell_bounds(cx: &mut TestAppContext) {
    let (handle, shell, _finish) = open_shell(cx, 1440., 900.);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let sidebar = window.find("sidebar").bounds();
        let main = window.find("main-content").bounds();
        assert_eq!(sidebar.size.width, px(if @@COMPACT@@ { 80. } else { 248. }));
        assert_eq!(main.origin.x, sidebar.right());
        assert_eq!(main.origin.y, sidebar.origin.y);
        assert_eq!(main.right(), px(1440.));
        if @@COMPACT@@ {
            assert!(window.try_find("sidebar-toggle").is_none());
        } else {
            let icon = window.find(("nav-settings", 1usize)).bounds();
            let row = window.find(("nav-settings", 0usize)).bounds();
            assert_eq!(icon.origin.x, row.origin.x, "详细菜单图标不能被 Kit 内层默认居中");
            assert!(icon.origin.x < sidebar.origin.x + px(32.));
            for collapsed in [false, true] {
                window.render_frame(cx);
                let sidebar = window.find("sidebar").bounds();
                let main = window.find("main-content").bounds();
                let logo = window.find("sidebar-logo").bounds();
                let toggle = window.find("sidebar-toggle").bounds();
                assert_eq!(shell.read(cx).preferences.collapsed, collapsed);
                assert_eq!(sidebar.size.width, px(if collapsed { 76. } else { 248. }));
                assert_eq!(logo.size, size(px(if collapsed { 44. } else { 72. }), px(if collapsed { 44. } else { 72. })));
                assert_eq!(toggle.size, size(px(24.), px(24.)));
                assert_eq!(toggle.center().x, sidebar.right(), "按钮必须横跨侧栏和内容边界");
                assert_eq!(toggle.center().y, logo.center().y, "按钮与 Logo 必须沿同一水平中心线");
                assert_eq!(main.origin.x, sidebar.right());
                assert_eq!(main.origin.y, sidebar.origin.y);
                assert_eq!(main.right(), px(1440.));
                if collapsed {
                    assert!(window.try_find(("nav-settings", 2usize)).is_none());
                    let icon = window.find(("nav-settings", 1usize)).bounds();
                    assert!((icon.center().x - sidebar.center().x).abs() <= px(1.));
                }
                println!("toggle geometry collapsed={collapsed}: sidebar={sidebar:?} logo={logo:?} toggle={toggle:?} main={main:?}");
                // Button 之外的 Logo/身份区与主内容不拥有折叠动作。
                window.click("sidebar-logo", cx);
                window.click("sidebar-identity", cx);
                window.click_at("application-shell", point(sidebar.right() + px(20.), logo.center().y), cx);
                assert_eq!(shell.read(cx).preferences.collapsed, collapsed, "周围父级不能代理 Button 动作");
                // 点击按钮右半边，真实指针落在主内容范围内，不能被它覆盖或被侧栏裁切。
                window.click_at("sidebar-toggle", point(px(18.), px(12.)), cx);
                assert_eq!(shell.read(cx).preferences.collapsed, !collapsed, "伸出边界的右半边必须实际可点击");
            }
            window.render_frame(cx);
            assert!(!shell.read(cx).preferences.collapsed);
        }
    }).expect("更新真实布局测试窗口");
}
