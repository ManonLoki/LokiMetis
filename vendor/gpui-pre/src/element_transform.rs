// 正统一缩放与平移仅作用于元素绘制、命中及辅助边界，不改变布局或纹理缓存键。

use super::*;
use crate::Primitive;

/// 窗口DIP中的仿射变换；正比例保持矩形裁切和绘制排序可表达。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ElementTransform {
    scale: f32,
    offset: Point<Pixels>,
}

impl Default for ElementTransform {
    /// 静态元素不改变几何，也不为绘制增加分配。
    fn default() -> Self {
        Self { scale: 1., offset: Point::default() }
    }
}

impl ElementTransform {
    /// 子变换先执行，再进入当前祖先坐标系。
    fn compose(self, child: Self) -> Self {
        Self {
            scale: self.scale * child.scale,
            offset: child.offset * self.scale + self.offset,
        }
    }

    /// 反变换继承的矩形遮罩，让局部裁切计算继续使用未缩放坐标。
    pub(super) fn inverse(self) -> Self {
        Self { scale: 1. / self.scale, offset: self.offset * (-1. / self.scale) }
    }

    /// 布局矩形保持原值，只把用于命中或辅助树的副本映射到窗口坐标。
    fn bounds(self, bounds: Bounds<Pixels>) -> Bounds<Pixels> {
        Bounds::new(bounds.origin * self.scale + self.offset, bounds.size.map(|value| value * self.scale))
    }

    /// 设备图元沿用同一变换；DIP平移只乘一次当前屏幕比例。
    fn scaled_point(self, value: Point<ScaledPixels>, device_scale: f32) -> Point<ScaledPixels> {
        point(
            ScaledPixels(value.x.0 * self.scale + self.offset.x.0 * device_scale),
            ScaledPixels(value.y.0 * self.scale + self.offset.y.0 * device_scale),
        )
    }

    /// 保持图元纹理坐标不变，缩放其真实设备矩形。
    pub(super) fn scaled_bounds(self, bounds: Bounds<ScaledPixels>, device_scale: f32) -> Bounds<ScaledPixels> {
        Bounds::new(
            self.scaled_point(bounds.origin, device_scale),
            bounds.size.map(|value| ScaledPixels(value.0 * self.scale)),
        )
    }

    /// SVG原矩阵共轭到新坐标；单位字形矩阵保持单位，不重新栅格化。
    fn sprite_matrix(self, matrix: &mut TransformationMatrix, device_scale: f32) {
        let dx = self.offset.x.0 * device_scale;
        let dy = self.offset.y.0 * device_scale;
        matrix.translation = [
            self.scale * matrix.translation[0] + dx
                - matrix.rotation_scale[0][0] * dx - matrix.rotation_scale[0][1] * dy,
            self.scale * matrix.translation[1] + dy
                - matrix.rotation_scale[1][0] * dx - matrix.rotation_scale[1][1] * dy,
        ];
    }

    /// 在Scene记录排序、缓存副本之前变换图元，不改GPU结构、atlas条目或祖先圆角链。
    fn primitive(self, primitive: &mut Primitive, device_scale: f32) {
        let mask = |value: &mut ContentMask<ScaledPixels>| {
            value.bounds = self.scaled_bounds(value.bounds, device_scale);
        };
        match primitive {
            Primitive::Quad(value) => {
                value.bounds = self.scaled_bounds(value.bounds, device_scale);
                value.corner_radii = value.corner_radii.map(|v| ScaledPixels(v.0 * self.scale));
                value.border_widths = value.border_widths.map(|v| ScaledPixels(v.0 * self.scale));
                mask(&mut value.content_mask);
            }
            Primitive::Shadow(value) => {
                value.bounds = self.scaled_bounds(value.bounds, device_scale);
                value.element_bounds = self.scaled_bounds(value.element_bounds, device_scale);
                value.corner_radii = value.corner_radii.map(|v| ScaledPixels(v.0 * self.scale));
                value.element_corner_radii = value.element_corner_radii.map(|v| ScaledPixels(v.0 * self.scale));
                value.blur_radius = ScaledPixels(value.blur_radius.0 * self.scale);
                mask(&mut value.content_mask);
            }
            Primitive::Underline(value) => {
                value.bounds = self.scaled_bounds(value.bounds, device_scale);
                value.thickness = ScaledPixels(value.thickness.0 * self.scale);
                mask(&mut value.content_mask);
            }
            Primitive::Path(value) => {
                value.bounds = self.scaled_bounds(value.bounds, device_scale);
                mask(&mut value.content_mask);
                for vertex in &mut value.vertices {
                    vertex.xy_position = self.scaled_point(vertex.xy_position, device_scale);
                    mask(&mut vertex.content_mask);
                }
            }
            Primitive::MonochromeSprite(value) => {
                value.bounds = self.scaled_bounds(value.bounds, device_scale);
                self.sprite_matrix(&mut value.transformation, device_scale);
                mask(&mut value.content_mask);
            }
            Primitive::SubpixelSprite(value) => {
                value.bounds = self.scaled_bounds(value.bounds, device_scale);
                self.sprite_matrix(&mut value.transformation, device_scale);
                mask(&mut value.content_mask);
            }
            Primitive::PolychromeSprite(value) => {
                value.bounds = self.scaled_bounds(value.bounds, device_scale);
                value.rounded_clip_bounds = self.scaled_bounds(value.rounded_clip_bounds, device_scale);
                value.corner_radii = value.corner_radii.map(|v| ScaledPixels(v.0 * self.scale));
                mask(&mut value.content_mask);
            }
            Primitive::Surface(value) => {
                value.bounds = self.scaled_bounds(value.bounds, device_scale);
                mask(&mut value.content_mask);
            }
        }
    }
}

impl Window {
    /// 以原border-box中心进行正统一缩放和平移；调用方在prepaint/paint使用同一参数。
    /// 布局不变，继承遮罩反映射后再参与局部裁切；缓存和延迟绘制保留相同变换。
    /// 祖先圆角链保持窗口坐标；本接口的按钮后代不创建新的父级圆角裁切域。
    pub fn with_element_transform<R>(
        &mut self,
        center: Point<Pixels>,
        scale: f32,
        translation: Point<Pixels>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        assert!(scale.is_finite() && scale > 0.);
        let local = ElementTransform { scale, offset: center * (1. - scale) + translation };
        let old = self.begin_element_transform(self.element_transform.compose(local));
        let result = f(self);
        self.end_element_transform(old);
        result
    }

    /// 辅助树及原生命中共享同一窗口DIP几何，布局系统不消费此结果。
    pub(crate) fn element_transform_bounds(&self, bounds: Bounds<Pixels>) -> Bounds<Pixels> {
        self.element_transform.bounds(bounds)
    }

    /// 缓存键仅保存值，不持有按钮Entity或纹理。
    pub(crate) fn element_transform_key(&self) -> (f32, Point<Pixels>) {
        (self.element_transform.scale, self.element_transform.offset)
    }

    /// 延迟元素恢复捕获时的绝对变换，不能把已完成的Scene回放再次缩放。
    pub(super) fn begin_element_transform(&mut self, transform: ElementTransform) -> ElementTransform {
        self.invalidator.debug_assert_paint_or_prepaint();
        let old = self.element_transform;
        let relative = old.inverse().compose(transform);
        let mask = ContentMask { bounds: relative.inverse().bounds(self.content_mask().bounds) };
        self.content_mask_stack.push(mask);
        self.element_transform = transform;
        old
    }

    /// 离开子树时恢复祖先变换和遮罩，不影响兄弟元素。
    pub(super) fn end_element_transform(&mut self, previous: ElementTransform) {
        self.element_transform = previous;
        self.content_mask_stack.pop();
    }

    /// 全部即时绘制统一进入此入口；Scene保存已变换图元，缓存回放沿原入口一次复用。
    pub(super) fn insert_transformed_primitive(&mut self, primitive: impl Into<Primitive>) {
        let mut primitive = primitive.into();
        if self.element_transform != ElementTransform::default() {
            self.element_transform.primitive(&mut primitive, self.scale_factor());
        }
        self.next_frame.scene.insert_primitive(primitive);
    }
}
