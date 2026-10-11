//! 相同窗口的精确阴影共用一份像素；当前与退休 lease 才是需求，绘制 Arc 不参与计数。

use super::raster::{Pixels, ShadowSample};
use gpui_kit::{App, Global, RenderImage, Subscription, Window, WindowId};
use std::{cell::RefCell, collections::HashMap, rc::{Rc, Weak}, sync::Arc};

/// 长轴只使用真正参与像素生成的尺寸；比例、圆角和连续样本均保留完整位值。
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct Key {
    pub width: u32,
    pub height: u32,
    pub scale: u32,
    pub radius: u32,
    sample: [u64; 4],
    bgra: [u8; 3],
}

impl Key {
    /// 不量化动态 CSS 参数；不同动画历史只有实际样本完全一致时才能共享。
    pub fn new(dimensions: (u32, u32), scale: f32, radius: f32, sample: ShadowSample) -> Self {
        Self {
            width: dimensions.0, height: dimensions.1,
            scale: scale.to_bits(), radius: radius.to_bits(),
            sample: [sample.sigma.to_bits(), sample.spread.to_bits(), sample.dy.to_bits(), sample.alpha.to_bits()],
            bgra: sample.bgra,
        }
    }
}

/// 弱缓存不拥有纹理，最后一个 lease 退出时删除精确代际的键和 atlas 条目。
struct Pool {
    window: WindowId,
    images: HashMap<Key, Weak<Lease>>,
}

/// 一个强引用代表一个当前或退休需求；RenderImage 的临时绘制引用不延长需求。
pub(super) struct Lease {
    pub key: Key,
    pub image: Arc<RenderImage>,
    pool: Rc<RefCell<Pool>>,
}

impl Lease {
    /// 只在最后需求退出时清退图集；A→B→A 重取不会被迟到的旧退休槽删除。
    pub fn release(self: Rc<Self>, window: &mut Window) {
        if Rc::strong_count(&self) != 1 { return; }
        let mut pool = self.pool.borrow_mut();
        debug_assert_eq!(pool.window, window.window_handle().window_id());
        if pool.images.get(&self.key).and_then(Weak::upgrade)
            .is_some_and(|entry| entry.image.id == self.image.id)
        {
            pool.images.remove(&self.key);
        }
        let _ = window.drop_image(self.image.clone());
    }
}

/// 每个 App 隔离窗口集合；弱 pool 不因关闭或无材质窗口而保留像素历史。
struct Registry {
    windows: HashMap<WindowId, Weak<RefCell<Pool>>>,
    _closed: Subscription,
}
impl Global for Registry {}

/// 首个阴影绑定一次关闭清理，外部仍持有旧 view 时也不能重新借到已关闭窗口缓存。
fn registry(cx: &mut App) -> &mut Registry {
    if !cx.has_global::<Registry>() {
        let closed = cx.on_window_closed(|cx, window| {
            let pool = cx.global_mut::<Registry>().windows.remove(&window).and_then(|pool| pool.upgrade());
            if let Some(pool) = pool { pool.borrow_mut().images.clear(); }
        });
        cx.set_global(Registry { windows: HashMap::new(), _closed: closed });
    }
    cx.global_mut::<Registry>()
}

/// 命中直接取得需求；未命中才运行 raster，缓存大小受当前和单代退休需求数约束。
pub(super) fn acquire(key: Key, window: &Window, cx: &mut App, generate: impl FnOnce() -> Pixels) -> Rc<Lease> {
    let registry = registry(cx);
    registry.windows.retain(|_, pool| pool.strong_count() > 0);
    let window = window.window_handle().window_id();
    let pool = registry.windows.get(&window).and_then(Weak::upgrade).unwrap_or_else(|| {
        let pool = Rc::new(RefCell::new(Pool { window, images: HashMap::new() }));
        registry.windows.insert(window, Rc::downgrade(&pool));
        pool
    });
    if let Some(lease) = pool.borrow().images.get(&key).and_then(Weak::upgrade) { return lease; }
    let lease = Rc::new(Lease { key, image: generate().into_image(), pool: pool.clone() });
    pool.borrow_mut().images.insert(key, Rc::downgrade(&lease));
    lease
}

#[cfg(test)]
#[path = "shadow_shared_tests.rs"]
mod tests;
