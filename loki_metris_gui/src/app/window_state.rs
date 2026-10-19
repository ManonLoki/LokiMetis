//! 主窗口恢复事务属于宿主交互；普通几何只在最终原生读回后交给偏好 worker。

use super::Shell;
use crate::native::windowing::{self, DesktopSnapshot, WindowHost};
use gpui_kit::{Context, Window};
use loki_metris_core::window_placement::*;
use std::time::Duration;

/// generation 使迟到的恢复 task 失效；隐藏/关闭不会提交迁屏中间反馈。
pub(super) struct MainWindowState {
    /// 主窗原生宿主；捕获失败时为 None。
    host: Option<WindowHost>,
    /// 恢复代次；新的恢复请求使旧的失效。
    generation: u64,
    /// 启动恢复在途，期间不保存几何。
    restoring: bool,
    /// 已同步的宿主几何修订号。
    revision: u64,
}

impl MainWindowState {
    /// 测试虚拟窗口或捕获失败不会生成伪原生状态。
    pub fn capture(window: &Window) -> Self {
        let host = WindowHost::capture(window).ok();
        Self { restoring: host.is_some(), host, generation: 0, revision: 0 }
    }
    /// 同一窗口只允许最后一次主动显示动作继续操作。
    fn begin(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.restoring = true;
        self.generation
    }
    /// 同线程校验后立即执行原生方法，中间不 await。
    fn current(&self, generation: u64) -> bool { self.restoring && self.generation == generation }
}

impl Shell {
    /// 首次拓扑失败时窗口仍沿旧入口可见；结束抑制，让后续真实事件可以重新捕获。
    pub(crate) fn release_initial_window_restore(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.window_state.restoring = false;
        self.remember_native_window(window, cx);
    }
    /// 原生服务只读取信号 revision，空闲轮询不进入 Window 更新栈。
    pub(crate) fn window_host(&self) -> Option<WindowHost> { self.window_state.host.clone() }
    /// resize/move 回调可能处于系统 zoom 动画中；统一等原生事件静默后再保存。
    pub(super) fn note_native_geometry_change(&self) {
        if let Some(host) = &self.window_state.host { host.geometry_changed(); }
    }
    /// 内容尺寸与原生最大化分开保存；隐藏、最小化及系统全屏过渡不覆盖普通记录。
    pub(super) fn remember_native_window(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.window_state.restoring { return; }
        self.capture_confirmed_window(window, cx, false);
    }

    /// 关闭与退出读取已完成的最新普通几何，不要求额外等待偏好合并时间。
    pub(super) fn remember_native_window_at_boundary(&mut self, window: &Window, cx: &mut Context<Self>) {
        if !self.window_state.restoring { self.capture_confirmed_window(window, cx, true); }
    }

    /// 显式显示前可为原先可见的普通窗建立检查点；不能捕获另一恢复事务的中间帧。
    fn capture_confirmed_window(&mut self, window: &Window, cx: &mut Context<Self>, boundary: bool) {
        if !window.is_visible()
            || self.window_state.host.as_ref().is_none_or(|host| host.transitioning()
                || if boundary { !host.boundary_geometry_ready() } else { host.settled_revision().is_none() }) { return; }
        let Ok(desktop) = windowing::capture_topology(cx) else { return; };
        let Ok(state) = windowing::capture_window(window, &desktop) else { return; };
        if state.transient { return; }
        let normal = state.normal.or_else(|| {
            let saved = self.preferences.window_state_v2.as_ref()?;
            let current = windowing::window_display_key(window, &desktop).ok()?;
            reconcile_maximized_window_display(&desktop.areas(), &current, saved, windowing::frame_insets(window).ok()?)
        });
        let Some(mut normal) = normal else { return; };
        normal.maximized = state.maximized;
        self.commit_normal_window(normal);
    }

    /// 旧字段只供兼容读取；新状态与它们作为同一几何快照提交。
    fn commit_normal_window(&mut self, normal: WindowStateV2) {
        if !normal.is_valid() { return; }
        let legacy = Some([normal.origin.x as f32, normal.origin.y as f32,
            normal.content_size.width as f32, normal.content_size.height as f32]);
        if self.preferences.window_state_v2.as_ref() == Some(&normal)
            && self.preferences.bounds == legacy && self.preferences.maximized == normal.maximized { return; }
        self.preferences.bounds = legacy;
        self.preferences.maximized = normal.maximized;
        self.preferences.window_state_v2 = Some(normal.clone());
        self.writer.save_window_v2(legacy, normal.maximized, normal);
    }

    /// NSWindow 信号只由已有40ms原生调度读取；没有变化时不读拓扑或写偏好。
    pub(crate) fn poll_native_window(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(host) = &self.window_state.host else { return; };
        let Some(revision) = host.settled_revision() else { return; };
        if revision == self.window_state.revision { return; }
        self.window_state.revision = revision;
        self.remember_native_window(window, cx);
    }

    /// 隐藏取消未完成的显示动作，普通几何此前已保存或在这里读回。
    pub(crate) fn prepare_window_hide(&mut self, window: &Window, cx: &mut Context<Self>) -> Option<WindowHost> {
        self.remember_native_window(window, cx);
        self.cancel_window_restore()
    }

    /// close 回调已借用 Window 时只操作壳层租约，不能再次进入 handle.update。
    pub(crate) fn cancel_window_restore(&mut self) -> Option<WindowHost> {
        self.window_state.generation = self.window_state.generation.wrapping_add(1);
        self.window_state.restoring = false;
        self.window_state.host.clone()
    }

    /// 指针屏值来自主动显示入口，等待全屏退出时不追随鼠标。
    pub(crate) fn request_window_restore(&mut self, desktop: DesktopSnapshot, window: &Window, cx: &mut Context<Self>) {
        let Some(host) = self.window_state.host.clone() else { window.activate_window(); return; };
        let pending_zoom = host.begin_controlled_restore();
        let checkpoint = !pending_zoom && !self.window_state.restoring && window.is_visible() && !host.fullscreen() && !host.transitioning();
        let failure_revision = host.failure_revision();
        let generation = self.window_state.begin();
        let handle = self.native_window;
        let pointer = desktop.pointer;
        // 仅记录选屏关系，不把显示器身份或全局坐标写入日志。
        tracing::info!(event="window_restore_requested", pointer_available=pointer.is_some(),
            pointer_is_primary=desktop.displays.iter().any(|display| display.area.primary && pointer.as_deref()==Some(display.area.key.as_str())));
        cx.spawn(async move |weak, cx| {
            // 每一次原生操作前先复核活窗及 generation，随后释放全部 App/Window 借用。
            let current = |cx: &mut gpui_kit::AsyncApp| cx.update(|cx| handle.update(cx, |_, _, cx| {
                weak.update(cx, |shell, _| shell.window_state.current(generation)).unwrap_or(false)
            }).unwrap_or(false));
            let mut ready = false;
            for _ in 0..180 {
                if !current(cx) { return; }
                if host.failure_revision() != failure_revision {
                    finish_failed_restore(&weak, handle, generation, &host, cx); return;
                }
                if !host.transitioning() && !host.fullscreen() { ready = true; break; }
                host.exit_fullscreen();
                cx.background_executor().timer(Duration::from_millis(16)).await;
            }
            if !ready || !current(cx) { finish_failed_restore(&weak, handle, generation, &host, cx); return; }
            if checkpoint {
                if !wait_for_geometry(&weak, handle, generation, &host, failure_revision, false, cx).await {
                    finish_failed_restore(&weak, handle, generation, &host, cx); return;
                }
                cx.update(|cx| { let _ = handle.update(cx, |_, window, cx| {
                    let _ = weak.update(cx, |shell, cx| {
                        if shell.window_state.current(generation) { shell.capture_confirmed_window(window, cx, false); }
                    });
                }); });
            }
            if host.normalize().is_err() || !wait_for_geometry(&weak, handle, generation, &host, failure_revision, true, cx).await {
                finish_failed_restore(&weak, handle, generation, &host, cx); return;
            }
            let prepared = cx.update(|cx| handle.update(cx, |_, window, cx| {
                weak.update(cx, |shell, cx| {
                    if !shell.window_state.current(generation) { return None; }
                    let mut desktop = windowing::capture_topology(cx).ok()?;
                    desktop.pointer = pointer;
                    let placement = shell.main_placement(window, &desktop)?;
                    Some((desktop, placement))
                }).ok().flatten()
            }).ok().flatten());
            let Some((desktop, placement)) = prepared else {
                finish_failed_restore(&weak, handle, generation, &host, cx); return;
            };
            tracing::info!(event="window_restore_selected", target_is_pointer=desktop.pointer.as_deref()==Some(placement.display_key.as_str()),
                target_is_primary=desktop.display(&placement.display_key).is_some_and(|display| display.area.primary));
            if !current(cx) { return; }
            if host.apply_normal(&desktop, &placement).is_err() {
                finish_failed_restore(&weak, handle, generation, &host, cx); return;
            }
            if !wait_for_geometry(&weak, handle, generation, &host, failure_revision, false, cx).await {
                finish_failed_restore(&weak, handle, generation, &host, cx); return;
            }
            let normal = cx.update(|cx| handle.update(cx, |_, window, cx| {
                windowing::capture_topology(cx).ok().and_then(|desktop| windowing::capture_normal(window, &desktop).ok())
            }).ok().flatten());
            let Some(mut normal) = normal else {
                finish_failed_restore(&weak, handle, generation, &host, cx); return;
            };
            if !current(cx) { return; }
            if placement.maximized { host.maximize(); }
            host.show();
            if !wait_for_geometry(&weak, handle, generation, &host, failure_revision, false, cx).await {
                finish_failed_restore(&weak, handle, generation, &host, cx); return;
            }
            if !current(cx) { return; }
            let actual = cx.update(|cx| handle.update(cx, |_, window, cx| {
                let desktop = windowing::capture_topology(cx).ok()?;
                if host.maximized() {
                    let current = windowing::window_display_key(window, &desktop).ok()?;
                    reconcile_maximized_window_display(&desktop.areas(), &current, &normal, windowing::frame_insets(window).ok()?)
                } else { windowing::capture_normal(window, &desktop).ok() }
            }).ok().flatten());
            let Some(actual) = actual else { finish_failed_restore(&weak, handle, generation, &host, cx); return; };
            normal = actual;
            cx.update(|cx| {
                let _ = handle.update(cx, |_, _, cx| {
                    let _ = weak.update(cx, |shell, _| {
                        if !shell.window_state.current(generation) { return; }
                        shell.window_state.restoring = false;
                        shell.window_state.revision = host.revision();
                        shell.commit_normal_window(normal);
                    });
                });
                cx.activate(true);
            });
        }).detach();
    }

    /// 目标屏样式提供真实边框；保存状态缺失时仅保留合法旧内容尺寸。
    fn main_placement(&self, window: &Window, desktop: &DesktopSnapshot) -> Option<WindowPlacement> {
        let areas = desktop.areas();
        let insets = windowing::frame_insets(window).ok()?;
        let legacy = self.preferences.bounds.and_then(|bounds| migrate_legacy_main_window_state(&areas,
            WindowDipPoint { x: bounds[0].into(), y: bounds[1].into() },
            WindowDipSize { width: bounds[2].into(), height: bounds[3].into() }, insets, self.preferences.maximized));
        let current = windowing::window_display_key(window, desktop).ok();
        restore_window_placement(&areas, WindowDisplaySelection {
            pointer: desktop.pointer.as_deref(),
            current: current.as_deref(),
        }, WindowPlacementRequest {
            role: WindowPlacementRole::Main,
            saved: self.preferences.window_state_v2.as_ref().or(legacy.as_ref()),
            legacy_content_size: self.preferences.bounds.map(|bounds| WindowDipSize { width: bounds[2].into(), height: bounds[3].into() }),
            legacy_maximized: self.preferences.maximized,
            frame_insets: insets,
        })
    }
}

/// 原生动画分阶段完成；不持有 App/Window 借用，不把固定延时本身当作成功。
async fn wait_for_geometry(weak: &gpui_kit::WeakEntity<Shell>, handle: gpui_kit::AnyWindowHandle,
    generation: u64, host: &WindowHost, failure_revision: u64, require_normal: bool, cx: &mut gpui_kit::AsyncApp) -> bool {
    for _ in 0..120 {
        // 至少让原生事件循环处理一次，否则 zoom 请求后的旧标志仍可能看似稳定。
        cx.background_executor().timer(Duration::from_millis(16)).await;
        let current = cx.update(|cx| handle.update(cx, |_, _, cx| weak.update(cx, |shell, _|
            shell.window_state.current(generation)).unwrap_or(false)).unwrap_or(false));
        if !current || host.failure_revision() != failure_revision { return false; }
        // Show 可在上一段 zoom 的 isZoomed 尚未变真时到达；终态变真后再请求一次 unzoom。
        if require_normal && host.maximized() && !host.fullscreen() && !host.transitioning()
            && host.settled_revision().is_some() {
            if host.normalize().is_err() { return false; }
            continue;
        }
        if !host.fullscreen() && !host.transitioning() && host.settled_revision().is_some()
            && (!require_normal || !host.maximized()) { return true; }
    }
    false
}

/// 失败保留最后已确认状态；仍给用户显示原窗口的恢复入口。
fn finish_failed_restore(weak: &gpui_kit::WeakEntity<Shell>, handle: gpui_kit::AnyWindowHandle,
    generation: u64, host: &WindowHost, cx: &mut gpui_kit::AsyncApp) {
    let current = cx.update(|cx| handle.update(cx, |_, _, cx| weak.update(cx, |shell, _|
        shell.window_state.current(generation)).unwrap_or(false)).unwrap_or(false));
    if !current { return; }
    tracing::warn!(event="window_restore_failed"); host.show();
    cx.update(|cx| { let _ = weak.update(cx, |shell, _| {
        if shell.window_state.current(generation) {
            shell.window_state.restoring = false; shell.window_state.revision = host.revision();
        }
    }); });
}
