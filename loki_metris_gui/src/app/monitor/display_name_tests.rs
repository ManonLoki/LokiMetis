//! 真实 Monitor 视图的原生名称与标签使用公开目录，Hook 协议名称保持独立。

use super::{Shell, state::Route};
use crate::monitor::listener::{HookRelayLastEvent, HookRelayStatus};
use gpui_kit::{AnyWindowHandle, AppContext, Entity, Role, TestAppContext, test::TestWindowExt};
use loki_metris_core::{AiProfileDraft, AiTool, public_monitor_ai_tools};

/// 五个公开名称按产品目录固定，协议全集中的 OpenCode 用于验证未公开工具回退。
const PUBLIC_NAMES: [(AiTool, &str); 5] = [
    (AiTool::Codex, "Codex"),
    (AiTool::ClaudeCode, "Claude Code"),
    (AiTool::Cursor, "Cursor"),
    (AiTool::Grok, "Grok"),
    (AiTool::WorkBuddy, "WorkBuddy"),
];

/// 只构造真实编辑页所需快照，不访问用户设置或配置目录。
fn setup(
    cx: &mut TestAppContext,
    route: Route,
) -> (
    AnyWindowHandle,
    Entity<Shell>,
    crate::app::tests::FinishWriter,
) {
    let (handle, shell, writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    shell.update(cx, |this, cx| {
        this.page = crate::app::Page::Monitor;
        this.monitor.route = route;
        this.monitor.catalog.accept_profiles(
            public_monitor_ai_tools()
                .map(AiProfileDraft::default_for)
                .collect(),
        );
        this.monitor.catalog.images_loaded = true;
        this.monitor.hooks.accept_locations(Vec::new());
        cx.notify();
    });
    (handle, shell, writer)
}

/// 管理页所有公开工具的可访问名称遵守公开目录，稳定元素身份不因展示名改变。
#[gpui_kit::test]
fn management_native_tab_names_use_public_directory(cx: &mut TestAppContext) {
    let (handle, _shell, _writer) = setup(cx, Route::Management);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        for (tool, name) in PUBLIC_NAMES {
            let tab = window.find(format!(
                "monitor-agent-{}",
                loki_metris_core::ai_tool_name(tool)
            ));
            assert_eq!(tab.role(), Some(Role::Tab));
            assert_eq!(tab.label(), Some(name));
        }
    })
    .unwrap();
}

/// Hooks 使用相同公开名称，不能把 Grok Build 协议名重新暴露为标签。
#[gpui_kit::test]
fn hooks_native_tab_names_use_public_directory(cx: &mut TestAppContext) {
    let (handle, _shell, _writer) = setup(cx, Route::Hooks);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        for (tool, name) in PUBLIC_NAMES {
            let tab = window.find(format!(
                "monitor-hook-agent-{}",
                loki_metris_core::ai_tool_name(tool)
            ));
            assert_eq!(tab.role(), Some(Role::Tab));
            assert_eq!(tab.label(), Some(name));
        }
    })
    .unwrap();
}

/// 原生工作台事件标签对五个公开工具使用产品名，未公开 OpenCode 仍保留既有回退。
#[gpui_kit::test]
fn workbench_event_names_use_public_directory_and_keep_private_fallback(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = setup(cx, Route::Workbench);
    for (tool, name) in PUBLIC_NAMES
        .into_iter()
        .chain([(AiTool::OpenCode, "OpenCode")])
    {
        shell.update(cx, |this, cx| {
            this.monitor.status = Some(HookRelayStatus {
                last_event: Some(HookRelayLastEvent {
                    tool,
                    hook_type: "fixture-event".into(),
                }),
                ..Default::default()
            });
            cx.notify();
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let event = window.find("monitor-last-event");
            assert_eq!(event.role(), Some(Role::Label));
            assert!(
                event
                    .label()
                    .unwrap()
                    .ends_with(&format!("{name} / fixture-event")),
                "unexpected label {:?}",
                event.label()
            );
        })
        .unwrap();
    }
}
