//! 穿透只改变原生输入属性，恢复入口始终在浮窗以外。

use super::{Controller, Manager, host};
use gpui_kit::{App, Bounds, Pixels};

/// 托盘消费可比较的内存状态，不轮询OS或克隆图片/用量快照。
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct State {
    pub enabled: bool,
    pub busy: bool,
    pub available: bool,
    pub confirmed: Option<bool>,
}

impl Controller {
    /// 当前overlay固定拥有此状态；全窗输入切换不重建播放器或刷新bounds。
    pub(super) fn apply_mouse_passthrough(&mut self, cx: &mut App) -> anyhow::Result<()> {
        let handle = self.overlay.ok_or_else(|| anyhow::anyhow!("pet overlay unavailable"))?;
        let enabled = self.settings.pet_window.mouse_passthrough;
        handle.update(cx, |_, window, _| host::set_mouse_passthrough(window, enabled, &mut self.mouse_state))?
    }
    /// 失败时只回滚本事务触碰的宿主属性；回滚失败明确保留未知状态和错误。
    pub(super) fn rollback_native(&mut self, geometry: bool, bounds: Option<Bounds<Pixels>>, cx: &mut App) -> bool {
        let result = if geometry {
            let result = self.apply_geometry(false, cx);
            self.restore_bounds(bounds, cx);
            result
        } else {
            self.apply_mouse_passthrough(cx)
        };
        if let Err(error) = result {
            self.mouse_state.confirmed = None;
            tracing::warn!(%error, "pet native preference rollback failed");
            false
        } else { true }
    }
}

/// 不依赖桌宠点击的独立设置入口，复用现有owner与显示事务。
pub(crate) fn open_settings(cx: &mut App) {
    if let Some(owner) = cx.try_global::<Manager>().map(|manager| manager.0.clone()) {
        owner.update(cx, |owner, cx| owner.open_settings(cx));
    }
}

/// 菜单取消优先：偏好或实际原生状态任一已开启时都提交关闭。
pub(crate) fn toggle_mouse_passthrough(cx: &mut App) {
    if let Some(owner) = cx.try_global::<Manager>().map(|manager| manager.0.clone()) {
        owner.update(cx, |owner, cx| {
            let enabled = owner.settings.pet_window.mouse_passthrough || owner.mouse_state.confirmed() != Some(false);
            owner.change(loki_metris_core::PetPreferenceChange::MousePassthrough(!enabled), cx);
        });
    }
}

/// 已确认值负责菜单勾选，忙碌和可用性独立表示，未知状态不能宣称成功。
pub(crate) fn mouse_passthrough_state(cx: &App) -> State {
    cx.try_global::<Manager>().map(|manager| {
        let owner = manager.0.read(cx);
        State { enabled: owner.settings.pet_window.mouse_passthrough, busy: owner.busy, available: owner.overlay.is_some() && !owner.quitting, confirmed: owner.mouse_state.confirmed() }
    }).unwrap_or_default()
}
