//! 应用身份、赞助媒体与设备示意图在编译期嵌入；其余资源交由 GPUI Kit。

use gpui_kit::{AssetSource, Result, SharedString};
use std::borrow::Cow;

/// 窗口内共享的已选择应用图标路径。
pub const LOGO: &str = "app-identity/logo.png";
/// 原样迁入的本地微信收款图，只用于显示。
pub const SPONSOR_WECHAT:&str="app-support/wechat-pay.png";
/// 原样迁入的本地支付宝收款图，只用于显示。
pub const SPONSOR_ALIPAY:&str="app-support/alipay.jpg";

/// 设备卡片使用固定本地产品示意图，不请求远程图片。
pub const DEVICE_IPHONE: &str = "app-devices/iphone.png";
pub const DEVICE_ANDROID_PHONE: &str = "app-devices/android-phone.png";
pub const DEVICE_IPAD: &str = "app-devices/ipad.png";
pub const DEVICE_ANDROID_TABLET: &str = "app-devices/android-tablet.png";
/// iPhone卡片内的附属Watch示意图，不用于识别实际型号。
pub const DEVICE_WATCH: &str = "app-devices/watch.png";

/// 固定本地资源源不接受任意文件路径或远程 URL。
pub struct Assets;

impl AssetSource for Assets {
    /// 精确匹配本产品的固定嵌入资源，其余路径交由组件库资源源处理。
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == LOGO {
            return Ok(Some(Cow::Borrowed(include_bytes!("../assets/logo.png"))));
        }

        match path {
            DEVICE_IPHONE => Ok(Some(Cow::Borrowed(include_bytes!("../assets/devices/iphone.png")))),
            DEVICE_ANDROID_PHONE => Ok(Some(Cow::Borrowed(include_bytes!("../assets/devices/android-phone.png")))),
            DEVICE_IPAD => Ok(Some(Cow::Borrowed(include_bytes!("../assets/devices/ipad.png")))),
            DEVICE_ANDROID_TABLET => Ok(Some(Cow::Borrowed(include_bytes!("../assets/devices/android-tablet.png")))),
            DEVICE_WATCH => Ok(Some(Cow::Borrowed(include_bytes!("../assets/devices/watch.png")))),
            SPONSOR_WECHAT=>Ok(Some(Cow::Borrowed(include_bytes!("../assets/sponsor/wechat-pay.png")))),
            SPONSOR_ALIPAY=>Ok(Some(Cow::Borrowed(include_bytes!("../assets/sponsor/alipay.jpg")))),
            _=>gpui_kit::assets::Assets.load(path),
        }
    }

    /// 只枚举当前编译包实际含有的资源。
    fn list(&self, prefix: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(prefix)?;
        for path in [LOGO, SPONSOR_WECHAT, SPONSOR_ALIPAY, DEVICE_IPHONE, DEVICE_ANDROID_PHONE, DEVICE_IPAD, DEVICE_ANDROID_TABLET, DEVICE_WATCH] {
            if path.starts_with(prefix) { paths.push(path.into()); }
        }

        Ok(paths)
    }
}
