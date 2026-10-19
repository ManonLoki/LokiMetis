//! Windows 主窗使用冻结指针及 V2 普通几何；Win32 修改与 GPUI 借用严格分离。

use super::Shell;
use crate::native::windowing::{self, DesktopSnapshot, WindowHost};
use gpui_kit::{Context, Window};
use loki_metris_core::window_placement::*;

/// 单窗代次撤销迟到恢复；未读回确认的几何不能进入偏好 worker。
pub(super) struct MainWindowState {
    /// 主窗原生宿主；捕获失败时为 None。
    host: Option<WindowHost>,
    /// 恢复代次；新的恢复请求使旧的失效。
    generation: u64,
    /// 启动恢复在途，期间不保存几何。
    restoring: bool,
    /// 原生几何尚未读回确认，期间不保存几何。
    geometry_unconfirmed: bool,
    /// 已同步的宿主几何修订号。
    revision: u64,
}

impl MainWindowState {
    /// 首帧隐藏创建期间抑制保存，避免默认几何覆盖已有正常状态。
    pub fn capture(window: &Window) -> Self {
        let host = WindowHost::capture(window).ok();
        Self { restoring: host.is_some(), geometry_unconfirmed: host.is_some(), host, generation: 0, revision: 0 }
    }

    /// 每个原生步骤之前复核唯一有效显示意图。
    fn current(&self, generation: u64) -> bool { self.restoring && self.generation == generation }
}

impl Shell {
    /// 拓扑不可读时保留普通可见入口，让后续明确唤起可以重试。
    pub(crate) fn release_initial_window_restore(&mut self, _window: &Window, _cx: &mut Context<Self>) {
        self.window_state.restoring = false;
        // 无法核验启动矩形时继续保护已有偏好，待下次明确显示事务成功再解除。
    }

    /// 唯一宿主由原生调度持有同一信号，不复制独立几何状态。
    pub(crate) fn window_host(&self) -> Option<WindowHost> { self.window_state.host.clone() }

    /// 系统 move/resize 回调只触发静默合并，不在同步消息中读写偏好。
    pub(super) fn note_native_geometry_change(&self) { if let Some(host) = &self.window_state.host { host.geometry_changed(); } }

    /// 关闭与退出保存真实普通内容尺寸；最小化/隐藏/全屏不会覆盖锚点。
    pub(super) fn remember_native_window_at_boundary(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.window_state.restoring || self.window_state.geometry_unconfirmed || window.is_fullscreen() { return; }
        let Ok(desktop) = windowing::capture_topology(cx) else { return; };
        let Ok(state) = windowing::capture_window(window, &desktop) else { return; };
        if state.transient { return; }
        let normal = state.normal.or_else(|| {
            let saved = self.preferences.window_state_v2.as_ref()?;
            let current = windowing::window_display_key(window, &desktop).ok()?;
            reconcile_maximized_window_display(&desktop.areas(), &current, saved, windowing::frame_insets(window).ok()?)
        });
        if let Some(mut normal) = normal {
            normal.maximized = state.maximized;
            if let Ok(insets) = windowing::frame_insets(window) { self.commit_normal_window(normal, &desktop, insets); }
        }
    }

    /// 新旧几何字段由 writer 作为一个快照合并，不能覆盖语言或主题等并行偏好。
    fn commit_normal_window(&mut self, normal: WindowStateV2, desktop: &DesktopSnapshot, insets: WindowFrameInsets) {
        if !normal.is_valid() { return; }
        let Some(display) = desktop.display(&normal.display_key) else { return; };
        let legacy = Some([(display.global_origin.x / display.area.scale + normal.origin.x + insets.left) as f32,
            (display.global_origin.y / display.area.scale + normal.origin.y + insets.top) as f32,
            normal.content_size.width as f32, normal.content_size.height as f32]);
        if self.preferences.window_state_v2.as_ref() == Some(&normal) && self.preferences.maximized == normal.maximized { return; }
        self.preferences.bounds = legacy;
        self.preferences.maximized = normal.maximized;
        self.preferences.window_state_v2 = Some(normal.clone());
        self.writer.save_window_v2(legacy, normal.maximized, normal);
    }

    /// 已有40ms调度仅在静默后处理新 bounds 版本，空闲不触发额外 IO。
    pub(crate) fn poll_native_window(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(revision) = self.window_state.host.as_ref().and_then(WindowHost::settled_revision) else { return; };
        if revision == self.window_state.revision { return; }
        self.window_state.revision = revision;
        self.remember_native_window_at_boundary(window, cx);
    }

    /// 隐藏前建立普通检查点并撤销在途恢复。
    pub(crate) fn prepare_window_hide(&mut self, window: &Window, cx: &mut Context<Self>) -> Option<WindowHost> {
        self.remember_native_window_at_boundary(window, cx);
        self.cancel_window_restore()
    }

    /// hide/show/退出竞态中旧任务不能继续修改同一 HWND。
    pub(crate) fn cancel_window_restore(&mut self) -> Option<WindowHost> {
        self.window_state.generation = self.window_state.generation.wrapping_add(1);
        self.window_state.restoring = false;
        self.window_state.host.clone()
    }

    /// 主动唤起在入口冻结鼠标屏；全屏退出、迁屏及显示均在借用之外串行执行。
    pub(crate) fn request_window_restore(&mut self, desktop: DesktopSnapshot, window: &Window, cx: &mut Context<Self>) {
        let Some(host) = self.window_state.host.clone() else { window.activate_window(); return; };
        self.remember_native_window_at_boundary(window, cx);
        self.window_state.generation = self.window_state.generation.wrapping_add(1);
        let generation = self.window_state.generation;
        self.window_state.restoring = true;
        self.window_state.geometry_unconfirmed = true;
        let handle = self.native_window;
        let pointer = desktop.pointer;
        cx.spawn(async move |weak, cx| {
            let live = |cx: &mut gpui_kit::AsyncApp| cx.update(|cx| handle.update(cx, |_, _, cx|
                weak.update(cx, |shell, _| shell.window_state.current(generation)).unwrap_or(false)).unwrap_or(false));
            let mut fullscreen_requested = false;
            let mut ready = false;
            for _ in 0..120 {
                if !live(cx) { return; }
                ready = cx.update(|cx| handle.update(cx, |_, window, _| {
                    if window.is_fullscreen() && !fullscreen_requested { window.toggle_fullscreen(); fullscreen_requested = true; }
                    !window.is_fullscreen()
                }).unwrap_or(false));
                if ready { break; }
                cx.background_executor().timer(std::time::Duration::from_millis(16)).await;
            }
            if !ready || !live(cx) || host.normalize().is_err() { fail(&weak, handle, generation, &host, cx); return; }
            let prepared = cx.update(|cx| handle.update(cx, |_, window, cx| {
                weak.update(cx, |shell, cx| {
                    let mut desktop = windowing::capture_topology(cx).ok()?;
                    desktop.pointer = pointer;
                    let plan = shell.main_placement(window, &desktop)?;
                    Some((desktop, plan))
                }).ok().flatten()
            }).ok().flatten());
            let Some((desktop, plan)) = prepared else { fail(&weak, handle, generation, &host, cx); return; };
            if !live(cx) { return; }
            if host.prime_display(&desktop, &plan).is_err() { fail(&weak, handle, generation, &host, cx); return; }
            if !live(cx) { return; }
            let corrected = cx.update(|cx| handle.update(cx, |_, window, cx| weak.update(cx, |shell, cx| {
                let mut refreshed = windowing::capture_topology(cx).ok()?;
                refreshed.pointer = desktop.pointer.clone();
                let plan = shell.main_placement(window, &refreshed)?;
                Some((refreshed, plan))
            }).ok().flatten()).ok().flatten());
            let Some((desktop, plan)) = corrected else { fail(&weak, handle, generation, &host, cx); return; };
            if !live(cx) { return; }
            let Ok(plan) = host.apply_auxiliary(&desktop, &plan) else { fail(&weak, handle, generation, &host, cx); return; };
            let mut normal = None;
            for _ in 0..8 {
                cx.background_executor().timer(std::time::Duration::from_millis(16)).await;
                if !live(cx) { return; }
                normal = cx.update(|cx| handle.update(cx, |_, window, _| windowing::capture_normal(window, &desktop).ok()).ok().flatten());
                if normal.as_ref().is_some_and(|normal| matches(normal, &plan)) { break; }
            }
            let Some(mut normal) = normal.filter(|normal| matches(normal, &plan)) else { fail(&weak, handle, generation, &host, cx); return; };
            let insets = cx.update(|cx| handle.update(cx, |_, window, _| windowing::frame_insets(window).ok()).ok().flatten());
            let Some(insets) = insets else { fail(&weak, handle, generation, &host, cx); return; };
            if !live(cx) { return; }
            if plan.maximized { host.maximize(); }
            host.show();
            cx.background_executor().timer(std::time::Duration::from_millis(16)).await;
            if !live(cx) { return; }
            normal.maximized = host.maximized();
            if normal.maximized != plan.maximized { fail(&weak, handle, generation, &host, cx); return; }
            let actual = cx.update(|cx| handle.update(cx, |_, window, cx| {
                let refreshed = windowing::capture_topology(cx).ok()?;
                let current = windowing::window_display_key(window, &refreshed).ok()?;
                if current != plan.display_key { return None; }
                let actual = if normal.maximized {
                    reconcile_maximized_window_display(&refreshed.areas(), &current, &normal, insets)?
                } else { windowing::capture_normal(window, &refreshed).ok()? };
                Some((refreshed, actual))
            }).ok().flatten());
            let Some((desktop, normal)) = actual else { fail(&weak, handle, generation, &host, cx); return; };
            cx.update(|cx| {
                let _ = weak.update(cx, |shell, _| {
                    if !shell.window_state.current(generation) { return; }
                    shell.window_state.restoring = false;
                    shell.window_state.geometry_unconfirmed = false;
                    shell.window_state.revision = host.revision();
                    shell.commit_normal_window(normal, &desktop, insets);
                });
                tracing::info!(event="window_restore_verified");
                cx.activate(true);
            });
        }).detach();
    }

    /// 旧全局客户区位置仅唯一解释后迁移，坏位置仍保留有效内容尺寸和最大化。
    fn main_placement(&self, window: &Window, desktop: &DesktopSnapshot) -> Option<WindowPlacement> {
        let insets = windowing::frame_insets(window).ok()?;
        let legacy = self.preferences.bounds.and_then(|bounds| windowing::legacy_main(desktop, bounds, insets, self.preferences.maximized));
        let current = windowing::window_display_key(window, desktop).ok();
        restore_window_placement(&desktop.areas(), WindowDisplaySelection { pointer: desktop.pointer.as_deref(), current: current.as_deref() },
            WindowPlacementRequest { role: WindowPlacementRole::Main, saved: self.preferences.window_state_v2.as_ref().or(legacy.as_ref()),
                legacy_content_size: self.preferences.bounds.map(|bounds| WindowDipSize { width: bounds[2].into(), height: bounds[3].into() }),
                legacy_maximized: self.preferences.maximized, frame_insets: insets })
    }
}

/// 普通几何只在真实显示器与像素对齐目标都一致时提交。
fn matches(actual: &WindowStateV2, plan: &WindowPlacement) -> bool {
    let tolerance = 0.5 / actual.scale.max(1.);
    actual.display_key == plan.display_key && (actual.origin.x - plan.origin.x).abs() <= tolerance
        && (actual.origin.y - plan.origin.y).abs() <= tolerance
        && (actual.content_size.width - plan.content_size.width).abs() <= tolerance
        && (actual.content_size.height - plan.content_size.height).abs() <= tolerance
}

/// 失败保留旧可信偏好并显示可恢复窗口，不把经过等待写成定位成功。
fn fail(weak: &gpui_kit::WeakEntity<Shell>, handle: gpui_kit::AnyWindowHandle, generation: u64, host: &WindowHost, cx: &mut gpui_kit::AsyncApp) {
    let live = cx.update(|cx| handle.update(cx, |_, _, cx| weak.update(cx, |shell, _|
        shell.window_state.current(generation)).unwrap_or(false)).unwrap_or(false));
    if !live { return; }
    host.show();
    cx.update(|cx| { let _ = weak.update(cx, |shell, _| {
        if shell.window_state.current(generation) { shell.window_state.restoring = false; shell.window_state.revision = host.revision(); }
    }); });
    tracing::warn!(event="window_restore_failed");
}
