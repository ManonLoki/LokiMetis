// 真实主线程 AppKit 菜单回归；不运行 NSApplication 事件循环或修改系统偏好。

use super::*;
use std::{
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind},
    time::{Duration, Instant},
};

const PROBE_ENV: &str = "LOKI_METRIS_MACOS_MENU_NATIVE_TEST";
const REBUILD: &str = "platform::menu_tests::menu_rebuild_reuses_values_and_preserves_old_items";
const CALLBACK: &str = "platform::menu_tests::menu_callbacks_allow_reentry_and_reject_invalid_tags";
const DISABLED: &str = "platform::menu_tests::menu_disabled_survives_native_validation";

/// 同类动作携带不同参数，不能只按 TypeId 复用槽位。
#[derive(Clone, PartialEq, gpui::Action)]
#[action(no_json, no_register)]
struct Probe(u32);

/// 子进程构造入口先于测试调度，确保真正处于 macOS 主线程。
#[used]
#[unsafe(link_section = "__DATA,__mod_init_func")]
static MAIN_THREAD_ENTRY: extern "C" fn() = native_test_entry;

/// 仅精确目标子进程初始化原生宿主；立即退出避免后续 ctor 重复注册类。
extern "C" fn native_test_entry() {
    let selected = std::env::var(PROBE_ENV).unwrap_or_default();
    let exercise: fn() = match selected.as_str() {
        REBUILD => exercise_rebuild,
        CALLBACK => exercise_callbacks,
        DISABLED => exercise_disabled,
        _ => return,
    };
    let result = catch_unwind(AssertUnwindSafe(|| {
        assert!(MainThreadMarker::new().is_some(), "必须使用真正主线程");
        objc2::rc::autoreleasepool(|_| exercise());
    }));
    std::process::exit(if result.is_ok() { 0 } else { 1 });
}

/// 超时会终止独占测试进程，把原生死锁转为失败而不阻塞用户应用。
fn run_native(name: &str) {
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture"])
        .env(PROBE_ENV, name)
        .spawn()
        .unwrap();
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "真实菜单回归失败：{name}: {status}");
            return;
        }
        if started.elapsed() > Duration::from_secs(30) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("真实菜单回归超时：{name}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 重建一千次仍只有三个参数值；旧主菜单及 Dock 项继续分派原参数。
#[test]
fn menu_rebuild_reuses_values_and_preserves_old_items() {
    run_native(REBUILD);
}

/// 原生 selector 的无效 tag 不死锁；回调重建菜单及替换自身均可继续执行。
#[test]
fn menu_callbacks_allow_reentry_and_reject_invalid_tags() {
    run_native(CALLBACK);
}

/// NSMenu.update 的真实自动校验不能把显式禁用项重新启用。
#[test]
fn menu_disabled_survives_native_validation() {
    run_native(DISABLED);
}

/// 独占测试宿主仅绑定实际生产 delegate，退出时解除全部原生反向指针。
struct Host {
    platform: Rc<MacPlatform>,
    app: id,
    delegate: id,
}
impl Host {
    /// 未运行框架 ctor 时调用同一生产注册函数，随后只创建独占 AppKit 对象。
    fn new() -> Self {
        // 安全性：此入口已验证主线程，类只注册一次；Rc 保证 ivar 指针在宿主内稳定。
        unsafe {
            if APP_CLASS.is_null() {
                build_classes();
            }
            assert!(!APP_DELEGATE_CLASS.is_null());
            let platform = Rc::new(MacPlatform::new(true));
            let app: id = msg_send![APP_CLASS, sharedApplication];
            let delegate: id = msg_send![APP_DELEGATE_CLASS, new];
            let pointer = Rc::as_ptr(&platform) as *const c_void;
            (*app).set_ivar(MAC_PLATFORM_IVAR, pointer);
            (*delegate).set_ivar(MAC_PLATFORM_IVAR, pointer);
            app.setDelegate_(delegate);
            Self {
                platform,
                app,
                delegate,
            }
        }
    }

    /// 返回生产 set_menus 安装的真实子菜单，父应用持有其生命周期。
    fn main_menu(&self) -> id {
        // 安全性：app 与实际生产主菜单在 Host 生命周期内有效。
        unsafe {
            let root: id = msg_send![self.app, mainMenu];
            let first: id = msg_send![root, itemAtIndex: 0isize];
            msg_send![first, submenu]
        }
    }

    /// 经 Objective-C 注册 selector 调用生产分派，不绕过原生回调桥。
    fn dispatch(&self, item: id) {
        // 安全性：调用方持有真实菜单项，delegate 的 platform ivar 由本宿主拥有。
        unsafe {
            let _: () = msg_send![self.delegate, handleGPUIMenuItem: item];
        }
    }

    /// 经原生 selector 验证动作，缺失或负 tag 必须返回 false。
    fn validate(&self, item: id) -> bool {
        // 安全性：与 dispatch 相同，返回类型匹配生产注册的布尔签名。
        unsafe { msg_send![self.delegate, validateMenuItem: item] }
    }
}
impl Drop for Host {
    /// 先清除 AppKit 反向指针，再释放 delegate 及最后一个 Dock 菜单。
    fn drop(&mut self) {
        // 安全性：所有对象在主线程且仍有效；Dock 的 +1 引用只取走并释放一次。
        unsafe {
            self.app.setMainMenu_(nil);
            self.app.setDelegate_(nil);
            (*self.app).set_ivar(MAC_PLATFORM_IVAR, null_mut::<c_void>());
            (*self.delegate).set_ivar(MAC_PLATFORM_IVAR, null_mut::<c_void>());
            let dock = self.platform.0.lock().dock_menu.take();
            if let Some(menu) = dock {
                CFRelease(menu as _);
            }
            let _: () = msg_send![self.delegate, release];
        }
    }
}

/// 单独 retain 旧菜单项，确保替换菜单后旧索引仍有可执行的真实对象。
struct Item(id);
impl Item {
    /// 从活菜单中取得并保留一个实际 NSMenuItem，不复制 tag 或动作算法。
    fn at(menu: id, index: isize) -> Self {
        // 安全性：调用方提供活菜单和合法索引，retain 为此包装独占一个引用。
        unsafe {
            let item: id = msg_send![menu, itemAtIndex: index];
            let _: id = msg_send![item, retain];
            Self(item)
        }
    }
    /// 直接读取生产代码赋予原生对象的 tag。
    fn tag(&self) -> NSInteger {
        // 安全性：Item 的 retain 保证对象有效。
        unsafe { msg_send![self.0, tag] }
    }
}
impl Drop for Item {
    /// 恰好释放包装持有的一个原生引用。
    fn drop(&mut self) {
        // 安全性：每个 Item 仅在 at 中 retain 一次，析构不重复调用。
        unsafe {
            let _: () = msg_send![self.0, release];
        }
    }
}

/// 读取真实动作参数，类型不匹配立即失败。
fn value(action: &dyn Action) -> u32 {
    action.as_any().downcast_ref::<Probe>().unwrap().0
}

/// 标题、排序和视觉状态变化不增加动作所有权，旧 Main 与 Dock 项互不破坏。
fn exercise_rebuild() {
    let host = Host::new();
    let keymap = Keymap::default();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let observed = calls.clone();
    host.platform.on_app_menu_action(Box::new(move |action| {
        observed.borrow_mut().push(value(action))
    }));
    let (old_main, old_dock) = objc2::rc::autoreleasepool(|_| {
        host.platform.set_menus(
            vec![Menu::new("初始").items([
                MenuItem::action("一", Probe(1)),
                MenuItem::action("二", Probe(2)),
            ])],
            &keymap,
        );
        host.platform.set_dock_menu(
            vec![
                MenuItem::action("重复一", Probe(1)),
                MenuItem::action("三", Probe(3)),
            ],
            &keymap,
        );
        let old_main = Item::at(host.main_menu(), 0);
        let dock = host.platform.0.lock().dock_menu.unwrap();
        let old_dock = Item::at(dock, 1);
        (old_main, old_dock)
    });
    let tag_main = old_main.tag();
    let tag_dock = old_dock.tag();
    for index in 0..1000 {
        objc2::rc::autoreleasepool(|_| {
            host.platform.set_menus(
                vec![
                    Menu::new(if index % 2 == 0 { "中文" } else { "English" }).items([
                        MenuItem::action("重排三", Probe(3))
                            .checked(index % 2 == 0)
                            .disabled(index % 3 == 0),
                        MenuItem::action("重排一", Probe(1)),
                        MenuItem::action("重排二", Probe(2)),
                    ]),
                ],
                &keymap,
            );
            host.platform.set_dock_menu(
                vec![
                    MenuItem::action("二", Probe(2)),
                    MenuItem::action("三", Probe(3)),
                ],
                &keymap,
            );
            assert_eq!(
                host.platform.0.lock().menu_actions.len(),
                3,
                "第{index}次重建仍只拥有三个动作值"
            );
        });
    }
    host.platform
        .set_dock_menu(vec![MenuItem::action("新参数", Probe(4))], &keymap);
    assert_eq!(host.platform.0.lock().menu_actions.len(), 4);
    let new_dock = host.platform.0.lock().dock_menu.unwrap();
    let new_item = Item::at(new_dock, 0);
    host.dispatch(new_item.0);
    host.platform.set_menus(vec![], &keymap);
    host.dispatch(old_main.0);
    host.dispatch(old_dock.0);
    assert_eq!(&*calls.borrow(), &[4, 1, 3]);
    assert_eq!(old_main.tag(), tag_main);
    assert_eq!(old_dock.tag(), tag_dock);
    host.platform.set_menus(
        vec![Menu::new("复用").items([MenuItem::action("一", Probe(1))])],
        &keymap,
    );
    assert_eq!(Item::at(host.main_menu(), 0).tag(), tag_main);
    host.platform
        .set_dock_menu(vec![MenuItem::action("三", Probe(3))], &keymap);
    assert_eq!(
        Item::at(host.platform.0.lock().dock_menu.unwrap(), 0).tag(),
        tag_dock
    );
}

/// 业务回调允许重入重建和替换 callback；无效索引不会消费既有 callback。
fn exercise_callbacks() {
    let host = Host::new();
    let keymap = Keymap::default();
    host.platform.set_menus(
        vec![Menu::new("回调").items([MenuItem::action("一", Probe(1))])],
        &keymap,
    );
    let item = Item::at(host.main_menu(), 0);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let observed = calls.clone();
    let weak = Rc::downgrade(&host.platform);
    host.platform.on_app_menu_action(Box::new(move |action| {
        observed.borrow_mut().push(value(action));
        let platform = weak.upgrade().unwrap();
        platform.set_menus(vec![], &Keymap::default());
        platform.set_dock_menu(vec![MenuItem::action("二", Probe(2))], &Keymap::default());
        let next = observed.clone();
        platform.on_app_menu_action(Box::new(move |action| {
            next.borrow_mut().push(value(action) + 10)
        }));
    }));
    host.dispatch(item.0);
    host.dispatch(item.0);
    assert_eq!(&*calls.borrow(), &[1, 11]);
    let weak = Rc::downgrade(&host.platform);
    let validations = Rc::new(Cell::new(0));
    let counted = validations.clone();
    host.platform
        .on_validate_app_menu_command(Box::new(move |_| {
            let platform = weak.upgrade().unwrap();
            platform.set_menus(vec![], &Keymap::default());
            platform.set_dock_menu(vec![], &Keymap::default());
            let next = counted.clone();
            platform.on_validate_app_menu_command(Box::new(move |action| {
                next.set(next.get() + 1);
                value(action) == 1
            }));
            false
        }));
    assert!(!host.validate(item.0));
    assert!(host.validate(item.0));
    assert_eq!(validations.get(), 1);
    let invalid = Item::at(host.main_menu_or_fixture(&keymap), 0);
    for tag in [-1, 10_000] {
        // 安全性：该独占测试项仍被 Item retain，仅改 tag 验证不可信原生输入。
        unsafe {
            let _: () = msg_send![invalid.0, setTag: tag as NSInteger];
        }
        host.dispatch(invalid.0);
        assert!(!host.validate(invalid.0));
    }
    assert_eq!(&*calls.borrow(), &[1, 11]);
    assert_eq!(validations.get(), 1);
    host.dispatch(item.0);
    assert!(host.validate(item.0));
    assert_eq!(&*calls.borrow(), &[1, 11, 11]);
    assert_eq!(validations.get(), 2);
}

impl Host {
    /// 原菜单已被重入清空时，用生产创建路径准备独立的无效输入测试项。
    fn main_menu_or_fixture(&self, keymap: &Keymap) -> id {
        self.platform.set_menus(
            vec![Menu::new("无效输入").items([MenuItem::action("测试", Probe(2))])],
            keymap,
        );
        self.main_menu()
    }
}

/// 每个真实菜单都包含可启用对照项，确认 update 确实执行过生产 validation。
fn assert_disabled(host: &Host, menu: id, validations: &Cell<usize>) {
    let disabled = Item::at(menu, 0);
    let enabled = Item::at(menu, 1);
    // 安全性：Host 与 Item 保持对象存活，update 不进入应用事件循环。
    unsafe {
        let before: BOOL = msg_send![disabled.0, isEnabled];
        assert_eq!(before, NO);
        let count = validations.get();
        let _: () = msg_send![menu, update];
        assert!(validations.get() > count, "必须实际触发 AppKit 动态校验");
        let after: BOOL = msg_send![disabled.0, isEnabled];
        assert_eq!(after, NO, "显式禁用不能被校验重新启用");
        let after: BOOL = msg_send![enabled.0, isEnabled];
        assert_eq!(after, YES);
    }
    assert!(!host.validate(disabled.0));
    assert!(host.validate(enabled.0));
}

/// 同一动作值的禁用是菜单项属性；主菜单、嵌套菜单与 Dock 都必须保持。
fn exercise_disabled() {
    let host = Host::new();
    let keymap = Keymap::default();
    let validations = Rc::new(Cell::new(0));
    let counted = validations.clone();
    let available = Rc::new(Cell::new(true));
    let validation_available = available.clone();
    host.platform
        .on_validate_app_menu_command(Box::new(move |_| {
            counted.set(counted.get() + 1);
            validation_available.get()
        }));
    let items = || {
        [
            MenuItem::action("禁用", Probe(1)).disabled(true),
            MenuItem::action("启用", Probe(1)),
        ]
    };
    host.platform
        .set_menus(vec![Menu::new("状态").items(items())], &keymap);
    assert_disabled(&host, host.main_menu(), &validations);
    host.platform.set_menus(
        vec![Menu::new("父").items([MenuItem::Submenu(Menu::new("子").items(items()))])],
        &keymap,
    );
    // 安全性：生产菜单树存在，取到的子菜单由根菜单持有。
    let nested = unsafe {
        let item: id = msg_send![host.main_menu(), itemAtIndex: 0isize];
        msg_send![item, submenu]
    };
    assert_disabled(&host, nested, &validations);
    host.platform
        .set_dock_menu(items().into_iter().collect(), &keymap);
    let dock = host.platform.0.lock().dock_menu.unwrap();
    assert_disabled(&host, dock, &validations);
    host.platform.set_menus(
        vec![Menu::new("系统动作").items([
            MenuItem::os_action("禁用剪切", Probe(2), gpui::OsAction::Cut).disabled(true),
            MenuItem::action("动态对照", Probe(1)),
        ])],
        &keymap,
    );
    assert_disabled(&host, host.main_menu(), &validations);
    let enabled = Item::at(host.main_menu(), 1);
    for expected in [false, true] {
        available.set(expected);
        // 安全性：真实主菜单仍持有 enabled 项，校验改变的是动态可用性而非显式禁用。
        unsafe {
            let _: () = msg_send![host.main_menu(), update];
            let actual: BOOL = msg_send![enabled.0, isEnabled];
            assert_eq!(actual == YES, expected, "动态校验必须允许禁用后再启用");
        }
    }
}
