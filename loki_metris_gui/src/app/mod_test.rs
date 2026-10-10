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
    let preferences = Preferences { language: LanguageChoice::English, theme: ThemeChoice::Light, ..Preferences::default() };
    let writer=crate::preferences::Writer::in_memory(preferences.clone());
    let finish = FinishWriter(writer.clone());
    let (handle, shell) = cx.update(|cx| {
        gpui_kit::open_window(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), size(px(width), px(height))))),
            ..Default::default()
        }, cx, |window, cx| cx.new(|cx| {let mut shell=Shell::new(preferences, writer, window, cx);shell.agent_settings.value=Some(crate::monitor::settings::MonitorSettings{enabled_ai_tools:loki_metris_core::public_monitor_ai_tools().collect(),..Default::default()});shell})).expect("创建真实布局测试窗口")
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

/// 旧几何平台的窗口变化直接保存；macOS 的 AppKit V2 反馈由独立宿主边界验证。
#[cfg(not(target_os = "macos"))]
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

/// 不支持原生句柄时，真实退出钩子不得用虚拟 resize 覆盖最后可信的 macOS 状态。
#[cfg(target_os = "macos")]
#[gpui_kit::test]
fn unsupported_native_window_preserves_confirmed_geometry_on_actual_quit(cx: &mut TestAppContext) {
    use loki_metris_core::window_placement::*;
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("preferences.txt");
    let area = WindowDisplayArea {
        key: "isolated-test-screen".into(), scale: 2., primary: true,
        work_area: WindowDipRect {
            origin: WindowDipPoint { x: 0., y: 24. },
            size: WindowDipSize { width: 2000., height: 1200. },
        },
    };
    let confirmed = capture_window_state(&area, WindowDipPoint { x: 85., y: 119. },
        WindowDipSize { width: 1320., height: 820. }, WindowFrameInsets::default(), false).unwrap();
    std::fs::write(&file, format!("theme=light\nlanguage=en-US\nbounds=85,119,1320,820\nwindow_state_v2={}\n",
        serde_json::to_string(&confirmed).unwrap())).unwrap();
    let (preferences, writer) = crate::preferences::load_from(Some(file.clone()));
    let (_, guard) = tracing_appender::non_blocking(std::io::sink());
    let shutdown = crate::lifecycle::Shutdown::new(writer.clone(), guard);
    cx.update(gpui_kit::init);
    let (handle, shell) = cx.update(|cx| {
        gpui_kit::open_window(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), size(px(1440.), px(900.))))),
            ..Default::default()
        }, cx, |window, cx| cx.new(|cx| Shell::new(preferences, writer.clone(), window, cx))).unwrap()
    });
    shutdown.borrow_mut().own_window(handle, shell.downgrade());
    cx.update(|cx| crate::lifecycle::install(cx, shutdown.clone()));
    cx.simulate_window_resize(handle, size(px(1200.), px(800.)));
    cx.update_window(handle, |_, window, cx| {
        assert!(crate::native::windowing::WindowHost::capture(window).is_err(), "夹具确实没有可捕获的原生对象");
        assert_eq!(window.viewport_size(), size(px(1200.), px(800.)));
        assert_eq!(shell.read(cx).preferences.window_state_v2.as_ref(), Some(&confirmed));
    }).unwrap();
    // 真实异步偏好也必须经同一退出钩子排空，不能只证明预置文件从未被访问。
    drop(writer.mutate(crate::preferences::PreferencePatch::Theme(ThemeChoice::Dark)).unwrap());
    cx.quit();
    assert!(writer.mutate(crate::preferences::PreferencePatch::Theme(ThemeChoice::Light)).is_err(), "退出钩子已关闭实际 Writer");
    let (saved, finished) = crate::preferences::load_from(Some(file));
    finished.finish();
    assert_eq!(saved.theme, ThemeChoice::Dark);
    assert_eq!(saved.bounds, Some([85., 119., 1320., 820.]));
    assert!(!saved.maximized);
    assert_eq!(saved.window_state_v2, Some(confirmed));
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
        assert_eq!(window.find("sidebar-identity").bounds().size.height, px(70.));
        assert_eq!(window.find("nav-dashboard").bounds().origin.y, px(93.));
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

/// 统计和图表的窗口按 Agent 隔离；全部可继续看图，不能误入物理统计页。
#[gpui_kit::test]
fn analytics_filters_are_independent_across_routes_and_agents(cx:&mut TestAppContext) {
    let (handle,shell,_finish)=open_shell(cx,1440.,900.);
    cx.update_window(handle,|_,window,cx|{
        window.render_frame(cx);window.click("dashboard-usage",cx);
        window.click(("statistics-window",1usize),cx);
        window.click(("usage-client",2usize),cx);
        assert_eq!(shell.read(cx).dashboard.detail.ranges[2],2);
        window.click(("usage-client",1usize),cx);
        assert_eq!(shell.read(cx).dashboard.detail.ranges[1],1);
        window.click("dashboard-charts",cx);
        assert_eq!(shell.read(cx).dashboard.chart.ranges[1],0);
        window.click(("chart-window",4usize),cx);
        window.click(("usage-client",0usize),cx);
        assert_eq!(shell.read(cx).dashboard.chart.ranges[0],0);
        assert!(window.try_find("chart-dimension").is_none());
        assert!(window.try_find("dashboard-calls").is_some());
        window.click(("usage-client",1usize),cx);
        window.click("dashboard-charts",cx);
        assert_eq!(shell.read(cx).dashboard.chart.ranges[1],4);
        assert_eq!(shell.read(cx).dashboard.ranges[1],0);
    }).expect("验证真实筛选事件与跨 Agent 状态");
}

/// 曲线至少保留一条；切 Agent 后使用该 Agent 自己的勾选集合。
#[gpui_kit::test]
fn chart_cannot_hide_last_series_and_keeps_agent_choices(cx:&mut TestAppContext) {
    use loki_metris_core::*;
    let (handle,shell,_finish)=open_shell(cx,1440.,900.);
    let data=build_usage_chart_with_standard(&canonicalize_usage_calls(Vec::new()),&std::collections::BTreeMap::new(),
        LocalIndexState::Ready,&empty_coverage(),LocalUsageWindow::Today,UsageChartDimension::Model,
        1_786_000_000_000,ProviderKind::RolloutJsonl,None,TimeStandard::Local,&jiff::tz::TimeZone::UTC).unwrap();
    shell.update(cx,|shell,cx|{shell.dashboard.chart.data=Some(data);cx.notify();});
    cx.update_window(handle,|_,window,cx|{
        window.render_frame(cx);window.click("dashboard-charts",cx);
        window.click(("chart-series",1usize),cx);window.click(("chart-series",4usize),cx);
        window.click(("chart-series",0usize),cx);
        assert_eq!(shell.read(cx).dashboard.chart.visible[1],[true,false,false,false,false,false]);
        let plot=window.find("token-trend").bounds();let scale=f32::from(plot.size.width)/920.;
        for (x,expected) in [(73.,Some(0)),(485.,Some(12)),(897.,Some(23)),(20.,None)] {
            window.simulate_mouse_move(plot.origin+point(px(x*scale),px(100.*scale)),cx);
            window.render_frame(cx);
            assert_eq!(shell.read(cx).dashboard.chart.hover[0].get(),expected);
        }
        window.click(("usage-client",2usize),cx);
        assert_eq!(shell.read(cx).dashboard.chart.visible[2],[true,true,false,false,true,false]);
        window.click(("usage-client",1usize),cx);
        assert_eq!(shell.read(cx).dashboard.chart.visible[1],[true,false,false,false,false,false]);
    }).expect("验证真实 Checkbox 事件与最少一条曲线约束");
}

/// 查询失败必须隐藏旧结果；首次加载有骨架，重试不会触发扫描。
#[gpui_kit::test]
fn analytics_loading_and_failure_hide_untrusted_old_results(cx:&mut TestAppContext) {
    let (handle,shell,_finish)=open_shell(cx,1440.,900.);
    shell.update(cx,|shell,cx|{shell.dashboard.route=dashboard::Route::Charts;shell.dashboard.loading=true;cx.notify();});
    cx.update_window(handle,|_,window,cx|{
        window.render_frame(cx);assert!(window.try_find("usage-loading").is_some());
        assert!(window.try_find("chart-dimension").is_none());
    }).unwrap();
    shell.update(cx,|shell,cx|{shell.dashboard.loading=false;shell.dashboard.error=Some("usage_read_failed");cx.notify();});
    cx.update_window(handle,|_,window,cx|{
        window.render_frame(cx);assert!(window.try_find("usage-retry").is_some());
        assert!(window.try_find("chart-dimension").is_none());
        window.click("usage-retry",cx);assert!(!shell.read(cx).dashboard.scanning);
    }).unwrap();
}

/// All 只暴露概览/调用；排序变化及清除筛选保留会话排序，WorkBuddy 退出调用。
#[gpui_kit::test]
fn calls_navigation_sorting_and_clear_preserve_contract(cx:&mut TestAppContext) {
    use loki_metris_core::*;
    let (handle,shell,_finish)=open_shell(cx,1440.,900.);
    cx.update_window(handle,|_,window,cx|{
        window.render_frame(cx);window.click("dashboard-charts",cx);window.click(("usage-client",0usize),cx);
        assert_eq!(shell.read(cx).dashboard.route,dashboard::Route::Overview);
        assert!(window.try_find("dashboard-charts").is_none());assert!(window.try_find("dashboard-calls").is_some());
        window.click("dashboard-calls",cx);
    }).unwrap();
    let page=build_usage_calls_page(&[],LocalIndexState::Ready,&std::collections::BTreeMap::new(),&UsageCallsQuery::default(),0,
        SourceClientKind::Codex,ProviderKind::RolloutJsonl,None).unwrap();
    shell.update(cx,|shell,cx|{shell.dashboard.calls.page=Some(page);shell.dashboard.calls.queries[0].filters.model=Some("model-filter".into());cx.notify();});
    cx.update_window(handle,|_,window,cx|{
        window.render_frame(cx);window.click(("calls-sort",4usize),cx);
        assert_eq!(shell.read(cx).dashboard.calls.queries[0].sort_field,UsageCallSortField::TotalTokens);
        assert_eq!(shell.read(cx).dashboard.calls.queries[0].sort_direction,UsageCallSortDirection::Desc);
        window.click(("calls-sort",4usize),cx);window.click("calls-clear",cx);
        assert_eq!(shell.read(cx).dashboard.calls.queries[0].filters,UsageCallFilters::default());
        assert_eq!(shell.read(cx).dashboard.calls.queries[0].sort_direction,UsageCallSortDirection::Asc);
        window.click(("usage-client",1usize),cx);
        assert_eq!(shell.read(cx).dashboard.route,dashboard::Route::Calls);
        assert_eq!(shell.read(cx).dashboard.calls.queries[1],UsageCallsQuery::default());
        window.click(("usage-client",4usize),cx);
        assert_eq!(shell.read(cx).dashboard.route,dashboard::Route::Overview);
    }).unwrap();
}

/// 长时间文案与长项目名不能覆盖相邻列；Token 精确值与紧凑值留在同一列。
#[gpui_kit::test]
fn calls_columns_keep_time_project_and_token_bounds_separate(cx:&mut TestAppContext) {
    use loki_metris_core::*;
    let (handle,shell,_finish)=open_shell(cx,1440.,900.);
    let tokens=TokenUsage::new(1_234_567,0,None,123,0,Some(1_234_690)).unwrap();
    let mut page=build_usage_calls_page(&[],LocalIndexState::Ready,&std::collections::BTreeMap::new(),&UsageCallsQuery::default(),0,
        SourceClientKind::Codex,ProviderKind::RolloutJsonl,None).unwrap();
    page.items.push(UsageCallItem {id:"layout-call".into(),client:SourceClientKind::Codex,occurred_at_epoch_ms:1_791_488_880_000,
        project_label:"a-very-long-project-directory-name".into(),project_label_code:DisplayLabelCode::Literal,
        thread_label:"thread-12345678".into(),thread_label_code:DisplayLabelCode::Literal,
        model_label:"gpt-6.1-sol".into(),model_label_code:DisplayLabelCode::Literal,
        reasoning_effort_label:"ultra".into(),reasoning_effort_label_code:DisplayLabelCode::Literal,
        uncached_input_tokens:Some(tokens.input_tokens),usage:tokens.clone(),confidence:Confidence::Exact,
        fact:MetricFact::new(tokens,ProviderKind::RolloutJsonl,MetricScope::ThreadObserved,0,Freshness::Fresh,Completeness::Complete,Confidence::Exact,None)});
    shell.update(cx,|shell,cx|{shell.dashboard.client=0;shell.dashboard.route=dashboard::Route::Calls;shell.dashboard.loading=false;
        shell.dashboard.calls.page=Some(page);cx.notify();});
    cx.update_window(handle,|_,window,cx|{
        window.render_frame(cx);
        let time=window.find("call-0-1").bounds();let project=window.find("call-0-2").bounds();let thread=window.find("call-0-3").bounds();
        assert!(time.right()<=project.left());assert!(project.right()<=thread.left());
        let label="a-very-long-project-directory-name";
        let run=gpui_kit::TextRun {len:label.len(),font:gpui_kit::font("Geist"),..Default::default()};
        let required=window.text_system().shape_line(label.into(),px(14.),&[run],None).width();
        assert!(project.size.width+px(1.)>=required,"项目列 {:?} 小于实际字体宽度 {:?}",project.size.width,required);
        assert!(window.try_find("call-0-6").is_some());assert!(window.try_find("calls-count").is_some());
    }).unwrap();
}

/// 来源弹窗从真实行按钮打开，取消不改登记；离页重新挂载清除局部反馈与草稿。
#[gpui_kit::test]
fn sources_dialog_is_local_and_scan_busy_blocks_new_row_actions(cx:&mut TestAppContext) {
    use loki_metris_core::*;
    let (handle,shell,_finish)=open_shell(cx,1440.,900.);
    let root=SourceRootSummary{id:"codex-root-a".into(),alias:"Codex fixture".into(),enabled:true,
        activation_state:RootActivationState::Ready,is_primary:true,discovery_label:String::new(),
        discovery_code:SourceDiscoveryCode::DefaultRoot,file_count:7,skipped_count:0,error_count:0,duplicate_count:0};
    shell.update(cx,|shell,cx| {shell.dashboard.route=dashboard::Route::Sources;shell.dashboard.sources.roots=Some(vec![root]);cx.notify();});
    cx.update_window(handle,|_,window,cx| {
        window.render_frame(cx);window.click("source-rename-0",cx);window.render_frame(cx);
        assert!(shell.read(cx).dashboard.sources.modal.is_some());
        window.click("sources-dialog-confirm",cx);
        assert!(shell.read(cx).dashboard.sources.modal.is_some(),"未修改别名不能提交");
        window.click("sources-dialog-cancel",cx);window.render_frame(cx);
        assert!(shell.read(cx).dashboard.sources.modal.is_none());
        assert_eq!(shell.read(cx).dashboard.sources.roots.as_ref().unwrap()[0].alias,"Codex fixture");
        shell.update(cx,|shell,cx| {shell.dashboard.scanning=true;cx.notify();});
        window.render_frame(cx);window.click("source-remove-0",cx);
        assert!(shell.read(cx).dashboard.sources.modal.is_none(),"扫描忙时不能新开行操作");
        shell.update(cx,|shell,cx| {shell.dashboard.scanning=false;cx.notify();});
        window.render_frame(cx);window.click("source-remove-0",cx);
        assert!(shell.read(cx).dashboard.sources.modal.is_some());
        window.click("sources-dialog-cancel",cx);window.click("dashboard-overview",cx);
        assert!(shell.read(cx).dashboard.sources.roots.is_none());
        assert!(shell.read(cx).dashboard.sources.feedback.is_none());
    }).unwrap();
}

/// 普通重绘和扫描进度不重建数字编辑器；越界草稿保持可见并阻止提交。
#[gpui_kit::test]
fn usage_settings_keeps_native_editor_and_invalid_draft_across_render(cx:&mut TestAppContext) {
    let (handle,shell,_finish)=open_shell(cx,1440.,900.);
    shell.update(cx,|shell,cx|{shell.dashboard.route=dashboard::Route::Settings;shell.dashboard.usage_settings.saved=Some(loki_metris_core::UsageSettings::default());shell.dashboard.usage_settings.reset_inputs=[Some("5".into()),Some("90".into())];cx.notify();});
    cx.update_window(handle,|_,window,cx|{
        window.render_frame(cx);let input=shell.read(cx).dashboard.usage_settings.inputs.as_ref().unwrap()[0].clone();let id=input.entity_id();
        input.update(cx,|input,cx|input.replace_all("0",window,cx));window.render_frame(cx);window.render_frame(cx);
        assert_eq!(shell.read(cx).dashboard.usage_settings.inputs.as_ref().unwrap()[0].entity_id(),id);
        assert_eq!(input.read(cx).value().as_ref(),"0");assert!(window.try_find(("usage-settings-invalid",0usize)).is_some());
        window.click(("usage-settings-save",0usize),cx);assert!(!shell.read(cx).dashboard.usage_settings.pending[0]);
    }).unwrap();
}
