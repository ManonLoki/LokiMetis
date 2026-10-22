//! 本地产品示意图只消费当前认证会话的系统平台与终端形态。

use crate::assets;
use gpui_kit::assets::IconName;
use gpui_kit::{AnyElement, IntoElement, ObjectFit, Role, SharedString,
    base::TestSupportExt as _, component::Icon, div, img, prelude::*, px};
use loki_metris_core::devices::link::{DevicePlatform, DeviceFormFactor};

/// 仅按设备上报的平台与形态选择示意图；缺少任一事实时使用中性图标。
pub(super) fn artwork_path(platform: Option<DevicePlatform>, form_factor: Option<DeviceFormFactor>) -> Option<&'static str> {
    match (platform, form_factor) {
        (Some(DevicePlatform::Ios), Some(DeviceFormFactor::Phone)) => Some(assets::DEVICE_IPHONE),
        (Some(DevicePlatform::Ios), Some(DeviceFormFactor::Tablet)) => Some(assets::DEVICE_IPAD),
        (Some(DevicePlatform::Android), Some(DeviceFormFactor::Phone)) => Some(assets::DEVICE_ANDROID_PHONE),
        (Some(DevicePlatform::Android), Some(DeviceFormFactor::Tablet)) => Some(assets::DEVICE_ANDROID_TABLET),
        _ => None,
    }
}

/// 固定图像槽和比例，解码前后尺寸一致；未知平台用中性图标，不猜测系统。
pub(super) fn device_artwork(id: &str, path: Option<&'static str>, label: String) -> AnyElement {
    artwork_frame(format!("device-artwork-{id}"), path, label, 96., 112.)
}

/// 附属Watch使用固定40×48小图槽；插图不代表型号，也不生成独立设备入口。
pub(super) fn watch_artwork(id: &str, label: String) -> AnyElement {
    artwork_frame(format!("device-watch-artwork-{id}"), Some(assets::DEVICE_WATCH), label, 40., 48.)
}

/// 手机和Watch共用同一图像槽结构，只由调用方决定标识和尺寸。
fn artwork_frame(id: String, path: Option<&'static str>, label: String, width: f32, height: f32) -> AnyElement {
    let frame = div().id(SharedString::from(id))
        .test_support().role(Role::Image).aria_label(label).w(px(width)).h(px(height))
        .flex_shrink_0().flex().items_center().justify_center();
    match path {
        Some(path) => frame.child(img(path).w(px(width)).h(px(height)).object_fit(ObjectFit::Contain)),
        None => frame.child(Icon::new(IconName::MonitorSmartphone).size(px(40.))),
    }.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 图片只由明确上报决定，旧终端缺少形态时不再默认成手机。
    #[test]
    fn illustrations_use_reported_platform_and_form_factor_only() {
        for (platform, phone, tablet) in [
            (DevicePlatform::Ios, assets::DEVICE_IPHONE, assets::DEVICE_IPAD),
            (DevicePlatform::Android, assets::DEVICE_ANDROID_PHONE, assets::DEVICE_ANDROID_TABLET),
        ] {
            assert_eq!(artwork_path(Some(platform), Some(DeviceFormFactor::Phone)), Some(phone));
            assert_eq!(artwork_path(Some(platform), Some(DeviceFormFactor::Tablet)), Some(tablet));
            assert_eq!(artwork_path(Some(platform), None), None);
        }
        assert_eq!(artwork_path(None, Some(DeviceFormFactor::Tablet)), None);
        assert_eq!(artwork_path(None, None), None);
    }

    /// 手机、平板和Watch实际嵌入图必须能够通过同一资源源读取、解码并保留透明边缘。
    #[test]
    fn bundled_device_images_are_decodable_transparent_and_registered() {
        use gpui_kit::AssetSource;
        let source = assets::Assets;
        let listed = source.list("app-devices/").unwrap();
        assert_eq!(listed.len(), 5);
        for path in [assets::DEVICE_IPHONE, assets::DEVICE_ANDROID_PHONE, assets::DEVICE_IPAD, assets::DEVICE_ANDROID_TABLET, assets::DEVICE_WATCH] {
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
