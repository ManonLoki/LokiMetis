//! Kit的Root键盘上下文位于WindowBorder内侧；无焦点时由主窗补上首次Tab入口。

use gpui_kit::{AnyWindowHandle, App, Subscription};

/// 订阅由Shell持有并随其释放；只在目标主窗无焦点时交给原生tab stop遍历。
pub(super) fn install(main: AnyWindowHandle, cx: &mut App) -> Subscription {
    let main_id = main.window_id();
    cx.intercept_keystrokes(move |event, window, cx| {
        let key = &event.keystroke;
        let modifiers = key.modifiers;
        if window.window_handle().window_id() != main_id
            || window.focused(cx).is_some()
            || key.key != "tab"
            || modifiers.control
            || modifiers.alt
            || modifiers.platform
            || modifiers.function
        {
            return;
        }
        if modifiers.shift {
            window.focus_prev(cx);
        } else {
            window.focus_next(cx);
        }
        // 已有焦点的后续Tab仍走Root/Input/模态；本次成功获焦才阻止重复分发。
        if window.focused(cx).is_some() {
            cx.stop_propagation();
        }
    })
}
