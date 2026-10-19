//! 纯坐标回归与显式真实 X11 窗口回归分开；后者只操作测试自己创建的 XID。

use super::*;
use x11rb::{properties::WmHints, protocol::xproto::{CreateWindowAux, WindowClass}, rust_connection::RustConnection};

/// 创建者连接贯穿同一测试进程，避免快速换连接复用 XID 后被上一窗口的 WM 事件误认。
static SERVER: std::sync::OnceLock<(RustConnection, u32)> = std::sync::OnceLock::new();

/// 真实服务器夹具只拥有一个独立窗口，不修改 root 属性、桌面设置或实际应用状态。
struct ServerWindow(NativeWindow);

impl ServerWindow {
    /// 隐藏创建且禁止抢焦点；可选 override_redirect 用于排除 WM 对几何的调整。
    fn new(unmanaged: bool) -> Self {
        let (connection, root) = SERVER.get_or_init(|| {
            let (connection, screen) = x11rb::connect(None).expect("native X11 test requires an actual DISPLAY");
            let root = connection.setup().roots[screen].root;
            (connection, root)
        });
        let root = *root;
        let window = connection.generate_id().unwrap();
        connection.create_window(x11rb::COPY_DEPTH_FROM_PARENT, window, root, 200, 220, 128, 160, 0,
            WindowClass::INPUT_OUTPUT, x11rb::COPY_FROM_PARENT,
            &CreateWindowAux::new().override_redirect(u32::from(unmanaged))).unwrap().check().unwrap();
        let mut hints = WmHints::new();
        hints.input = Some(false);
        hints.set(connection, window).unwrap().check().unwrap();
        let native = NativeWindow::connect(window).unwrap();
        native.connection.change_property32(PropMode::REPLACE, window, native.atom(b"_NET_WM_USER_TIME").unwrap(),
            AtomEnum::CARDINAL, &[0]).unwrap().check().unwrap();
        Self(native)
    }
    /// 管理器异步处理 EWMH 消息；只在测试中等待真实属性确认，不用模拟状态替代。
    fn wait_state(&self, atom: u32, enabled: bool) {
        for _ in 0..100 {
            let states = self.0.property(self.0.window, b"_NET_WM_STATE", AtomEnum::ATOM, 256).unwrap();
            if states.contains(&atom) == enabled { return; }
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
        panic!("window manager did not confirm requested EWMH state");
    }
    /// 仅测试线程等待真实 OS，生产代码通过 GPUI timer 执行同一个单轮读回接口。
    fn wait_native(&self) {
        for _ in 0..200 {
            if self.0.settled().unwrap() { return; }
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
        panic!("native X11 operation did not settle");
    }
}

impl Drop for ServerWindow {
    /// 即使断言失败也销毁自身窗口，连接释放不会遗留测试入口或输入区域。
    fn drop(&mut self) { let _ = self.0.connection.destroy_window(self.0.window); let _ = self.0.connection.flush(); }
}

/// 工作区保留负坐标且精确扣除面板，跨屏交集不能退成硬编码 1440×900。
#[test]
fn work_area_intersection_preserves_negative_monitor_origins() {
    let monitor = PhysicalRect { x: -1920, y: -200, width: 1920, height: 1080 };
    let desktop = PhysicalRect { x: -1920, y: -168, width: 3840, height: 1048 };
    let work = monitor.intersect(desktop).unwrap();
    assert_eq!(work, PhysicalRect { x: -1920, y: -168, width: 1920, height: 1048 });
    assert!(monitor.intersect(PhysicalRect { x: 0, y: 0, width: 100, height: 100 }).is_none());
    let logical = work.logical(1.25);
    assert_eq!(f32::from(logical.origin.x), -1536.);
    assert_eq!(f32::from(logical.size.width), 1536.);
}

/// 多屏窗口以中心所属屏幕为准，拔屏后采用实际拓扑中的最大交集或第一块屏。
#[test]
fn monitor_selection_uses_current_window_and_real_fallback() {
    let left = PhysicalRect { x: -1920, y: 0, width: 1920, height: 1080 };
    let right = PhysicalRect { x: 0, y: 0, width: 2560, height: 1400 };
    assert_eq!(select_area(&[left, right], PhysicalRect { x: -50, y: 80, width: 200, height: 160 }), right);
    assert_eq!(select_area(&[left, right], PhysicalRect { x: 4000, y: 80, width: 200, height: 160 }), left);
    assert!(!left.contains(0, 200));
    assert!(left.contains(-1920, 0));
}

/// 在自有真实 X11 窗口上验证单事务几何、显隐和全窗穿透，关闭后 resize 仍恢复完整输入区。
#[test]
#[ignore = "requires a live X11 server; run explicitly for native Linux verification"]
fn native_x11_geometry_visibility_and_input_region_are_real() {
    let fixture = ServerWindow::new(true);
    let native = &fixture.0;
    let target = PhysicalRect { x: 250, y: 260, width: 180, height: 200 };
    native.set_geometry(target).unwrap();
    fixture.wait_native();
    assert_eq!(native.bounds().unwrap(), target);
    native.set_visible(true).unwrap();
    fixture.wait_native();
    assert_eq!(native.connection.get_window_attributes(native.window).unwrap().reply().unwrap().map_state, MapState::VIEWABLE);
    let mut input = super::super::MousePassthroughState::default();
    native.set_mouse_passthrough(true, &mut input).unwrap();
    assert_eq!(input.confirmed(), Some(true));
    assert!(native.connection.shape_get_rectangles(native.window, SK::INPUT).unwrap().reply().unwrap().rectangles.is_empty());
    native.set_mouse_passthrough(false, &mut input).unwrap();
    native.set_geometry(PhysicalRect { width: 240, height: 260, ..target }).unwrap();
    fixture.wait_native();
    let rectangles = native.connection.shape_get_rectangles(native.window, SK::INPUT).unwrap().reply().unwrap().rectangles;
    assert_eq!(rectangles.len(), 1);
    assert_eq!((rectangles[0].width, rectangles[0].height), (240, 260));
    assert_eq!(input.confirmed(), Some(false));
    native.connection.configure_window(native.window, &ConfigureWindowAux::new().x(270).y(280)).unwrap().check().unwrap();
    native.set_mouse_passthrough(true, &mut input).unwrap();
    assert!(native.settled().unwrap(), "a later user drag must not retain an old geometry barrier");
    assert_eq!((native.bounds().unwrap().x, native.bounds().unwrap().y), (270, 280));
    native.set_visible(false).unwrap();
    fixture.wait_native();
    assert_eq!(native.connection.get_window_attributes(native.window).unwrap().reply().unwrap().map_state, MapState::UNMAPPED);
    assert!(!native.work_areas().unwrap().is_empty());
}

/// 真实 EWMH 管理器消费动态置顶请求，锁定限制尺寸，解锁恢复合法范围且不覆盖既有状态。
#[test]
#[ignore = "requires a live X11 server and EWMH window manager; run explicitly for native Linux verification"]
fn native_x11_topmost_lock_and_utility_states_round_trip() {
    let fixture = ServerWindow::new(false);
    let native = &fixture.0;
    let mut prefs = PetWindowPreferences { locked: true, ..Default::default() };
    native.apply(prefs, false, 1.).unwrap();
    fixture.wait_native();
    let above = native.atom(b"_NET_WM_STATE_ABOVE").unwrap();
    let skip = native.atom(b"_NET_WM_STATE_SKIP_TASKBAR").unwrap();
    let states = native.property(native.window, b"_NET_WM_STATE", AtomEnum::ATOM, 256).unwrap();
    assert!(states.contains(&above), "initial states={states:?}, above={above}, prefs={prefs:?}");
    assert!(states.contains(&skip));
    let hints = WmSizeHints::get_normal_hints(&native.connection, native.window).unwrap().reply().unwrap().unwrap();
    assert_eq!(hints.min_size, hints.max_size);
    assert_eq!(hints.min_size, Some((128, 160)));
    prefs.layout = loki_metris_core::PetLayout::Single;
    let area = select_area(&native.work_areas().unwrap(), native.bounds().unwrap()).logical(1.);
    prefs.pet_size = super::super::super::geometry::range(area, prefs.layout).constrain(80);
    let (width, height) = super::super::super::geometry::dimensions(prefs.layout, prefs.pet_size);
    let resized = (width as u32, height as u32);
    native.apply(prefs, false, 1.).unwrap();
    native.set_geometry(PhysicalRect { x: 200, y: 220, width: resized.0, height: resized.1 }).unwrap();
    fixture.wait_native();
    let hints = WmSizeHints::get_normal_hints(&native.connection, native.window).unwrap().reply().unwrap().unwrap();
    assert_eq!(hints.min_size, Some((resized.0 as i32, resized.1 as i32)), "locked size and layout changes must replace old fixed hints");
    assert_eq!(hints.min_size, hints.max_size);
    native.set_visible(true).unwrap();
    fixture.wait_native();
    fixture.wait_state(above, true);
    prefs.always_on_top = false;
    prefs.locked = false;
    native.apply(prefs, false, 1.).unwrap();
    fixture.wait_native();
    fixture.wait_state(above, false);
    fixture.wait_state(skip, true);
    let hints = WmSizeHints::get_normal_hints(&native.connection, native.window).unwrap().reply().unwrap().unwrap();
    assert_ne!(hints.min_size, hints.max_size);
    prefs.always_on_top = true;
    native.apply(prefs, false, 1.).unwrap();
    fixture.wait_native();
    fixture.wait_state(above, true);
    native.set_visible(false).unwrap();
    fixture.wait_native();
}

/// 销毁后的 XID 必须返回原生错误，不能进入 controller 的成功持久化分支。
#[test]
#[ignore = "requires a live X11 server; run explicitly for native Linux verification"]
fn native_x11_destroyed_window_never_reports_success() {
    let fixture = ServerWindow::new(true);
    fixture.0.connection.destroy_window(fixture.0.window).unwrap().check().unwrap();
    assert!(NativeWindow::connect(fixture.0.window).is_err());
    assert!(fixture.0.set_visible(true).is_err());
    assert!(fixture.0.set_geometry(PhysicalRect { x: 0, y: 0, width: 100, height: 100 }).is_err());
}
