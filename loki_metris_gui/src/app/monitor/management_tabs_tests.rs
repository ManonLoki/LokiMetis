//! 独立子进程主线程运行生产 Shell；只观察标签字重、原生宽度和原有选择/辅助语义。

#[path = "management_tabs_font_spy.rs"]
mod font_spy;

use crate::{
    app::{Page, Shell, monitor::state::Route},
    preferences::{LanguageChoice, Preferences, ThemeChoice, Writer},
};
use font_spy::{NativeFontSpy, ObservedLine};
use gpui_kit::{
    AppContext, FontId, FontRun, FontWeight, HeadlessAppContext, PlatformTextSystem, Role, font,
    px, size, test::TestWindowExt,
};
use loki_metris_core::{AiProfileDraft, public_monitor_ai_tools};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
    time::{Duration, Instant},
};

const TEST_NAME: &str =
    "app::monitor::management::tabs_tests::management_tabs_use_native_bold_and_keep_selection";
const PROBE_ENV: &str = "LOKI_METRIS_MANAGEMENT_TABS_NATIVE_TEST";

/// 仅精确目标子进程在真实主线程执行；普通测试进程初始化完全不产生动作。
#[used]
#[unsafe(link_section = "__DATA,__mod_init_func")]
static MAIN_THREAD_ENTRY: extern "C" fn() = native_test_entry;

/// 与原生 AX 独立进程测试一样验证真实 marker，并将 panic 明确反馈给父测试。
extern "C" fn native_test_entry() {
    if std::env::var(PROBE_ENV).as_deref() != Ok(TEST_NAME) {
        return;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        assert!(
            objc2::MainThreadMarker::new().is_some(),
            "原生字体测试必须在真正主线程运行"
        );
        objc2::rc::autoreleasepool(|_| exercise_shell());
    }));
    std::process::exit(if result.is_ok() { 0 } else { 1 });
}

/// 真实主线程字体入口通过有界子进程执行，既不伪造 marker，也不要求全仓串行。
#[test]
fn management_tabs_use_native_bold_and_keep_selection() {
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST_NAME, "--nocapture"])
        .env(PROBE_ENV, TEST_NAME)
        .spawn()
        .expect("启动独立原生字体测试进程");
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("读取原生测试回执") {
            assert!(
                status.success(),
                "真实主线程 Management 标签回归失败：{status}"
            );
            break;
        }
        if start.elapsed() > Duration::from_secs(45) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("真实主线程 Management 标签回归超过45秒");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 测试退出时只完成自己的内存偏好队列，不访问当前用户配置。
struct Finish(Writer);
impl Drop for Finish {
    /// 正常结束和断言失败都回收独占 writer。
    fn drop(&mut self) {
        self.0.finish();
    }
}

/// 使用实际 CoreText FontId 测量 oracle，不从待修控件提取预期字重。
fn native_width(spy: &NativeFontSpy, text: &str, size: f32, id: FontId) -> f32 {
    f32::from(
        spy.native
            .layout_line(
                text,
                px(size),
                &[FontRun {
                    len: text.len(),
                    font_id: id,
                }],
            )
            .width,
    )
}

/// 真实排版必须存在，而且输入身份与 shaping 输出身份都保持历史700实例。
fn assert_bold_lines(
    spy: &NativeFontSpy,
    lines: &[ObservedLine],
    label: &str,
    size: f32,
    bold: FontId,
) {
    let matching: Vec<_> = lines
        .iter()
        .filter(|line| line.text == label && line.size == px(size))
        .collect();
    assert!(
        !matching.is_empty(),
        "没有观察到真实 {label} / {size}px 排版"
    );
    for line in matching {
        assert!(
            !line.input.is_empty() && !line.output.is_empty(),
            "标签不能以空排版通过"
        );
        for id in &line.input {
            assert_eq!(
                spy.font(*id).weight,
                FontWeight::BOLD,
                "{label} 实际请求的字重错误：{line:?}"
            );
            assert_eq!(*id, bold, "{label} 必须使用真实700 FontId");
        }
        assert!(
            line.output.iter().all(|id| *id == bold),
            "{label} shaping改变了原生字体身份：{line:?}"
        );
        assert!((f32::from(line.width) - native_width(spy, label, size, bold)).abs() < 0.01);
    }
}

/// 五个实际管理标签均使用原样字体700，且点击后仍由各原生按钮独立更新选择。
fn exercise_shell() {
    let native_platform = gpui_kit::platform::current_platform(true);
    let spy = Arc::new(NativeFontSpy::new(native_platform.text_system()));
    let mut cx =
        HeadlessAppContext::with_asset_source(spy.clone(), Arc::new(crate::assets::Assets));
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::fonts::install(cx);
    });
    let mut regular = font("Geist");
    regular.weight = FontWeight::NORMAL;
    let regular = spy.font_id(&regular).expect("真实Geist400实例");
    let mut bold = font("Geist");
    bold.weight = FontWeight::BOLD;
    let bold = spy.font_id(&bold).expect("真实Geist700实例");
    assert_ne!(regular, bold, "不能以Noop等同字重的身份通过测试");
    assert!(
        (native_width(&spy, "Codex", 14., regular) - native_width(&spy, "Codex", 14., bold)).abs()
            > 0.01,
        "真实变量字体400与700的advance应可区分"
    );
    spy.clear_lines();
    let preferences = Preferences {
        language: LanguageChoice::English,
        theme: ThemeChoice::Light,
        ..Default::default()
    };
    let writer = Writer::in_memory(preferences.clone());
    let _finish = Finish(writer.clone());
    let (handle, shell) = cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |window, cx| {
            cx.new(|cx| {
                let mut shell = Shell::new(preferences, writer, window, cx);
                shell.agent_settings.value = Some(crate::monitor::settings::MonitorSettings {
                    enabled_ai_tools: public_monitor_ai_tools().collect(),
                    ..Default::default()
                });
                shell.page = Page::Monitor;
                shell.monitor.route = Route::Management;
                shell.monitor.catalog.accept_profiles(
                    public_monitor_ai_tools()
                        .map(AiProfileDraft::default_for)
                        .collect(),
                );
                shell.monitor.catalog.images_loaded = true;
                shell
            })
        })
        .unwrap()
    });
    cx.update_window(handle, |_, window, cx| {
        window.resize(size(px(1440.), px(1000.)));
        window.render_frame(cx);
    })
    .unwrap();
    let management = spy.lines();
    let tools: Vec<_> = public_monitor_ai_tools().collect();
    assert_eq!(tools.len(), 5, "本回归必须覆盖五个真实公开工具");
    for tool in &tools {
        let label = super::super::monitor_tool_display_name(*tool);
        assert_bold_lines(&spy, &management, label, 14., bold);
        cx.update_window(handle, |_, window, cx| {
            let id = format!("monitor-agent-{}", loki_metris_core::ai_tool_name(*tool));
            let button = window.find(id.clone());
            assert_eq!(button.role(), Some(Role::Tab));
            assert_eq!(button.label(), Some(label));
            // 原生TextLayout先向上取整逻辑宽度，随后Taffy才做设备像素吸附。
            let expected = native_width(&spy, label, 14., bold).ceil() + 24.;
            assert!(
                (f32::from(button.bounds().size.width) - expected).abs()
                    <= 1. / window.scale_factor(),
                "标签实际宽度须来自取整后的700排版加24px内距：{button:?} vs {expected}"
            );
            window.click(id, cx);
            window.render_frame(cx);
            assert_eq!(shell.read(cx).monitor.catalog.selected, Some(*tool));
            for candidate in &tools {
                let tab = window.find(format!(
                    "monitor-agent-{}",
                    loki_metris_core::ai_tool_name(*candidate)
                ));
                assert_eq!(tab.selected(), Some(candidate == tool));
            }
        })
        .unwrap();
    }
    let root = tempfile::tempdir().unwrap();
    let locations =
        crate::monitor::store::list_hook_config_locations(&Default::default(), root.path());
    spy.clear_lines();
    shell.update(&mut cx, |shell, cx| {
        shell.monitor.route = Route::Hooks;
        shell.monitor.hooks.accept_locations(locations);
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let hooks = spy.lines();
    for tool in &tools {
        let label = super::super::monitor_tool_display_name(*tool);
        assert_bold_lines(&spy, &hooks, label, 14.08, bold);
    }
    println!(
        "native tabs PASS: main-thread=true, Management=5×700, Hooks=5×700, distinct FontIds400={regular:?}/700={bold:?}, selection+AX=5"
    );
}
