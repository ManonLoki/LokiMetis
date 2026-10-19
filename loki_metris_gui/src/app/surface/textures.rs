//! 每个可见卡片仅持有当前材质和一代待回收图，不按时间点或指针位置累计纹理。

use super::raster::Pixels;
use gpui_kit::{Bounds, Corners, Pixels as LogicalPixels, RenderImage, Window, point, px, size};
use std::sync::Arc;
use std::rc::Rc;
use super::shadow_shared::Lease;

/// 普通纹理由单个 owner 释放，共享阴影由最后一个当前或退休 lease 释放。
enum RetiredImage {
    /// 单个 owner 独占的纹理。
    Owned(Arc<RenderImage>),
    /// 共享阴影的 lease。
    Shared(Rc<Lease>),
}
impl RetiredImage {
    /// 同一角色的固定槽统一走对应所有权合同，不把绘制 Arc 当成材质需求。
    fn release(self, window: &mut Window) {
        match self {
            Self::Owned(image) => { let _ = window.drop_image(image); }
            Self::Shared(lease) => lease.release(window),
        }
    }
}

/// 固定角色的纹理槽使上一代待回收资源数量始终有界。
#[derive(Clone, Copy)]
pub(super) enum TextureRole {
    /// 阴影。
    Shadow = 0,
    /// 聚光径向主体。
    Radial = 1,
    /// 左上圆角。
    Corner0 = 2,
    /// 右上圆角。
    Corner1 = 3,
    /// 左下圆角。
    Corner2 = 4,
    /// 右下圆角。
    Corner3 = 5,
    /// 边框与顶边。
    Rim = 6,
}

impl TextureRole {
    /// 四个圆角各自使用固定槽，不随指针坐标新增缓存键。
    pub fn corner(index: usize) -> Self {
        [Self::Corner0, Self::Corner1, Self::Corner2, Self::Corner3][index]
    }
}

/// 回收队列每种角色最多一张；后帧回调只捕获owner弱引用，不捕获纹理历史。
#[derive(Default)]
pub(super) struct Retired {
    /// 按纹理角色索引的待回收纹理。
    images: [Option<RetiredImage>; 7],
}

/// 共用九切片图片绘制：UV保留完整源图，矩形片段只决定目标裁切范围。
pub(super) fn paint_sliced(
    bounds: Bounds<LogicalPixels>,
    xs: &[super::raster::Slice],
    ys: &[super::raster::Slice],
    image: Arc<RenderImage>,
    window: &mut Window,
) {
    let scale = f64::from(window.scale_factor());
    let dimensions = image.size(0);
    let full_width = f64::from(dimensions.width.0);
    let full_height = f64::from(dimensions.height.0);
    for x in xs {
        for y in ys {
            let sx = (x.target_end - x.target_start) / (x.source_end - x.source_start);
            let sy = (y.target_end - y.target_start) / (y.source_end - y.source_start);
            let at = |x: f64, y: f64| {
                bounds.origin + point(px((x / scale) as f32), px((y / scale) as f32))
            };
            let target = Bounds::from_corners(
                at(x.target_start, y.target_start),
                at(x.target_end, y.target_end),
            );
            let source = Bounds::new(
                at(
                    x.target_start - x.source_start * sx,
                    y.target_start - y.source_start * sy,
                ),
                size(
                    px((full_width * sx / scale) as f32),
                    px((full_height * sy / scale) as f32),
                ),
            );
            let _ = window.paint_image(target, source, Corners::default(), image.clone(), 0, false);
        }
    }
}

impl Retired {
    /// 同一角色在回调之前再次更新时，先释放更早的退休图，避免连续重绘累计。
    pub fn push(&mut self, role: TextureRole, image: Arc<RenderImage>, window: &mut Window) {
        if let Some(previous) = self.images[role as usize].replace(RetiredImage::Owned(image)) {
            previous.release(window);
        }
    }

    /// 共享阴影同样只退休一代，但其它卡片仍持需求时不能删除它们的图集条目。
    pub fn push_shared_shadow(&mut self, lease: Rc<Lease>, window: &mut Window) {
        if let Some(previous) = self.images[TextureRole::Shadow as usize].replace(RetiredImage::Shared(lease)) {
            previous.release(window);
        }
    }

    /// 已绘制帧之后移除上一代atlas项；CPU引用同时随槽清空释放。
    pub fn clear(&mut self, window: &mut Window) {
        for image in &mut self.images {
            if let Some(image) = image.take() {
                image.release(window);
            }
        }
    }

    /// 没有退休图时不安排只为清理而产生的额外帧。
    pub fn is_empty(&self) -> bool {
        self.images.iter().all(Option::is_none)
    }
}

/// 相同实际采样值复用图像ID；替换、主题切换与显示比例改变至多退休一代。
pub(super) struct Texture<K> {
    /// 当前纹理的缓存键。
    pub key: Option<K>,
    /// 当前纹理。
    pub image: Option<Arc<RenderImage>>,
}

impl<K> Default for Texture<K> {
    /// 首绘没有像素或GPU条目，离屏元素保持零分配。
    fn default() -> Self {
        Self {
            key: None,
            image: None,
        }
    }
}

impl<K: PartialEq> Texture<K> {
    /// 仅真正键变化时生成像素，静止状态与普通重绘不会重新上传。
    pub fn ensure(
        &mut self,
        key: K,
        role: TextureRole,
        retired: &mut Retired,
        window: &mut Window,
        generate: impl FnOnce() -> Pixels,
    ) -> Arc<RenderImage> {
        if self.key.as_ref() != Some(&key) {
            if let Some(old) = self.image.take() {
                retired.push(role, old, window);
            }
            self.image = Some(generate().into_image());
            self.key = Some(key);
        }
        self.image.as_ref().expect("有效材质键拥有当前纹理").clone()
    }

    /// 不再使用的角色进入有界退休槽，立即清除键防止迟到帧借用旧几何。
    pub fn retire(&mut self, role: TextureRole, retired: &mut Retired, window: &mut Window) {
        self.key = None;
        if let Some(image) = self.image.take() {
            retired.push(role, image, window);
        }
    }

    /// 隐藏或owner释放时同步删除当前GPU条目，不等待新的窗口帧。
    pub fn clear(&mut self, window: &mut Window) {
        self.key = None;
        if let Some(image) = self.image.take() {
            let _ = window.drop_image(image);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{
        AppContext, Context, Corners, IntoElement, Render, TestAppContext, WindowOptions, canvas,
        prelude::*, px, test::TestWindowExt,
    };

    /// 探针只上传一个4×4纹理，使atlas断言来自实际paint_image调用。
    struct Probe {
        image: Arc<RenderImage>,
    }

    impl Render for Probe {
        /// 使用完整图片边界绘制，测试不依赖任何业务页面。
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let image = self.image.clone();
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    window
                        .paint_image(bounds, bounds, Corners::default(), image, 0, false)
                        .unwrap();
                },
            )
            .size(px(4.))
        }
    }

    /// 极小有效像素供生命周期测试使用，不创建文件或异步图片请求。
    fn pixels() -> Pixels {
        Pixels {
            width: 4,
            height: 4,
            bytes: vec![255; 4 * 4 * 4],
        }
    }

    /// 同键零生成，替换只退休一代；显式clear真实移除GPU条目。
    #[gpui_kit::test]
    fn cache_reuses_static_image_and_releases_atlas_entries(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let mut texture = Texture::<u32>::default();
        let mut retired = Retired::default();
        let (handle, probe) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                let image = texture.ensure(0, TextureRole::Shadow, &mut retired, window, pixels);
                cx.new(|_| Probe { image })
            })
            .unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let first = texture.image.as_ref().unwrap().clone();
            assert!(window.has_image_atlas_entry(&first));
            let same = texture.ensure(0, TextureRole::Shadow, &mut retired, window, || {
                panic!("静止重绘不可重新生成")
            });
            assert_eq!(first.id, same.id);
            let next = texture.ensure(1, TextureRole::Shadow, &mut retired, window, pixels);
            probe.update(cx, |probe, cx| {
                probe.image = next.clone();
                cx.notify();
            });
            window.render_frame(cx);
            assert!(window.has_image_atlas_entry(&first));
            assert!(window.has_image_atlas_entry(&next));
            retired.clear(window);
            assert!(!window.has_image_atlas_entry(&first));
            assert!(window.has_image_atlas_entry(&next));
            for key in 2..100 {
                texture.ensure(key, TextureRole::Shadow, &mut retired, window, pixels);
                assert_eq!(
                    retired
                        .images
                        .iter()
                        .filter(|image| image.is_some())
                        .count(),
                    1
                );
            }
            texture.clear(window);
            retired.clear(window);
            assert!(!window.has_image_atlas_entry(&next));
            assert!(retired.is_empty());
        })
        .unwrap();
    }
}
