//! 原生 Kit 的完整编辑样式投影，只将本地 placeholder 改为历史输入色。

use gpui_kit::{
    App, IntoElement,
    base::input::{DiagnosticColors, InputEditorStyle},
    component::{
        ActiveTheme, IconName, Selectable, Sizable,
        button::{Button, ButtonRounded, ButtonVariants},
    },
    prelude::*,
    px, rgb,
};
use std::rc::Rc;

/// 与 Kit Input::render 使用相同的主题字段、诊断、选区、光标与折叠渲染器。
pub(super) fn project(dark: bool, cx: &App) -> InputEditorStyle {
    let theme = cx.theme();
    InputEditorStyle {
        foreground: theme.foreground,
        muted_foreground: rgb(if dark { 0x696969 } else { 0xadb5bd }).into(),
        background: theme
            .highlight_theme
            .style
            .editor_background
            .unwrap_or_else(|| theme.input_background()),
        border: theme.border,
        selection: theme.selection,
        caret: theme.caret,
        diagnostics: DiagnosticColors {
            error: theme.highlight_theme.style.status.error(cx),
            warning: theme.highlight_theme.style.status.warning(cx),
            info: theme.highlight_theme.style.status.info(cx),
            hint: theme.highlight_theme.style.status.hint(cx),
        },
        highlight_styles: theme.highlight_theme.clone(),
        editor_invisible: theme.highlight_theme.style.editor_invisible,
        editor_active_line: theme.highlight_theme.style.editor_active_line,
        editor_gutter_background: theme.highlight_theme.style.editor_gutter_background,
        fold_icon_renderer: Some(Rc::new(|ix, is_folded| {
            Button::new(("fold-icon", ix))
                .ghost()
                .icon(if is_folded {
                    IconName::ChevronRight
                } else {
                    IconName::ChevronDown
                })
                .xsmall()
                .rounded(ButtonRounded::Small)
                .size(px(14.))
                .selected(is_folded)
                .into_any_element()
        })),
    }
}
