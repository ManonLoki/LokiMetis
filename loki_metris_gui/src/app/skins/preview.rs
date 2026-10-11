//! Mantine Card.Section的负margin与图片视口圆角；静态皮肤原图沿用GPUI异步解码。

use gpui_kit::{
    AnyElement, App, Asset, Corners, Image, ImageCacheError, ImageSource,
    ImgResourceLoader, IntoElement, ObjectFit, RenderImage, Role, SharedString, Window,
    base::TestSupportExt as _, canvas, div, prelude::*, px,
};
#[cfg(not(target_os = "macos"))]
use gpui_kit::{Bounds, ImageRoundedClip, point, size};
use std::{future::Future, sync::Arc};
use super::preview_residency::PreviewResidencies;

/// Card.Section 的 --card-padding：左右扩出、顶部上移的像素数。
const SECTION_BLEED: f32 = 16.;
/// 预览视口露出的高度（h176 扣除顶部上移后）。
const SECTION_HEIGHT: f32 = 160.;
/// 外卡片 r20 减 border 1 后的内缘圆角。
#[cfg(not(target_os = "macos"))]
const SECTION_RADIUS: f32 = 19.;

/// 皮肤预览外层容器的统一入口：macOS 由父级圆角裁切后代绘制与 Hitbox，
/// 其它平台由 `skin_preview` 自带圆角裁切，故此处不做处理。
pub(super) fn section_frame<T: Styled>(frame: T) -> T {
    #[cfg(target_os = "macos")]
    let frame = frame.clip_rounded_children();
    frame
}

/// GPUI内部ImageDecoder未公开，薄Asset直接复用Image::to_image_data，render不执行解码。
pub(super) struct PreviewImageDecoder;
impl Asset for PreviewImageDecoder {
    type Source = Arc<Image>;
    type Output = Result<Arc<RenderImage>, ImageCacheError>;
    /// 由原生Asset owner在后台执行，和原生Img同样保留Loading／Failed／Ready三态。
    fn load(source: Self::Source, cx: &mut App) -> impl Future<Output = Self::Output> + Send + 'static {
        let renderer = cx.svg_renderer();
        async move { source.to_image_data(renderer).map_err(Into::into) }
    }
}

/// 生产只传原始Arc<Image>，已解码测试和已有自定义／资源来源继续沿用GPUI加载能力。
fn use_data(source: &ImageSource, residencies: &PreviewResidencies, window: &mut Window, cx: &mut App)
    -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
    match source {
        ImageSource::Image(image) => {
            let result = window.use_asset::<PreviewImageDecoder>(image, cx);
            if let Some(Ok(decoded)) = &result { residencies.observed(image.id(), decoded); }
            result
        }
        ImageSource::Render(image) => Some(Ok(image.clone())),
        ImageSource::Resource(resource) => window.use_asset::<ImgResourceLoader>(resource, cx),
        ImageSource::Custom(load) => load(window, cx),
    }
}

/// p:0并不重置Card的--card-padding：Section左右扩16px、顶部移16px，h176只露160px。
/// 辅助Image语义仍属于外层；外卡片r20／border1的内缘r19固定圆心，不随图像视口偏移。
/// 调用方须用 [`section_frame`] 包装外层容器，macOS 才能由父级完成圆角裁切。
pub(super) fn skin_preview(source: ImageSource, id: SharedString, label: String, residencies: PreviewResidencies) -> AnyElement {
    div().id(id.clone()).test_support().absolute().inset_0().size_full().overflow_hidden()
        .role(Role::Image).aria_label(label).child(
            div().id(format!("{id}-image")).test_support()
                .absolute().left(px(-SECTION_BLEED)).right(px(-SECTION_BLEED)).top(px(-SECTION_BLEED)).h(px(SECTION_HEIGHT + SECTION_BLEED))
                .child(canvas(
                    move |_, window, cx| use_data(&source, &residencies, window, cx).and_then(Result::ok),
                    |inner, image, window, _| {
                        let Some(image) = image.filter(|image| image.frame_count() > 0) else { return; };
                        let fitted = ObjectFit::Cover.get_bounds(inner, image.size(0));
                        // 父R19已负责真实裁切；同一图片再乘自身R19会把AA边缘平方。
                        #[cfg(target_os = "macos")]
                        let _ = window.paint_image(inner, fitted, Corners::all(px(0.)), image, 0, false);
                        #[cfg(not(target_os = "macos"))]
                        {
                            let outer = Bounds::new(inner.origin + point(px(SECTION_BLEED), px(SECTION_BLEED)),
                                size(inner.size.width - px(2. * SECTION_BLEED), px(SECTION_HEIGHT)));
                            let _ = window.paint_image_with_rounded_clip(inner, fitted, ImageRoundedClip {
                                bounds: outer,
                                corner_radii: Corners { top_left: px(SECTION_RADIUS), top_right: px(SECTION_RADIUS),
                                    bottom_left: px(0.), bottom_right: px(0.) },
                            }, image, 0, false);
                        }
                    },
                ).size_full()),
        ).into_any_element()
}
