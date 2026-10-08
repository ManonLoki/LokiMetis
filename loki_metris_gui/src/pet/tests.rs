//! 真实GPUI窗口事件覆盖桌宠owner的创建、更新、关闭与退出持久化时序。

use super::*;
use gpui_kit::{TestAppContext, test::TestWindowExt};

/// 每个测试的服务和临时文件仅由当前测试拥有。
struct Fixture {
    root: tempfile::TempDir,
    service: Arc<UsageService>,
    owner: Entity<Controller>,
    overlay: AnyWindowHandle,
}
impl Drop for Fixture {
    /// 失败断言也必须回收测试运行时。
    fn drop(&mut self) {
        self.service.finish();
    }
}
/// 只使用隔离配置与测试平台，不触碰实际用户目录或原生窗口。
fn fixture(cx: &mut TestAppContext) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let service = UsageService::in_test_directory(root.path().to_owned()).unwrap();
    cx.update(gpui_kit::init);
    let main = cx.add_window(|_, _| gpui_kit::Empty);
    let owner = cx.new(|_| Controller::new(service.clone(), main.into(), "en-US"));
    let controller = owner.clone();
    let handle = cx.update(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(24.), px(24.)),
                    size(px(128.), px(128.)),
                ))),
                titlebar: None,
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| overlay::Overlay::new(controller, window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .unwrap()
    });
    owner.update(cx, |this, _| {
        this.overlay = Some(handle.into());
        this.geometry_restored = true;
    });
    Fixture {
        root,
        service,
        owner,
        overlay: handle.into(),
    }
}
/// 后台真实文件操作与GPUI假时钟之间有界等待，超时不能静默通过。
fn settle(cx: &mut TestAppContext, owner: &Entity<Controller>) {
    for _ in 0..500 {
        cx.run_until_parked();
        if owner.read_with(cx, |owner, _| !owner.busy && owner.pending.is_empty()) {
            return;
        }
        cx.executor().advance_clock(Duration::from_millis(30));
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("pet preference receipt did not settle");
}
/// 设置首绘不重借用owner；控件事件结束后才修改另一个窗口，关闭后能重建。
#[gpui_kit::test]
fn independent_settings_create_resize_close_and_reopen(cx: &mut TestAppContext) {
    let f = fixture(cx);
    f.owner.update(cx, |owner, cx| owner.open_settings(cx));
    cx.run_until_parked();
    let settings = f
        .owner
        .read_with(cx, |owner, _| owner.settings_window.unwrap());
    cx.update_window(settings, |_, window, cx| {
        window.render_frame(cx);
        window.click("pet-layout-Row3", cx);
    })
    .unwrap();
    settle(cx, &f.owner);
    assert_eq!(
        f.owner
            .read_with(cx, |owner, _| owner.settings.pet_window.layout),
        PetLayout::Row3
    );
    cx.update_window(f.overlay, |_, window, _| {
        assert_eq!(window.bounds().size, size(px(192.), px(64.)))
    })
    .unwrap();
    let saved =
        crate::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
    assert_eq!(saved.pet_window.layout, PetLayout::Row3);
    cx.update_window(settings, |_, window, cx| {
        window.render_frame(cx);
        window.click("pet-settings-close", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(
        f.owner
            .read_with(cx, |owner, _| owner.settings_window.is_none())
    );
    assert!(cx.update_window(settings, |_, _, _| ()).is_err());
    f.owner.update(cx, |owner, cx| owner.open_settings(cx));
    cx.run_until_parked();
    let reopened = f
        .owner
        .read_with(cx, |owner, _| owner.settings_window.unwrap());
    assert_ne!(settings, reopened);
}
/// 浮窗当前Window作用域内发出的意图不会丢失实际resize，也不会覆盖其它字段。
#[gpui_kit::test]
fn overlay_event_defers_resize_and_lock_blocks_wheel_size(cx: &mut TestAppContext) {
    let f = fixture(cx);
    cx.update_window(f.overlay, |_, _, cx| {
        f.owner.update(cx, |owner, cx| {
            owner.change(PetPreferenceChange::ResizeStep(true), cx)
        })
    })
    .unwrap();
    settle(cx, &f.owner);
    cx.update_window(f.overlay, |_, window, _| {
        assert_eq!(window.bounds().size, size(px(176.), px(176.)))
    })
    .unwrap();
    f.owner.update(cx, |owner, cx| {
        owner.change(PetPreferenceChange::Locked(true), cx);
        owner.change(PetPreferenceChange::ResizeStep(true), cx);
    });
    settle(cx, &f.owner);
    assert_eq!(
        f.owner.read_with(cx, |owner, _| (
            owner.settings.pet_window.pet_size,
            owner.settings.pet_window.locked
        )),
        (88, true)
    );
}
/// 唯一退出路径捕获尚未defer派发的意图、后续队列和未满200ms的位置。
#[gpui_kit::test]
fn shutdown_flushes_staged_pending_and_debounced_position_after_active_write(
    cx: &mut TestAppContext,
) {
    let f = fixture(cx);
    cx.update(|cx| cx.set_global(Manager(f.owner.clone())));
    let position = PetOverlayPosition { x: -123, y: 456 };
    let plan = cx.update(|cx| {
        f.owner.update(cx, |owner, cx| {
            owner.change(PetPreferenceChange::TurnPage(1), cx);
            owner.change(PetPreferenceChange::Locked(true), cx);
            owner.moved(
                Bounds::new(point(px(-61.5), px(228.)), size(px(128.), px(128.))),
                2.,
                cx,
            );
        });
        take_shutdown_plan(cx)
    });
    assert_eq!(plan.changes.len(), 2);
    assert_eq!(plan.position, Some(position));
    f.service.finish_with_pet(plan);
    let saved =
        crate::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
    assert_eq!(saved.pet_window.focused_slot, 4);
    assert!(saved.pet_window.locked);
    assert_eq!(saved.pet_overlay_position, Some(position));
    cx.run_until_parked();
    assert!(
        f.service
            .change_pet_preferences(
                PetPreferenceChange::TurnPage(1),
                PetSizeRange { min: 64, max: 135 }
            )
            .is_err()
    );
}
/// 原生应用失败不提交偏好，保留可见错误；后续意图不会因busy卡死。
#[gpui_kit::test]
fn missing_overlay_rejects_native_change_without_saving(cx: &mut TestAppContext) {
    let f = fixture(cx);
    f.owner.update(cx, |owner, cx| {
        owner.overlay = None;
        owner.change(PetPreferenceChange::Locked(true), cx);
    });
    settle(cx, &f.owner);
    assert!(f.owner.read_with(cx, |owner, _| owner.last_error.is_some()
        && !owner.settings.pet_window.locked));
    assert_eq!(f.service.pet_settings_revision(), 0);
}

/// 无图时四个位置各占64逻辑像素，不因透明Root或百分比网格塌陷。
#[gpui_kit::test]
fn empty_overlay_has_four_visible_position_bounds(cx: &mut TestAppContext) {
    let f = fixture(cx);
    cx.update_window(f.overlay, |_, window, cx| {
        window.render_frame(cx);
        for id in 0_usize..4 {
            let bounds = window.find(("pet-position", id)).bounds();
            assert_eq!(bounds.size, size(px(64.), px(64.)));
        }
    })
    .unwrap();
}

/// 设置滑块键盘步长为1；观察值更新会保存辅助增减而程序同步相同值不重复提交。
#[gpui_kit::test]
fn settings_size_keyboard_step_persists(cx: &mut TestAppContext) {
    let f = fixture(cx);
    f.owner.update(cx, |owner, cx| owner.open_settings(cx));
    cx.run_until_parked();
    let settings = f
        .owner
        .read_with(cx, |owner, _| owner.settings_window.unwrap());
    cx.update_window(settings, |_, window, cx| {
        window.render_frame(cx);
        window.click("pet-size-control", cx);
    })
    .unwrap();
    settle(cx, &f.owner);
    let before = f
        .owner
        .read_with(cx, |owner, _| owner.settings.pet_window.pet_size);
    cx.simulate_keystrokes(settings, "right");
    settle(cx, &f.owner);
    assert_eq!(
        f.owner
            .read_with(cx, |owner, _| owner.settings.pet_window.pet_size),
        before + 1
    );
}
