//! 辅助窗宿主桥接只处理原生属性和工作区，不拥有领域设置。

/// 单一浮窗的原生穿透确认和样式位所有权，不保存跨窗口裸句柄。
#[derive(Default)]
pub struct MousePassthroughState {
    pub(super) confirmed: Option<bool>,
    #[cfg(all(windows, not(test)))]
    pub(super) original_bits: Option<isize>,
    #[cfg(test)]
    pub(super) calls: usize,
    #[cfg(test)]
    pub(super) fail_next: bool,
}
impl MousePassthroughState {
    /// None表示尚未确认或回滚失败，不能在托盘宣称本机已应用。
    pub fn confirmed(&self) -> Option<bool> { self.confirmed }
}

#[cfg(all(target_os = "macos", not(test)))]
mod macos;
#[cfg(all(target_os = "macos", not(test)))]
pub use macos::*;
#[cfg(all(windows, not(test)))]
mod windows;
#[cfg(all(windows, not(test)))]
pub use windows::*;

#[cfg(all(not(any(windows, target_os = "macos")), not(test)))]
mod fallback {
    use gpui_kit::{Bounds, Window, point, px, size};
    use loki_metris_core::PetWindowPreferences;
    /// 未支持平台不得接受并保存虚假的穿透成功。
    pub fn set_mouse_passthrough(_: &Window, _: bool, state: &mut super::MousePassthroughState) -> anyhow::Result<()> {
        state.confirmed = None;
        anyhow::bail!("mouse passthrough unsupported")
    }
    /// 当前产品目标平台之外不假报原生辅助窗能力。
    pub fn set_visible(_: &Window, _: bool) -> anyhow::Result<()> {
        anyhow::bail!("auxiliary windows unsupported")
    }
    /// 未支持平台不应用假属性。
    pub fn apply(_: &Window, _: PetWindowPreferences, _: bool) -> anyhow::Result<()> {
        anyhow::bail!("auxiliary windows unsupported")
    }
    /// 未支持平台不移动窗口。
    pub fn move_to(_: &Window, _: f64, _: f64) -> anyhow::Result<()> {
        anyhow::bail!("auxiliary windows unsupported")
    }
    /// 只提供尺寸算法回退，不声明实际屏幕区域。
    pub fn work_area(_: &Window) -> Bounds<gpui_kit::Pixels> {
        Bounds::new(point(px(0.), px(0.)), size(px(1440.), px(900.)))
    }
    /// 未支持平台只能提供已有GPUI位置。
    pub fn bounds(window: &Window) -> Bounds<gpui_kit::Pixels> {
        window.bounds()
    }
    /// 未支持平台不提交假几何。
    pub fn set_geometry(_: &mut Window, _: Bounds<gpui_kit::Pixels>) -> anyhow::Result<()> {
        anyhow::bail!("auxiliary windows unsupported")
    }
    /// 非目标平台没有可恢复的原生显示器。
    pub fn restoration_area(
        window: &Window,
        _: Option<loki_metris_core::PetOverlayPosition>,
        _: PetWindowPreferences,
    ) -> (Bounds<gpui_kit::Pixels>, f64, bool) {
        (work_area(window), f64::from(window.scale_factor()), false)
    }
    /// GPUI可用时直接交给当前事件。
    pub fn start_drag(window: &Window, _: &gpui_kit::App) {
        window.start_window_move();
    }
}
#[cfg(all(not(any(windows, target_os = "macos")), not(test)))]
pub use fallback::*;

#[cfg(test)]
mod test_host {
    use gpui_kit::{Bounds, Window, point, px, size};
    use loki_metris_core::PetWindowPreferences;
    /// 显式测试适配器记录事务与可控失败；不代表OS鼠标投递验证。
    pub fn set_mouse_passthrough(_: &Window, enabled: bool, state: &mut super::MousePassthroughState) -> anyhow::Result<()> {
        state.calls += 1;
        if std::mem::take(&mut state.fail_next) {
            anyhow::bail!("injected native mouse passthrough failure")
        }
        state.confirmed = Some(enabled);
        Ok(())
    }
    /// 假平台只验证GPUI实体/窗口时序，实际原生属性由Computer Use检查。
    pub fn set_visible(_: &Window, _: bool) -> anyhow::Result<()> {
        Ok(())
    }
    /// 测试平台没有OS句柄，属性调用由显式测试适配器接收。
    pub fn apply(_: &Window, _: PetWindowPreferences, _: bool) -> anyhow::Result<()> {
        Ok(())
    }
    /// 假平台不模拟OS移动，位置持久化另以显式边界事件验证。
    pub fn move_to(_: &Window, _: f64, _: f64) -> anyhow::Result<()> {
        Ok(())
    }
    /// 固定工作区使尺寸约束可重复。
    pub fn work_area(_: &Window) -> Bounds<gpui_kit::Pixels> {
        Bounds::new(point(px(0.), px(0.)), size(px(1440.), px(1080.)))
    }
    /// 几何取自假平台真实GPUI窗口。
    pub fn bounds(window: &Window) -> Bounds<gpui_kit::Pixels> {
        window.bounds()
    }
    /// 测试平台resize同步更新真实窗口布局，位置由专门持久化断言覆盖。
    pub fn set_geometry(
        window: &mut Window,
        bounds: Bounds<gpui_kit::Pixels>,
    ) -> anyhow::Result<()> {
        window.resize(bounds.size);
        Ok(())
    }
    /// 测试使用固定工作区和逻辑scale，实际多屏转换交由原生验证。
    pub fn restoration_area(
        window: &Window,
        stored: Option<loki_metris_core::PetOverlayPosition>,
        _: PetWindowPreferences,
    ) -> (Bounds<gpui_kit::Pixels>, f64, bool) {
        (
            work_area(window),
            f64::from(window.scale_factor()),
            stored.is_some(),
        )
    }
    /// 单元测试不进入原生模态拖动，实际行为由Computer Use检查。
    pub fn start_drag(_: &Window, _: &gpui_kit::App) {}
}
#[cfg(test)]
pub use test_host::*;
