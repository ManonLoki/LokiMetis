//! 真实来源弹窗验证原生语义与输入所有权；使用内存偏好且不连接来源写服务。

use super::*;
use gpui_kit::{AnyWindowHandle, AppContext, Role, TestAppContext, test::TestWindowExt};
use loki_metris_core::{RootActivationState, SourceDiscoveryCode};

/// 经生产行按钮打开弹窗，并保留真实触发控件焦点用于关闭恢复断言。
fn open_dialog(
    cx: &mut TestAppContext,
    locale: &'static str,
    remove: bool,
) -> (
    AnyWindowHandle,
    Entity<Shell>,
    FocusHandle,
    crate::app::tests::FinishWriter,
) {
    let (handle, shell, finish) = crate::app::tests::open_shell(cx, 1440., 900.);
    shell.update(cx, |this, cx| {
        this.locale = locale;
        this.dashboard.route = crate::app::dashboard::Route::Sources;
        this.dashboard.sources.roots = Some(vec![SourceRootSummary {
            id: "codex-root-a".into(),
            alias: "Codex fixture".into(),
            enabled: true,
            activation_state: RootActivationState::Ready,
            is_primary: true,
            discovery_label: String::new(),
            discovery_code: SourceDiscoveryCode::DefaultRoot,
            file_count: 7,
            skipped_count: 0,
            error_count: 0,
            duplicate_count: 0,
        }]);
        cx.notify();
    });
    let previous = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let trigger = if remove {
                "source-remove-0"
            } else {
                "source-rename-0"
            };
            // 鼠标点击不强制夺焦点，先经真实Tab到达触发按钮以明确恢复对象。
            for _ in 0..64 {
                if window.find(trigger).focused() == Some(true) {
                    break;
                }
                window.press("tab", cx);
            }
            assert_eq!(
                window.find(trigger).focused(),
                Some(true),
                "来源动作必须可由键盘到达"
            );
            let previous = window.focused(cx).unwrap();
            window.click(trigger, cx);
            window.render_frame(cx);
            let captured = shell
                .read(cx)
                .dashboard
                .sources
                .modal
                .as_ref()
                .unwrap()
                .previous_focus
                .clone()
                .expect("真实行按钮应被保存为恢复焦点");
            assert_eq!(captured, previous);
            captured
        })
        .unwrap();
    (handle, shell, previous, finish)
}

/// 原生Tab寻找真实按钮，不直接聚焦按钮或调用其回调。
fn tab_to(handle: AnyWindowHandle, id: &'static str, cx: &mut TestAppContext) {
    for _ in 0..6 {
        let focused = cx
            .update_window(handle, |_, window, _| {
                window.find(id).focused() == Some(true)
            })
            .unwrap();
        if focused {
            return;
        }
        cx.update_window(handle, |_, window, cx| window.press("tab", cx))
            .unwrap();
    }
    panic!("原生Tab无法在来源弹窗中到达 {id}");
}

/// 两种语言的重命名和移除均暴露真实Dialog、Heading与控件。
#[gpui_kit::test]
fn sources_dialog_a11y_exposes_translated_native_semantics(cx: &mut TestAppContext) {
    for (locale, rename, remove, input, body, close, cancel, save, confirm) in [
        (
            "en-US",
            "Rename data directory",
            "Remove data-root index",
            "New data-root alias",
            "Only this app’s index for “Codex fixture” will be removed. Original Codex files will not be deleted.",
            "Close",
            "Cancel",
            "Save",
            "Remove index",
        ),
        (
            "zh-CN",
            "重命名数据目录",
            "移除数据目录索引",
            "新的数据目录别名",
            "只移除“Codex fixture”的本产品索引；不会删除 Codex 原始文件。",
            "关闭",
            "取消",
            "保存",
            "移除索引",
        ),
    ] {
        for removing in [false, true] {
            let (handle, _, _, _finish) = open_dialog(cx, locale, removing);
            cx.update_window(handle, |_, window, _| {
                let title = if removing { remove } else { rename };
                for (id, role, label) in [
                    ("sources-dialog-surface", Role::Dialog, title),
                    ("sources-dialog-title", Role::Heading, title),
                    (
                        "sources-dialog-description",
                        Role::Label,
                        if removing { body } else { input },
                    ),
                ] {
                    let element = window.find(id);
                    assert_eq!(element.role(), Some(role));
                    assert_eq!(element.label(), Some(label));
                    assert_eq!(element.focused(), None, "静态语义不应新增Tab停靠点");
                }
                assert_eq!(window.find("sources-dialog-close").label(), Some(close));
                assert_eq!(window.find("sources-dialog-cancel").label(), Some(cancel));
                assert_eq!(
                    window.find("sources-dialog-confirm").label(),
                    Some(if removing { confirm } else { save })
                );
                if !removing {
                    let field = window.find("sources-dialog-input");
                    assert_eq!(field.role(), Some(Role::TextInput));
                    assert_eq!(field.label(), Some(input));
                    assert_eq!(field.value(), Some("Codex fixture"));
                    assert_eq!(field.focused(), Some(true));
                }
            })
            .unwrap();
        }
    }
}

/// 输入从原生初焦点接收文字，重绘保留实体与草稿，双向Tab始终处于真实焦点陷阱。
#[gpui_kit::test]
fn sources_dialog_a11y_keeps_input_and_traps_keyboard_focus(cx: &mut TestAppContext) {
    let (handle, shell, _, _finish) = open_dialog(cx, "en-US", false);
    let input_id = cx
        .update_window(handle, |_, window, cx| {
            let input = shell
                .read(cx)
                .dashboard
                .sources
                .modal
                .as_ref()
                .unwrap()
                .input
                .clone();
            assert!(input.focus_handle(cx).is_focused(window));
            window.press("cmd-a", cx);
            window.input("Codex renamed", cx);
            assert_eq!(input.read(cx).value().as_ref(), "Codex renamed");
            window.render_frame(cx);
            window.render_frame(cx);
            assert_eq!(
                shell
                    .read(cx)
                    .dashboard
                    .sources
                    .modal
                    .as_ref()
                    .unwrap()
                    .input
                    .entity_id(),
                input.entity_id()
            );
            input.entity_id()
        })
        .unwrap();
    for key in ["tab", "shift-tab"] {
        for _ in 0..12 {
            cx.update_window(handle, |_, window, cx| {
                window.press(key, cx);
                let modal = shell.read(cx).dashboard.sources.modal.as_ref().unwrap();
                assert!(
                    modal.focus.contains_focused(window, cx),
                    "{key}不能跳到后台"
                );
                assert_eq!(modal.input.entity_id(), input_id);
                assert_eq!(modal.input.read(cx).value().as_ref(), "Codex renamed");
            })
            .unwrap();
        }
    }
}

/// Confirm不能吞掉聚焦关闭/取消按钮的Enter；刷新失败时取消仍保持独立所有权。
#[gpui_kit::test]
fn sources_dialog_a11y_dismiss_keys_belong_to_focused_buttons(cx: &mut TestAppContext) {
    for id in ["sources-dialog-close", "sources-dialog-cancel"] {
        for key in ["enter", "space"] {
            let (handle, shell, previous, _finish) = open_dialog(cx, "en-US", false);
            shell.update(cx, |this, cx| {
                this.dashboard.sources.refresh_failed = true;
                cx.notify();
            });
            tab_to(handle, id, cx);
            cx.update_window(handle, |_, window, cx| {
                window.press(key, cx);
                assert!(
                    shell.read(cx).dashboard.sources.modal.is_none(),
                    "{id}/{key}应只取消"
                );
                assert_eq!(window.focused(cx), Some(previous));
                assert_eq!(
                    shell.read(cx).dashboard.sources.roots.as_ref().unwrap()[0].alias,
                    "Codex fixture"
                );
                assert!(!shell.read(cx).dashboard.sources.pending);
            })
            .unwrap();
        }
    }
}

/// 正文和容器内部点击不关闭、不导航；Close、取消、Escape与遮罩统一恢复触发焦点。
#[gpui_kit::test]
fn sources_dialog_a11y_dismissal_restores_owner_and_body_is_local(cx: &mut TestAppContext) {
    for remove in [false, true] {
        for action in [
            "sources-dialog-close",
            "sources-dialog-cancel",
            "escape",
            "backdrop",
        ] {
            let (handle, shell, previous, _finish) = open_dialog(cx, "en-US", remove);
            cx.update_window(handle, |_, window, cx| {
                window.click("sources-dialog-description", cx);
                assert!(shell.read(cx).dashboard.sources.modal.is_some());
                assert_eq!(
                    shell.read(cx).dashboard.route,
                    crate::app::dashboard::Route::Sources
                );
                if action == "escape" {
                    window.press(action, cx);
                } else if action == "backdrop" {
                    window.click_at("dialog-0", gpui_kit::point(px(5.), px(5.)), cx);
                } else {
                    window.click(action, cx);
                }
                assert!(shell.read(cx).dashboard.sources.modal.is_none(), "{action}");
                assert_eq!(window.focused(cx), Some(previous));
                assert_eq!(
                    shell.read(cx).dashboard.sources.roots.as_ref().unwrap()[0].alias,
                    "Codex fixture"
                );
                assert!(!shell.read(cx).dashboard.sources.pending);
            })
            .unwrap();
        }
    }
}

/// 默认确认仍归输入框：未改别名不提交，真实文字编辑后Enter关闭本次草稿。
#[gpui_kit::test]
fn sources_dialog_a11y_input_enter_retains_default_confirm(cx: &mut TestAppContext) {
    let (handle, shell, previous, _finish) = open_dialog(cx, "en-US", false);
    cx.update_window(handle, |_, window, cx| {
        window.press("enter", cx);
        assert!(shell.read(cx).dashboard.sources.modal.is_some());
        window.press("cmd-a", cx);
        window.input("Codex renamed", cx);
        window.press("enter", cx);
        assert!(shell.read(cx).dashboard.sources.modal.is_none());
        assert_eq!(window.focused(cx), Some(previous));
    })
    .unwrap();
}
