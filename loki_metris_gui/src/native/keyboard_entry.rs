//! Kit根视图无焦点时补上首次Tab入口，每个订阅只处理其所属窗口。

use gpui_kit::{AnyWindowHandle, App, Subscription};

/// 订阅由窗口内容实体持有并随其释放；已有焦点继续使用原生控件分发。
pub(crate) fn install(handle: AnyWindowHandle, cx: &mut App) -> Subscription {
    let window_id = handle.window_id();
    cx.intercept_keystrokes(move |event, window, cx| {
        let key = &event.keystroke;
        let modifiers = key.modifiers;
        if window.window_handle().window_id() != window_id
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
