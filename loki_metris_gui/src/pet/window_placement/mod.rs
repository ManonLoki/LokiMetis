//! 显示意图冻结入口屏，原生迁屏读回前不把中间矩形当作用户调整。

#[cfg(all(any(target_os = "macos", windows), not(test)))]
mod transaction;
#[cfg(any(all(any(target_os = "macos", windows), not(test)), test))]
mod plan;
#[cfg(test)]
mod tests;

use super::Controller;
#[cfg(any(not(any(target_os = "macos", windows)), test))]
use super::host;
use gpui_kit::{AnyWindowHandle, App, Context, Entity};

/// 一个显式显示动作的入口事实；首次设置回执只续用该意图，不重新读取鼠标。
#[derive(Clone)]
pub(super) struct Request {
    generation: u64,
    #[cfg(all(any(target_os = "macos", windows), not(test)))]
    pointer: Option<String>,
    revisions: Option<(u64, u64)>,
    visible: bool,
    reveal: bool,
}

impl Request {
    /// 捕获失败保留确定回退，不把当前窗口屏冒充真实鼠标屏。
    fn capture(generation: u64, cx: &App) -> Self {
        #[cfg(any(not(any(target_os = "macos", windows)), test))]
        let _ = cx;
        Self {
            generation,
            #[cfg(all(any(target_os = "macos", windows), not(test)))]
            pointer: crate::native::windowing::capture_desktop(cx).ok().and_then(|desktop| desktop.pointer),
            revisions: None,
            visible: true,
            reveal: true,
        }
    }
}

impl Controller {
    /// 默认展示与首次真实偏好共用冻结的入口，隐藏或退出会撤销整个租约。
    pub(super) fn request_display(&mut self, cx: &mut Context<Self>) {
        self.cancel_display();
        self.display_request = Some(Request::capture(self.display_generation, cx));
        self.launch_display(cx);
    }

    /// 每个原生事务具有独立代次；替换事务继承入口屏和仍有效的显示意图。
    fn launch_display(&mut self, cx: &mut Context<Self>) {
        if self.quitting { return; }
        if self.overlay.is_none() {
            self.display_applying = false;
            self.retry_after = Some(cx.background_executor().now() + std::time::Duration::from_secs(2));
            return;
        }
        let Some(mut request) = self.display_request.clone() else { return; };
        self.display_generation = self.display_generation.wrapping_add(1);
        self.display_task.take();
        request.generation = self.display_generation;
        self.display_request = Some(request.clone());
        self.display_applying = true;
        self.display_geometry_unconfirmed = true;
        let owner = cx.entity().clone();
        let handle = self.overlay;
        cx.defer(move |cx| begin(owner, request, handle, cx));
    }

    /// 取消只结束原生显示工作，位置队列及已接受的偏好回执仍由原 owner 保留。
    pub(super) fn cancel_display(&mut self) {
        self.display_generation = self.display_generation.wrapping_add(1);
        self.display_task.take();
        self.display_request = None;
        self.display_applying = false;
    }

    /// 首次读取继承未完成的显示意图；隐藏后的回执只恢复属性，不能再次显示。
    pub(super) fn accept_loaded_geometry(&mut self, revisions: (u64, u64), cx: &mut Context<Self>) {
        if let Some(request) = &mut self.display_request {
            request.revisions = Some(revisions);
            self.launch_display(cx);
            return;
        }
        // 普通回填不产生新的 reveal 意图，也不重新捕获鼠标屏。
        self.display_request = Some(Request {
            generation: self.display_generation, revisions: Some(revisions),
            visible: self.visible, reveal: false,
            #[cfg(all(any(target_os = "macos", windows), not(test)))]
            pointer: None,
        });
        self.launch_display(cx);
    }
}

/// 原生操作之前重新核对精确活窗，返回后不保留任何 GPUI 借用。
fn current(owner: &Entity<Controller>, generation: u64, handle: Option<AnyWindowHandle>, visible: bool, cx: &App) -> bool {
    let owner = owner.read(cx);
    !owner.quitting && owner.display_generation == generation && owner.visible == visible
        && owner.overlay == handle && handle.is_some()
}

/// 延迟入口过期时零原生操作；虚拟宿主与 Windows 保持现有几何桥接。
fn begin(owner: Entity<Controller>, request: Request, handle: Option<AnyWindowHandle>, cx: &mut App) {
    if !current(&owner, request.generation, handle, request.visible, cx) { return; }
    #[cfg(all(any(target_os = "macos", windows), not(test)))]
    transaction::begin(owner, request, handle.unwrap(), cx);
    #[cfg(any(not(any(target_os = "macos", windows)), test))]
    {
        let applied = owner.update(cx, |owner, cx| owner.apply_geometry(!owner.geometry_restored, cx).is_ok());
        if !current(&owner, request.generation, handle, request.visible, cx) { return; }
        let shown = if request.reveal || !request.visible {
            handle.unwrap().update(cx, |_, window, _| host::set_visible(window, request.visible)).is_ok_and(|result| result.is_ok())
        } else { true };
        finish(&owner, &request, handle.unwrap(), applied && shown, request.reveal && shown, cx);
    }
}

/// 原生显示完成后才放开反馈；失败保留入口供有界轮询重试，不消费成功版本。
fn finish(owner: &Entity<Controller>, request: &Request, handle: AnyWindowHandle, verified: bool, revealed: bool, cx: &mut App) {
    if !current(owner, request.generation, Some(handle), request.visible, cx) { return; }
    let restored = owner.update(cx, |owner, cx| {
        owner.display_applying = false;
        owner.display_task.take();
        // 已显示的默认浮窗不因迟到设置回填再次抢焦点，冻结屏仍保留到首次成功。
        if revealed
            && let Some(request) = &mut owner.display_request { request.reveal = false; }
        if verified && (owner.settings_ready || owner.geometry_restored) {
            owner.geometry_restored = true;
            owner.display_geometry_unconfirmed = false;
            if let Some((agent, pet)) = request.revisions {
                owner.observed.0 = agent;
                owner.observed.1 = pet;
            }
            owner.display_request = None;
        } else if !verified {
            owner.retry_after = Some(cx.background_executor().now() + std::time::Duration::from_secs(2));
        }
        if verified && owner.geometry_restored { owner.focus_first_populated(cx); }
        if !owner.busy { owner.next_pending(cx); }
        cx.notify();
        verified && owner.geometry_restored
    });
    if restored {
        // 先结束 owner 借用，再让正式 bounds 回调保存唯一最终位置。
        let _ = handle.update(cx, |_, window, cx| window.bounds_changed(cx));
    }
}

/// 普通用户拖动的原生读回更新可信锚点；调用方已排除未确认的程序迁屏。
pub(super) fn observed_state(window: &gpui_kit::Window, cx: &App) -> Option<loki_metris_core::window_placement::WindowStateV2> {
    #[cfg(all(any(target_os = "macos", windows), not(test)))]
    {
        let desktop = crate::native::windowing::capture_topology(cx).ok()?;
        crate::native::windowing::capture_normal(window, &desktop).ok()
    }
    #[cfg(any(not(any(target_os = "macos", windows)), test))]
    { let _ = (window, cx); None }
}

/// 隐藏也核对同一租约，hide→show 的旧 defer 不能迟到遮住新窗口。
pub(super) fn hide(owner: Entity<Controller>, generation: u64, handle: Option<AnyWindowHandle>, cx: &mut App) {
    if !current(&owner, generation, handle, false, cx) { return; }
    let handle = handle.unwrap();
    #[cfg(all(any(target_os = "macos", windows), not(test)))]
    {
        let host = handle.update(cx, |_, window, _| crate::native::windowing::WindowHost::capture(window)).ok().and_then(Result::ok);
        if !current(&owner, generation, Some(handle), false, cx) { return; }
        if let Some(host) = host { host.hide(); }
    }
    #[cfg(any(not(any(target_os = "macos", windows)), test))]
    { let _ = handle.update(cx, |_, window, _| host::set_visible(window, false)); }
}
