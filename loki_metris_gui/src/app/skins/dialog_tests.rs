//! 真实壳层验证重启确认窗的语义与键盘关闭；不装配资源服务或调用宿主。

use super::{Page, Shell, state::{ModalKind, PendingApply, RestartMode}};
use gpui_kit::{
    AnyWindowHandle, AppContext, Entity, FocusHandle, InputEvent, KeyDownEvent, KeyUpEvent,
    Keystroke, Role, TestAppContext, test::TestWindowExt,
};
use loki_metris_core::{SkinHostKind, SkinReference, SkinSource};

/// 成功快照仅开放生产模态入口；资源服务缺席确保测试不会提交宿主操作。
fn open_restart(cx: &mut TestAppContext, locale: &'static str) -> (
    AnyWindowHandle, Entity<Shell>, FocusHandle, crate::app::tests::FinishWriter,
) {
    let (handle, shell, finish) = crate::app::tests::open_shell(cx, 1440., 900.);
    let previous = cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("nav-skins", cx);
        let previous = window.focused(cx).expect("导航按钮拥有实际焦点");
        shell.update(cx, |this, cx| {
            this.locale = locale;
            this.skins.host = Some(SkinHostKind::Codex);
            this.skins.entries = Some(Vec::new());
            this.skins.status = Some(loki_metris_service::skins::SkinStatus {
                installed: false, skin_id: None, source: None, package_type: None,
                skin_name: None, version: String::new(), affected_pages: 0,
                compatibility: None, instance_id: None,
            });
            this.open_skin_dialog(ModalKind::Restart(PendingApply {
                host: SkinHostKind::Codex,
                skin: SkinReference { source: SkinSource::Builtin, id: "pikachu".into() },
                instance: Some("fixture".into()), instance_label: "Codex fixture".into(),
                allow_appearance: false, allow_workbuddy_recovery: false,
                allow_third_party_code: false,
            }, RestartMode::RestartSelected), window, cx);
        });
        window.render_frame(cx);
        assert!(shell.read(cx).skins.modal.is_some());
        previous
    }).unwrap();
    (handle, shell, previous, finish)
}

/// 原生按钮在松开时点击；发送完整事件，避免只有按下导致键盘测试假失败。
fn press(handle: AnyWindowHandle, key: &'static str, cx: &mut TestAppContext) {
    let keystroke = Keystroke::parse(key).unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_event(KeyDownEvent {
            keystroke: keystroke.clone(), is_held: false, prefer_character_input: false,
        }.to_platform_input(), cx);
        window.dispatch_event(KeyUpEvent { keystroke }.to_platform_input(), cx);
        window.render_frame(cx);
    }).unwrap();
}

/// 两种语言均在实际Div上暴露模态、标题及正文名称，静态文字不进入Tab序列。
#[gpui_kit::test]
fn restart_dialog_exposes_translated_native_semantics(cx: &mut TestAppContext) {
    for (locale, title, description, close, cancel) in [
        ("en-US", "Restart the selected host instance?",
         "Codex fixture does not expose a local debug connection. Continuing restarts only this instance and may affect unsaved work.", "Close", "Cancel"),
        ("zh-CN", "重启所选宿主实例？",
         "Codex fixture 尚未开放本机调试连接。继续将只重启这个实例，未保存内容可能受影响。", "关闭", "取消"),
    ] {
        let (handle, _, _, _finish) = open_restart(cx, locale);
        cx.update_window(handle, |_, window, _| {
            for (id, role, label) in [
                ("skin-dialog-surface", Role::Dialog, title),
                ("skin-dialog-title", Role::Heading, title),
                ("skin-dialog-description", Role::Label, description),
            ] {
                let element = window.find(id);
                assert_eq!(element.role(), Some(role));
                assert_eq!(element.label(), Some(label));
                assert_eq!(element.focused(), None);
            }
            assert_eq!(window.find("skin-dialog-close").label(), Some(close));
            assert_eq!(window.find("skin-dialog-cancel").label(), Some(cancel));
        }).unwrap();
    }
}

/// 宿主快照失效后仍可用Enter与Space取消；不能由模态Confirm吞掉子按钮动作。
#[gpui_kit::test]
fn focused_dismiss_buttons_use_enter_and_space_without_confirming(cx: &mut TestAppContext) {
    for (id, tabs) in [("skin-dialog-close", 1), ("skin-dialog-cancel", 2)] {
        for key in ["enter", "space"] {
            let (handle, shell, previous, _finish) = open_restart(cx, "en-US");
            shell.update(cx, |this, cx| { this.skins.status = None; cx.notify(); });
            for _ in 0..tabs { press(handle, "tab", cx); }
            cx.update_window(handle, |_, window, _| {
                assert_eq!(window.find(id).focused(), Some(true), "{id}/{key}");
            }).unwrap();
            press(handle, key, cx);
            cx.update_window(handle, |_, window, cx| {
                assert!(shell.read(cx).skins.modal.is_none(), "{id}/{key}应只关闭模态");
                assert_eq!(window.focused(cx), Some(previous));
                assert!(!shell.read(cx).skins.busy);
                assert!(shell.read(cx).skins.mutation.is_none());
            }).unwrap();
        }
    }
}

/// 正反Tab循环始终位于现有Dialog焦点陷阱，正文点击不会关闭或导航。
#[gpui_kit::test]
fn restart_dialog_traps_focus_and_keeps_body_clicks_inside(cx: &mut TestAppContext) {
    let (handle, shell, _, _finish) = open_restart(cx, "en-US");
    for key in ["tab", "shift-tab"] {
        for _ in 0..12 {
            press(handle, key, cx);
            cx.update_window(handle, |_, window, cx| {
                let focus = &shell.read(cx).skins.modal.as_ref().unwrap().focus;
                assert!(focus.contains_focused(window, cx), "焦点不能逃出重启确认窗");
                assert_eq!(shell.read(cx).page, Page::Skins);
            }).unwrap();
        }
    }
    cx.update_window(handle, |_, window, cx| {
        window.click("skin-dialog-description", cx);
        assert!(shell.read(cx).skins.modal.is_some());
        assert_eq!(shell.read(cx).page, Page::Skins);
    }).unwrap();
}

/// 关闭按钮、取消、Escape及遮罩共用恢复路径，关闭后导航焦点恢复到原控件。
#[gpui_kit::test]
fn restart_dialog_dismissal_restores_previous_control(cx: &mut TestAppContext) {
    for action in ["skin-dialog-close", "skin-dialog-cancel", "escape", "backdrop"] {
        let (handle, shell, previous, _finish) = open_restart(cx, "en-US");
        if action == "escape" {
            press(handle, action, cx);
        } else {
            cx.update_window(handle, |_, window, cx| {
                if action == "backdrop" {
                    window.click_at("dialog-0", gpui_kit::point(gpui_kit::px(5.), gpui_kit::px(5.)), cx);
                } else { window.click(action, cx); }
            }).unwrap();
        }
        cx.update_window(handle, |_, window, cx| {
            assert!(shell.read(cx).skins.modal.is_none(), "{action}");
            assert_eq!(window.focused(cx), Some(previous));
        }).unwrap();
    }
}
