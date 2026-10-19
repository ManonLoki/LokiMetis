//! 变更 NSWindow 的方法只由前台 task 在 App/Window 借用之外调用。

use super::{DesktopSnapshot, macos};
use block2::RcBlock;
use gpui_kit::Window;
use loki_metris_core::window_placement::WindowPlacement;
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_app_kit::*;
use objc2_foundation::{NSNotificationCenter, NSNotification, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSValue, ns_string};
use std::{ptr::NonNull, rc::Rc, sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}}, time::Instant};

/// 通知只更新信号、目标外框和单调时间，不进入 GPUI，也不借用应用实体。
#[derive(Default)]
struct Signals {
    /// 窗口几何变化次数；轮询比较它决定是否同步窗口状态。
    revision: AtomicU64,
    /// key 窗口变化次数，用于刷新菜单可用性。
    focus_revision: AtomicU64,
    /// 原生窗口事务失败次数。
    failure_revision: AtomicU64,
    /// 全屏等系统过渡动画进行中，期间不读取几何。
    transition: AtomicBool,
    /// 用户正在拖动改变窗口大小，期间不读取几何。
    live_resize: AtomicBool,
    /// 几何变化后的静默期判定。
    geometry: Mutex<super::geometry_settle::GeometrySettle>,
    /// zoom/unzoom 目标外框保护，拦截中间帧。
    zoom: Mutex<super::geometry_settle::ZoomGuard>,
}

impl Signals {
    /// 短期锁只保护时间戳，锁内不调用 AppKit、GPUI 或用户闭包。
    fn changed(&self) {
        self.geometry.lock().unwrap_or_else(|error| error.into_inner()).changed(Instant::now());
        self.revision.fetch_add(1, Ordering::Relaxed);
    }
    /// 未变更或最后一帧已静默时允许单次读回。
    fn geometry_ready(&self) -> bool {
        self.geometry.lock().unwrap_or_else(|error| error.into_inner()).ready(Instant::now())
    }
}

/// 保留原生对象和有界观察租约；最后一个引用释放时注销。
struct Owner {
    /// 主窗的 NSWindow。
    window: Retained<NSWindow>,
    /// 与通知回调共享的几何信号。
    signals: Arc<Signals>,
    /// 已注册的 NSNotification 观察者，最后一个引用释放时逐个注销。
    observers: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
}

impl Drop for Owner {
    /// Owner 销毁时注销它登记过的全部系统通知观察者，避免窗口关闭后还有回调打到已释放的对象。
    fn drop(&mut self) {
        let center = NSNotificationCenter::defaultCenter();
        for observer in &self.observers {
            // 安全：这些 token 仅由本 Owner 注册；删除不调用 GPUI。
            let object = <ProtocolObject<dyn NSObjectProtocol> as AsRef<objc2::runtime::AnyObject>>::as_ref(observer);
            unsafe { center.removeObserver(object); }
        }
    }
}

/// 主线程局部句柄；Rc 防止跨线程移动，Retained 防止裸指针悬垂。
#[derive(Clone)]
pub(crate) struct WindowHost(Rc<Owner>);

impl WindowHost {
    /// 在活窗借用中捕获；原生观察者不替换 GPUI 自己的 delegate。
    pub fn capture(window: &Window) -> anyhow::Result<Self> {
        let window = macos::native(window)?;
        let signals = Arc::new(Signals::default());
        let center = NSNotificationCenter::defaultCenter();
        let mut observers = Vec::new();
        // 安全：AppKit 静态通知名有效；订阅对象是当前保留的 NSWindow。
        let names = unsafe { [
            (NSWindowDidMoveNotification, None, None), (NSWindowDidResizeNotification, None, None),
            (NSWindowDidChangeScreenNotification, None, None), (NSWindowDidDeminiaturizeNotification, None, None),
            (NSWindowWillEnterFullScreenNotification, Some(true), None),
            (NSWindowWillExitFullScreenNotification, Some(true), None),
            (NSWindowDidEnterFullScreenNotification, Some(false), None),
            (NSWindowDidExitFullScreenNotification, Some(false), None),
            (NSWindowWillStartLiveResizeNotification, None, Some(true)),
            (NSWindowDidEndLiveResizeNotification, None, Some(false)),
        ] };
        for (name, transition, live_resize) in names {
            let signals = signals.clone();
            let block = RcBlock::new(move |_: NonNull<NSNotification>| {
                if let Some(value) = transition { signals.transition.store(value, Ordering::Relaxed); }
                if let Some(value) = live_resize { signals.live_resize.store(value, Ordering::Relaxed); }
                if live_resize == Some(false) { signals.zoom.lock().unwrap_or_else(|error| error.into_inner()).clear(); }
                signals.changed();
            });
            // 安全：block 仅捕获 Send+Sync 信号与短期值锁；通知中心复制 block 并保留 token。
            observers.push(unsafe { center.addObserverForName_object_queue_usingBlock(
                Some(name), Some(&window), None, &block,
            ) });
        }
        // 焦点失效独立于几何静默窗口；原生回调只递增计数，不能同步借用GPUI。
        // SAFETY: 通知名是 AppKit 导出的不可变静态 NSString，读取外部静态不存在数据竞争。
        for name in unsafe { [NSWindowDidBecomeKeyNotification, NSWindowDidResignKeyNotification] } {
            let signals = signals.clone();
            let block = RcBlock::new(move |_: NonNull<NSNotification>| {
                signals.focus_revision.fetch_add(1, Ordering::Relaxed);
            });
            // 安全：通知名来自AppKit，object精确限定当前保留主窗，token由Owner统一注销。
            observers.push(unsafe { center.addObserverForName_object_queue_usingBlock(
                Some(name), Some(&window), None, &block,
            ) });
        }
        let failed = signals.clone();
        let block = RcBlock::new(move |_: NonNull<NSNotification>| {
            failed.transition.store(false, Ordering::Relaxed);
            failed.failure_revision.fetch_add(1, Ordering::Relaxed);
            failed.changed();
        });
        let name = NSString::from_str("LokiMetrisGPUIWindowFullScreenTransitionFailedNotification");
        // 安全：同一保留 NSWindow 限定私有失败事件；block 仅捕获原子状态。
        observers.push(unsafe { center.addObserverForName_object_queue_usingBlock(Some(&name), Some(&window), None, &block) });
        let zoom = signals.clone();
        let block = RcBlock::new(move |note: NonNull<NSNotification>| {
            // 安全：本进程受管 delegate 用 NSValue.valueWithRect 产生此私有载荷；仍检查对象类型与有限值。
            let target = unsafe { note.as_ref() }.userInfo()
                .and_then(|info| info.objectForKey(ns_string!("targetFrame")))
                .and_then(|value| value.downcast::<NSValue>().ok())
                // SAFETY: targetFrame 由 GPUI 以 NSRect 封装进 NSValue，按同一类型读取。
                .map(|value| native_frame(unsafe { value.rectValue() }));
            zoom.zoom.lock().unwrap_or_else(|error| error.into_inner()).begin(target);
            zoom.changed();
        });
        let name = NSString::from_str("LokiMetrisGPUIWindowWillZoomNotification");
        // 安全：精确限定保留的 NSWindow；回调只复制原生值，不触发 UI 或业务状态变化。
        observers.push(unsafe { center.addObserverForName_object_queue_usingBlock(Some(&name), Some(&window), None, &block) });
        Ok(Self(Rc::new(Owner { window, signals, observers })))
    }

    /// 由既有原生轮询按 revision 驱动读回，不增加计时器或持续磁盘写入。
    pub fn revision(&self) -> u64 { self.0.signals.revision.load(Ordering::Relaxed) }
    /// 动画/连续拖动期间保持待处理，静默后仅返回一次新的 revision。
    pub fn settled_revision(&self) -> Option<u64> {
        (!self.0.signals.live_resize.load(Ordering::Relaxed) && self.0.signals.geometry_ready() && !self.zoom_pending()).then(|| self.revision())
    }
    /// 关闭/退出不必丢弃刚完成的普通拖动，但全屏、live resize和缩放中间帧仍禁止捕获。
    pub fn boundary_geometry_ready(&self) -> bool {
        !self.transitioning() && !self.0.signals.live_resize.load(Ordering::Relaxed) && !self.zoom_pending()
    }
    /// 有目标才读取原生 frame；空闲路径不增加 AppKit 调用。
    fn zoom_pending(&self) -> bool {
        let snapshot = self.0.signals.zoom.lock().unwrap_or_else(|error| error.into_inner()).clone();
        if !snapshot.pending() { return false; }
        let frame = native_frame(self.0.window.frame());
        let tolerance = 0.5 / self.0.window.backingScaleFactor().max(1.);
        let mut guard = self.0.signals.zoom.lock().unwrap_or_else(|error| error.into_inner());
        if *guard != snapshot { return guard.pending(); }
        !guard.observe(frame, tolerance)
    }
    /// 新的明确恢复动作可以接管已中断的目标，但必须从旧可信状态恢复，不能建立中间检查点。
    pub fn begin_controlled_restore(&self) -> bool {
        let pending = self.zoom_pending();
        self.0.signals.zoom.lock().unwrap_or_else(|error| error.into_inner()).clear();
        self.geometry_changed();
        pending
    }
    /// GPUI bounds 可能早于 NSWindow 通知，先标记变化，不在这个回调内读回动画帧。
    pub fn geometry_changed(&self) { self.0.signals.changed(); }
    /// 失败终态使当前事务结束，下一次显式显示才允许重试。
    pub fn failure_revision(&self) -> u64 { self.0.signals.failure_revision.load(Ordering::Relaxed) }
    /// 过渡期间不能保存全屏的临时普通矩形。
    pub fn transitioning(&self) -> bool { self.0.signals.transition.load(Ordering::Relaxed) }
    /// AppKit 的真实全屏样式，用于完成通知后的状态复核。
    pub fn fullscreen(&self) -> bool { self.0.window.styleMask().contains(NSWindowStyleMask::FullScreen) }
    /// 仅请求退出；调用方等待 didExit 通知后才迁移窗口。
    pub fn exit_fullscreen(&self) { if self.fullscreen() && !self.transitioning() { self.0.window.toggleFullScreen(None); } }
    /// 先请求离开最小化/zoom；调用方等待普通状态静默后才能设置外框。
    pub fn normalize(&self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.fullscreen() && !self.transitioning(), "window_fullscreen_transition_required");
        let window = &self.0.window;
        if window.isMiniaturized() { self.geometry_changed(); window.deminiaturize(None); }
        if window.isZoomed() { self.geometry_changed(); window.zoom(None); }
        Ok(())
    }
    /// 普通外框先应用；最大化另一步执行，以便先读回普通几何。
    pub fn apply_normal(&self, desktop: &DesktopSnapshot, placement: &WindowPlacement) -> anyhow::Result<()> {
        anyhow::ensure!(!self.fullscreen() && !self.transitioning() && !self.maximized(), "window_normal_transition_required");
        self.apply_frame(desktop, placement)
    }
    /// 固定辅助窗在原生全局点边界对齐一次，返回严格读回所需的对齐后目标。
    /// 不采用主窗 zoom 判据；本机固定无边框窗也可能返回 isZoomed=true。
    #[cfg_attr(test, allow(dead_code, reason = "只有桌宠真实窗口路径调用，测试窗口没有原生宿主"))]
    pub fn apply_auxiliary(&self, desktop: &DesktopSnapshot, placement: &WindowPlacement) -> anyhow::Result<WindowPlacement> {
        anyhow::ensure!(!self.fullscreen() && !self.transitioning(), "window_fullscreen_transition_required");
        let mut frame = self.target_frame(desktop, placement)?;
        frame.origin.x = frame.origin.x.round();
        frame.origin.y = frame.origin.y.round();
        let display = desktop.display(&placement.display_key).ok_or_else(|| anyhow::anyhow!("window_target_removed"))?;
        let mut aligned = placement.clone();
        aligned.origin.x = frame.origin.x - display.global_origin.x;
        aligned.origin.y = desktop.global_top - display.global_origin.y - frame.origin.y - frame.size.height;
        self.set_frame(frame);
        Ok(aligned)
    }
    /// 真实外框换算只有一处；调用方按各自窗口样式判断能否定位。
    fn apply_frame(&self, desktop: &DesktopSnapshot, placement: &WindowPlacement) -> anyhow::Result<()> {
        self.set_frame(self.target_frame(desktop, placement)?);
        Ok(())
    }
    /// 内容尺寸与屏内位置只在这里转换为 AppKit 全局外框。
    fn target_frame(&self, desktop: &DesktopSnapshot, placement: &WindowPlacement) -> anyhow::Result<NSRect> {
        let display = desktop.display(&placement.display_key).ok_or_else(|| anyhow::anyhow!("window_target_removed"))?;
        let window = &self.0.window;
        let content = NSRect::new(NSPoint::new(0., 0.), NSSize::new(placement.content_size.width, placement.content_size.height));
        let frame_size = window.frameRectForContentRect(content).size;
        Ok(NSRect::new(NSPoint::new(
            display.global_origin.x + placement.origin.x,
            desktop.global_top - display.global_origin.y - placement.origin.y - frame_size.height,
        ), frame_size))
    }
    /// AppKit 回调可以同步进入 GPUI，故调用方必须已释放所有 GPUI 借用。
    fn set_frame(&self, frame: NSRect) {
        self.geometry_changed();
        self.0.window.setFrame_display(frame, true);
    }
    /// 必须在普通几何读回之后调用，避免保存系统 zoom 矩形作为普通尺寸。
    pub fn maximize(&self) { if !self.0.window.isZoomed() { self.geometry_changed(); self.0.window.zoom(None); } }
    /// 真实 zoom 状态决定最后保存标志，不能乐观宣称系统接受请求。
    pub fn maximized(&self) -> bool { self.0.window.isZoomed() }
    /// 无边框辅助窗的样式变更也可能同步回调，必须在全部 GPUI 借用之外执行。
    #[cfg_attr(test, allow(dead_code, reason = "只有桌宠真实窗口路径调用，测试窗口没有原生宿主"))]
    pub fn configure_auxiliary(&self, resizable: bool, always_on_top: bool) {
        configure_auxiliary_window(&self.0.window, resizable, always_on_top);
    }
    /// 桌宠目标屏的内容尺寸上下限在迁屏前设置，调用方必须释放全部 GPUI 借用。
    #[cfg_attr(test, allow(dead_code, reason = "只有桌宠真实窗口路径调用，测试窗口没有原生宿主"))]
    pub fn auxiliary_size_limits(&self, min: (f64, f64), max: (f64, f64)) {
        self.0.window.setContentMinSize(NSSize::new(min.0, min.1));
        self.0.window.setContentMaxSize(NSSize::new(max.0, max.1));
    }
    /// 原生显示/聚焦会同步回调 GPUI，因此限于借用之外。
    pub fn show(&self) { self.0.window.makeKeyAndOrderFront(None); }
    /// 读取实际键盘焦点，不把 AppKit 的 mainWindow 当作 keyWindow。
    pub fn is_key(&self) -> bool { self.0.window.isKeyWindow() }
    /// 原生焦点变化只作为失效信号；消费时仍重读真实key状态。
    pub fn focus_revision(&self) -> u64 { self.0.signals.focus_revision.load(Ordering::Relaxed) }
    /// 排队期间失焦则放弃；原生最小化会同步回调，调用方必须释放全部 GPUI 借用。
    pub fn minimize_if_key(&self) {
        if self.is_key() { self.0.window.miniaturize(None); }
    }
    /// 关闭到托盘也在借用之外执行。
    pub fn hide(&self) { self.0.window.orderOut(None); }
}

/// 主窗与辅助窗桥接共享原生属性映射，调用者保证当前主线程及活窗引用。
#[cfg_attr(test, allow(dead_code, reason = "只有桌宠真实窗口路径调用，测试窗口没有原生宿主"))]
pub(crate) fn configure_auxiliary_window(window: &NSWindow, resizable: bool, always_on_top: bool) {
    let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::FullSizeContentView;
    window.setStyleMask(if resizable { style | NSWindowStyleMask::Resizable } else { style });
    window.setLevel(if always_on_top { 3 } else { 0 });
    window.setHasShadow(false);
    window.setHidesOnDeactivate(false);
    window.setMovableByWindowBackground(false);
    window.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces);
}

/// 将原生全局点矩形复制为纯数值，不把它与 core 的屏内 DIP 混用。
fn native_frame(frame: NSRect) -> super::geometry_settle::NativeFrame {
    super::geometry_settle::NativeFrame { x: frame.origin.x, y: frame.origin.y,
        width: frame.size.width, height: frame.size.height }
}
