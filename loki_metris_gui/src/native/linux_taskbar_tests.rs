//! 主窗任务栏意图的缓存、失败重试和真实 X11 协议回归；不把协议回执等同 Dock 实机验收。

use super::*;
use x11rb::{connection::Connection as _, protocol::{Event, xproto::{ChangeWindowAttributesAux, CreateWindowAux, WindowClass}}};

/// 托盘从可用到失效均提交对应意图；重复事实不访问原生宿主，重新显示会强制重发。
#[test]
fn dock_requests_follow_restore_availability_and_remap() {
    let mut request = TaskbarRequest::default();
    let mut sent = Vec::new();
    for available in [false, false, true, true, false, false, true] {
        request.sync(available, |skip| { sent.push(skip); Ok(()) }).unwrap();
    }
    assert_eq!(sent, [false, true, false, true]);
    request.invalidate();
    request.sync(true, |skip| { sent.push(skip); Ok(()) }).unwrap();
    assert_eq!(sent, [false, true, false, true, true]);
}

/// 原生失败不得缓存为已提交，下一轮继续同一意图；成功后才抑制重复请求。
#[test]
fn failed_dock_request_retries_before_caching() {
    let mut request = TaskbarRequest::default();
    assert!(request.sync(true, |_| anyhow::bail!("X11 request failed")).is_err());
    let mut calls = 0;
    request.sync(true, |skip| { assert!(skip); calls += 1; Ok(()) }).unwrap();
    request.sync(true, |_| panic!("已发送的同一意图不应重复请求")).unwrap();
    assert_eq!(calls, 1);
    assert!(request.sync(false, |_| anyhow::bail!("X11 request failed")).is_err());
    request.sync(true, |skip| { assert!(skip); calls += 1; Ok(()) }).unwrap();
    assert_eq!(calls, 2, "相反请求部分失败后，旧意图也必须重新提交");
    assert!(request.sync(false, |_| anyhow::bail!("X11 request failed")).is_err());
    request.sync(false, |skip| { assert!(!skip); calls += 1; Ok(()) }).unwrap();
    assert_eq!(calls, 3);
}

/// 每个协议测试独占连接和未映射窗口；销毁只影响自身 XID，不改变用户窗口及 root 属性。
struct ServerWindow {
    connection: x11rb::rust_connection::RustConnection,
    root: u32,
    window: u32,
    state: u32,
    skip: u32,
    above: u32,
}

impl ServerWindow {
    /// override-redirect 窗口避免测试管理状态被实际 WM 接管，所有请求仍由真实服务器处理。
    fn new() -> Self {
        let (connection, screen) = x11rb::connect(None).expect("native X11 test requires DISPLAY");
        let root = connection.setup().roots[screen].root;
        let window = connection.generate_id().unwrap();
        connection.create_window(x11rb::COPY_DEPTH_FROM_PARENT, window, root, 0, 0, 1, 1, 0,
            WindowClass::INPUT_OUTPUT, x11rb::COPY_FROM_PARENT, &CreateWindowAux::new().override_redirect(1))
            .unwrap().check().unwrap();
        let atom = |name: &[u8]| connection.intern_atom(false, name).unwrap().reply().unwrap().atom;
        let state = atom(b"_NET_WM_STATE");
        let skip = atom(b"_NET_WM_STATE_SKIP_TASKBAR");
        let above = atom(b"_NET_WM_STATE_ABOVE");
        Self { connection, root, window, state, skip, above }
    }

    /// 读取真实 ATOM 列表，缺失或错误类型均不能作为隐藏状态成功。
    fn states(&self) -> Vec<u32> {
        self.connection.get_property(false, self.window, self.state, AtomEnum::ATOM, 0, 256)
            .unwrap().reply().unwrap().value32().unwrap().collect()
    }
}

impl Drop for ServerWindow {
    /// 断言失败也释放本测试窗口，不留下桌面入口或长驻客户端。
    fn drop(&mut self) { let _ = self.connection.destroy_window(self.window); let _ = self.connection.flush(); }
}

/// 初次显示和撤回窗口只改变 skip atom，保留最大化、置顶及未知状态，重复开启没有重复 atom。
#[test]
#[ignore = "requires a live X11 server; run explicitly for native Linux verification"]
fn native_x11_taskbar_initial_state_preserves_other_atoms() {
    let f = ServerWindow::new();
    let maximum = f.connection.intern_atom(false, b"_NET_WM_STATE_MAXIMIZED_VERT").unwrap().reply().unwrap().atom;
    f.connection.change_property32(PropMode::REPLACE, f.window, f.state, AtomEnum::ATOM, &[f.above, maximum])
        .unwrap().check().unwrap();
    for skip in [true, true, false, false, true] {
        request_skip_taskbar(&f.connection, f.window, skip, true).unwrap();
        assert_eq!(f.states(), if skip { vec![f.above, maximum, f.skip] } else { vec![f.above, maximum] });
    }
    request_skip_taskbar(&f.connection, f.window, false, false).unwrap();
    assert_eq!(f.states(), [f.above, maximum, f.skip], "map后或轮询只发消息，即使WM尚未映射也不能覆写属性");
    f.connection.destroy_window(f.window).unwrap().check().unwrap();
    assert!(request_skip_taskbar(&f.connection, f.window, true, false).is_err(), "失效窗口不能报告请求成功");
}

/// 已映射或 Iconic 主窗始终交由 WM 解释 root 消息，客户端不得覆写已有状态列表。
#[test]
#[ignore = "requires a live X11 server; run explicitly for native Linux verification"]
fn native_x11_taskbar_managed_and_iconic_windows_send_state_messages() {
    let f = ServerWindow::new();
    f.connection.change_window_attributes(f.root, &ChangeWindowAttributesAux::new().event_mask(EventMask::SUBSTRUCTURE_NOTIFY))
        .unwrap().check().unwrap();
    let wm_state = f.connection.intern_atom(false, b"WM_STATE").unwrap().reply().unwrap().atom;
    for mapped in [false, true] {
        if mapped { f.connection.map_window(f.window).unwrap().check().unwrap(); }
        f.connection.change_property32(PropMode::REPLACE, f.window, wm_state, wm_state, &[3, 0]).unwrap().check().unwrap();
        for skip in [true, true, false] {
            f.connection.change_property32(PropMode::REPLACE, f.window, f.state, AtomEnum::ATOM, &[f.above])
                .unwrap().check().unwrap();
            request_skip_taskbar(&f.connection, f.window, skip, !mapped).unwrap();
            assert_eq!(f.states(), [f.above], "动态请求不能冒充 WM 修改属性");
            let event = std::iter::from_fn(|| f.connection.poll_for_event().unwrap()).find_map(|event| match event {
                Event::ClientMessage(event) if event.window == f.window && event.type_ == f.state => Some(event),
                _ => None,
            }).expect("必须向 root 提交任务栏状态消息");
            assert_eq!(event.format, 32);
            assert_eq!(event.data.as_data32(), [u32::from(skip), f.skip, 0, 1, 0]);
        }
    }
}
