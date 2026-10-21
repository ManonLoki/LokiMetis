//! 本地产品示意图只服务展示，不把名称提示当作已认证的硬件型号。

use crate::assets;
use gpui_kit::assets::IconName;
use gpui_kit::{AnyElement, IntoElement, ObjectFit, Role, SharedString,
    base::TestSupportExt as _, component::Icon, div, img, prelude::*, px};
use loki_metris_core::devices::link::DevicePlatform;

/// 已认证平台优先；名称中的明确机型词只选择装饰图，不影响平台文字、信任或在线状态。
pub(super) fn artwork_path(platform: Option<DevicePlatform>, name: &str) -> Option<&'static str> {
    let name = name.to_lowercase();
    let words: Vec<_> = name.split(|c: char| !c.is_alphanumeric()).collect();
    let tablet = name.contains("ipad") || name.contains("平板")
        || words.iter().any(|word| matches!(*word, "pad" | "tablet"));
    let platform = platform.or_else(|| {
        if name.contains("iphone") || name.contains("ipad") { Some(DevicePlatform::Ios) }
        else if words.contains(&"android") { Some(DevicePlatform::Android) }
        else { None }
    });
    match (platform, tablet) {
        (Some(DevicePlatform::Ios), false) => Some(assets::DEVICE_IPHONE),
        (Some(DevicePlatform::Ios), true) => Some(assets::DEVICE_IPAD),
        (Some(DevicePlatform::Android), false) => Some(assets::DEVICE_ANDROID_PHONE),
        (Some(DevicePlatform::Android), true) => Some(assets::DEVICE_ANDROID_TABLET),
        (None, _) => None,
    }
}

/// 固定图像槽和比例，解码前后尺寸一致；未知平台用中性图标，不猜测系统。
pub(super) fn device_artwork(id: &str, path: Option<&'static str>, label: String) -> AnyElement {
    let mut frame = div().id(SharedString::from(format!("device-artwork-{id}")))
        .test_support().role(Role::Image).aria_label(label).h(px(184.)).w_full()
        .flex_shrink_0().flex().items_center().justify_center();
    frame = if let Some(path) = path {
        let ratio = if path == assets::DEVICE_ANDROID_TABLET { 1.5 } else { 1. };
        frame.child(img(path).w(px(184. * ratio)).h(px(184.)).max_w_full().max_h(px(184.))
            .min_w_0().min_h_0().aspect_ratio(ratio).object_fit(ObjectFit::Contain))
    } else {
        frame.child(Icon::new(IconName::MonitorSmartphone).size(px(64.)))
    };
    frame.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 图片选择不根据未知名称伪造平台，认证平台也不能被名称中的系统词覆盖。
    #[test]
    fn illustrations_use_platform_and_explicit_name_hints_only() {
        assert_eq!(artwork_path(Some(DevicePlatform::Ios), "My phone"), Some(assets::DEVICE_IPHONE));
        assert_eq!(artwork_path(Some(DevicePlatform::Ios), "Manon's iPad"), Some(assets::DEVICE_IPAD));
        assert_eq!(artwork_path(Some(DevicePlatform::Android), "Android tablet"), Some(assets::DEVICE_ANDROID_TABLET));
        assert_eq!(artwork_path(Some(DevicePlatform::Android), "Work phone"), Some(assets::DEVICE_ANDROID_PHONE));
        assert_eq!(artwork_path(Some(DevicePlatform::Android), "iPhone"), Some(assets::DEVICE_ANDROID_PHONE));
        assert_eq!(artwork_path(None, "iPad"), Some(assets::DEVICE_IPAD));
        assert_eq!(artwork_path(None, "Android Pad"), Some(assets::DEVICE_ANDROID_TABLET));
        assert_eq!(artwork_path(None, "Receiver"), None);
        assert_eq!(artwork_path(None, "Notepad"), None);
    }

    /// 四种实际嵌入图必须能够通过同一资源源读取、解码并保留透明边缘。
    #[test]
    fn bundled_device_images_are_decodable_transparent_and_registered() {
        use gpui_kit::AssetSource;
        let source = assets::Assets;
        let listed = source.list("app-devices/").unwrap();
        assert_eq!(listed.len(), 4);
        for path in [assets::DEVICE_IPHONE, assets::DEVICE_ANDROID_PHONE, assets::DEVICE_IPAD, assets::DEVICE_ANDROID_TABLET] {
            let bytes = source.load(path).unwrap().unwrap();
            let image = image::load_from_memory(&bytes).unwrap().to_rgba8();
            assert!(image.width() >= 512 && image.height() >= 512);
            assert_eq!(image.get_pixel(0, 0)[3], 0, "{path}必须保留透明背景");
            // 生成PNG主体可能为253/254的近不透明像素，不能把非255的有效设备误判为空图。
            let solid = image.pixels().filter(|pixel| pixel[3] >= 250).count();
            assert!(solid > (image.width() * image.height() / 10) as usize, "{path}必须含有实际设备主体");
            assert!(listed.iter().any(|listed| listed.as_ref() == path));
        }
    }
}
