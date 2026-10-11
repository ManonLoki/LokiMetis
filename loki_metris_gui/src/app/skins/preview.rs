//! Mantine Card.Section的负margin与图片视口圆角；静态皮肤原图沿用GPUI异步解码。

use gpui_kit::{
    AnyElement, App, Asset, Bounds, Corners, Image, ImageCacheError, ImageRoundedClip, ImageSource,
    ImgResourceLoader, IntoElement, ObjectFit, RenderImage, Role, SharedString, Window,
    base::TestSupportExt as _, canvas, div, point, prelude::*, px, size,
};
use std::{future::Future, sync::Arc};
use super::preview_residency::PreviewResidencies;

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
pub(super) fn skin_preview(source: ImageSource, id: SharedString, label: String, residencies: PreviewResidencies) -> AnyElement {
    div().id(id.clone()).test_support().absolute().inset_0().size_full().overflow_hidden()
        .role(Role::Image).aria_label(label).child(
            div().id(format!("{id}-image")).test_support()
                .absolute().left(px(-16.)).right(px(-16.)).top(px(-16.)).h(px(176.))
                .child(canvas(
                    move |_, window, cx| use_data(&source, &residencies, window, cx).and_then(Result::ok),
                    |inner, image, window, _| {
                        let Some(image) = image.filter(|image| image.frame_count() > 0) else { return; };
                        let outer = Bounds::new(inner.origin + point(px(16.), px(16.)),
                            size(inner.size.width - px(32.), px(160.)));
                        let fitted = ObjectFit::Cover.get_bounds(inner, image.size(0));
                        let _ = window.paint_image_with_rounded_clip(inner, fitted, ImageRoundedClip {
                            bounds: outer,
                            corner_radii: Corners { top_left: px(19.), top_right: px(19.),
                                bottom_left: px(0.), bottom_right: px(0.) },
                        }, image, 0, false);
                    },
                ).size_full()),
        ).into_any_element()
}
