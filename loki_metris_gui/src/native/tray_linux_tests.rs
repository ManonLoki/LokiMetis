//! 私有 D-Bus 验证真实 Watcher、进程身份及注册生命周期，不接触用户托盘与会话环境。

use super::*;
use ashpd::zbus::{self, interface};
use std::{io::{BufRead as _, BufReader}, process::{Child, Command, Stdio}, sync::Mutex};

/// 总线地址只交给本测试连接，退出和断言失败均回收 daemon。
struct PrivateBus {
    /// 私有 dbus-daemon 子进程。
    child: Child,
    /// 私有总线地址。
    address: String,
}

impl PrivateBus {
    /// 创建独立会话总线，不修改 DBUS_SESSION_BUS_ADDRESS 或用户配置。
    fn new() -> Self {
        let mut child = Command::new("dbus-daemon").args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped()).stderr(Stdio::null()).spawn().expect("private dbus-daemon");
        let mut address = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut address).unwrap();
        assert!(!address.trim().is_empty());
        Self { child, address: address.trim().into() }
    }

    /// 每次连接有有限方法期限，直接使用私有地址而非真实用户总线。
    async fn connection(&self) -> Connection {
        Builder::address(self.address.as_str()).unwrap().method_timeout(Duration::from_millis(500))
            .build().await.unwrap()
    }

    /// 服务实现真实两个 Watcher 属性，条目列表可在同一客户端连接存续期间改变。
    async fn watcher(&self, state: Arc<WatcherState>) -> Connection {
        Builder::address(self.address.as_str()).unwrap().name("org.kde.StatusNotifierWatcher").unwrap()
            .serve_at("/StatusNotifierWatcher", Watcher(state)).unwrap().build().await.unwrap()
    }
}

impl Drop for PrivateBus {
    /// 独立 daemon 无论测试是否通过都不成为遗留进程。
    fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); }
}

/// 属性只携带外部协议事实，query 的身份和可恢复判断始终执行正式代码。
#[derive(Default)]
struct WatcherState {
    /// 模拟 Watcher 是否声明宿主已注册。
    host: AtomicBool,
    /// 模拟的已注册条目。
    items: Mutex<Vec<String>>,
}

/// 本地注册项由独立对象导出，服务的真实 PID 由 D-Bus daemon 返回。
struct Watcher(Arc<WatcherState>);

#[interface(name = "org.kde.StatusNotifierWatcher", crate = "ashpd::zbus")]
impl Watcher {
    /// 声明是否有实际托盘宿主，单独创建 Watcher 不等于宿主存在。
    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool { self.0.host.load(Ordering::Acquire) }

    /// 返回当前注册对象，注销与重注册通过真实 Properties.Get/GetAll 重新读取。
    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> { self.0.items.lock().unwrap().clone() }
}

/// 测试条目只提供协议 Id；不以测试 helper 预先计算 query 结果。
struct Item(Arc<Mutex<String>>);

#[interface(name = "org.kde.StatusNotifierItem", crate = "ashpd::zbus")]
impl Item {
    /// 公开 Id 可随场景改变，只有正式产品的精确 main_tray 才可作为恢复入口。
    #[zbus(property)]
    fn id(&self) -> String { self.0.lock().unwrap().clone() }
}

/// 真实桌面两种注册形式都解析，畸形路径不意外指向另一个对象。
#[test]
fn watcher_addresses_support_kde_and_gnome_without_accepting_malformed_objects() {
    assert_eq!(item_address(":1.104@/StatusNotifierItem"), Some((":1.104", "/StatusNotifierItem")));
    assert_eq!(item_address(":1.104/StatusNotifierItem"), Some((":1.104", "/StatusNotifierItem")));
    assert_eq!(item_address("org.example.Tray"), Some(("org.example.Tray", "/StatusNotifierItem")));
    assert!(item_address("@/StatusNotifierItem").is_none());
    assert!(item_address(":1.1@relative").is_none());
}

/// 活宿主、精确产品 Id 和本进程三项缺一不可；daemon 的另一 PID 不能冒充本应用。
#[test]
fn private_bus_requires_host_exact_item_id_and_own_process() {
    futures::executor::block_on(async {
        let bus = PrivateBus::new();
        let state = Arc::new(WatcherState::default());
        let _watcher = bus.watcher(state.clone()).await;
        let id = Arc::new(Mutex::new("main_tray".into()));
        let item = Builder::address(bus.address.as_str()).unwrap().serve_at("/StatusNotifierItem", Item(id.clone()))
            .unwrap().build().await.unwrap();
        let client = bus.connection().await;
        let entry = format!("{}/StatusNotifierItem", item.unique_name().unwrap());
        *state.items.lock().unwrap() = vec![entry.clone()];
        assert!(!query(&client).await.unwrap(), "registered object without host must not allow hiding");
        state.host.store(true, Ordering::Release);
        for wrong in ["other", "Main_Tray", "main_tray_extra", " main_tray"] {
            *id.lock().unwrap() = wrong.into();
            assert!(!query(&client).await.unwrap(), "wrong Id was accepted: {wrong}");
        }
        *id.lock().unwrap() = "main_tray".into();
        assert!(query(&client).await.unwrap());
        let dbus = zbus::fdo::DBusProxy::new(&client).await.unwrap();
        let daemon_pid = dbus.get_connection_unix_process_id("org.freedesktop.DBus".try_into().unwrap()).await.unwrap();
        assert_eq!(daemon_pid, bus.child.id());
        assert_ne!(daemon_pid, std::process::id());
        *state.items.lock().unwrap() = vec!["org.freedesktop.DBus/StatusNotifierItem".into()];
        assert!(!query(&client).await.unwrap(), "another PID must be rejected before querying its item");
        *state.items.lock().unwrap() = vec!["org.freedesktop.DBus/StatusNotifierItem".into(), entry];
        assert!(query(&client).await.unwrap(), "foreign entries must not hide our registered item");
    });
}

/// 同一活连接反复读到条目注销及重注册，KDE 与 GNOME 的对象地址均能实际查询。
#[test]
fn private_bus_tracks_unregister_and_reregister_with_both_item_addresses() {
    futures::executor::block_on(async {
        let bus = PrivateBus::new();
        let state = Arc::new(WatcherState::default());
        state.host.store(true, Ordering::Release);
        let _watcher = bus.watcher(state.clone()).await;
        let item = Builder::address(bus.address.as_str()).unwrap().serve_at("/StatusNotifierItem", Item(Arc::new(Mutex::new("main_tray".into()))))
            .unwrap().build().await.unwrap();
        let client = bus.connection().await;
        for separator in ["", "@"] {
            *state.items.lock().unwrap() = vec![format!("{}{separator}/StatusNotifierItem", item.unique_name().unwrap())];
            assert!(query(&client).await.unwrap());
            state.items.lock().unwrap().clear();
            assert!(!query(&client).await.unwrap(), "unregistered item must immediately stop allowing hiding");
        }
        *state.items.lock().unwrap() = vec![format!("{}/StatusNotifierItem", item.unique_name().unwrap())];
        assert!(query(&client).await.unwrap());
        state.host.store(false, Ordering::Release);
        assert!(!query(&client).await.unwrap(), "host loss must revoke a still-registered item");
        state.host.store(true, Ordering::Release);
        assert!(query(&client).await.unwrap());
    });
}

/// Watcher 失效时返回错误供正式 worker 发布 unavailable，重建后同一客户端可重新确认。
#[test]
fn private_bus_recovers_after_watcher_owner_disappears_and_restarts() {
    futures::executor::block_on(async {
        let bus = PrivateBus::new();
        let state = Arc::new(WatcherState::default());
        state.host.store(true, Ordering::Release);
        let first = bus.watcher(state.clone()).await;
        let item = Builder::address(bus.address.as_str()).unwrap().serve_at("/StatusNotifierItem", Item(Arc::new(Mutex::new("main_tray".into()))))
            .unwrap().build().await.unwrap();
        *state.items.lock().unwrap() = vec![format!("{}/StatusNotifierItem", item.unique_name().unwrap())];
        let client = bus.connection().await;
        assert!(query(&client).await.unwrap());
        first.release_name("org.kde.StatusNotifierWatcher").await.unwrap();
        assert!(query(&client).await.is_err(), "absent Watcher must not reuse a previous successful result");
        let second = bus.watcher(state.clone()).await;
        assert_ne!(first.unique_name(), second.unique_name());
        assert!(query(&client).await.unwrap(), "replacement Watcher must be probed without restarting the app");
        state.items.lock().unwrap().clear();
        assert!(!query(&client).await.unwrap());
    });
}
