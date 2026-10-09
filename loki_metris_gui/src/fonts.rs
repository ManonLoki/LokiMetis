//! 进程内注册参考产品字体；各平台仅启用已验证的字重实现。

use gpui_kit::App;
use std::borrow::Cow;

/// macOS 使用历史产品原样 WOFF2，CoreText 补丁保留真实 wght 实例。
#[cfg(target_os = "macos")]
pub(crate) fn install(cx: &App) {
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(include_bytes!("../assets/fonts/Geist-Latin-variable.woff2")),
            Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-Latin-variable.woff2")),
        ])
        .expect("无法注册内嵌字体");
}

/// 其它目标暂保留静态字重，不能从 CoreText 的结果推断 DirectWrite 支持。
#[cfg(not(target_os = "macos"))]
pub(crate) fn install(cx: &App) {
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(include_bytes!("../assets/fonts/Geist-Regular.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/Geist-Medium.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/Geist-SemiBold.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/Geist-Bold.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/Geist-ExtraBold.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-Regular.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-Medium.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-SemiBold.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-Bold.ttf")),
            Cow::Borrowed(include_bytes!("../assets/fonts/GeistMono-ExtraBold.ttf")),
        ])
        .expect("无法注册内嵌字体");
}
