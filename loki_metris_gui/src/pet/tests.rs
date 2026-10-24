//! 真实GPUI窗口事件覆盖桌宠owner的创建、更新、关闭与退出持久化时序。

use super::*;
use gpui_kit::{TestAppContext, test::TestWindowExt};

/// 每个测试的服务和临时文件仅由当前测试拥有。
pub(super) struct Fixture {
    /// 测试临时目录。
    pub(super) root: tempfile::TempDir,
    /// 测试用量服务。
    pub(super) service: Arc<UsageService>,
    /// 桌宠控制器。
    pub(super) owner: Entity<Controller>,
    /// 桌宠浮窗句柄。
    pub(super) overlay: AnyWindowHandle,
}
impl Drop for Fixture {
    /// 失败断言也必须回收测试运行时。
    fn drop(&mut self) {
        self.service.finish();
    }
}
/// 只使用隔离配置与测试平台，不触碰实际用户目录或原生窗口。
pub(super) fn fixture(cx: &mut TestAppContext) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let service = UsageService::in_test_directory(root.path().to_owned()).unwrap();
    cx.update(gpui_kit::init);
    let main = cx.add_window(|_, _| gpui_kit::Empty);
    let owner = cx.new(|_| Controller::new(service.clone(), main.into(), "en-US"));
    let controller = owner.clone();
    let mut overlay_view = None;
    let handle = cx.update(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(24.), px(24.)),
                    size(px(128.), px(160.)),
                ))),
                titlebar: None,
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| overlay::Overlay::new(controller, window, cx));
                overlay_view = Some(view.clone());
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .unwrap()
    });
    owner.update(cx, |this, _| {
        this.overlay = Some(handle.into());
        this.overlay_view = overlay_view;
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
pub(super) fn settle(cx: &mut TestAppContext, owner: &Entity<Controller>) {
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
/// 使用公开窗口观察机制记录真实 bounds 通知，不替换宿主或读取生产交互标志。
fn bounds_refreshes(handle: AnyWindowHandle, cx: &mut TestAppContext) -> (Entity<usize>, gpui_kit::Subscription) {
    let count = cx.new(|_| 0_usize);
    let subscription = cx.update_window(handle, |_, window, cx| {
        count.update(cx, |_, cx| cx.observe_window_bounds(window, |count, _, _| *count += 1))
    }).unwrap();
    (count, subscription)
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
        assert_eq!(window.bounds().size, size(px(192.), px(96.)))
    })
    .unwrap();
    let saved =
        loki_metris_service::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
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
        assert_eq!(window.bounds().size, size(px(176.), px(208.)))
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
                Bounds::new(point(px(-61.5), px(196.)), size(px(128.), px(128.))),
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
        loki_metris_service::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
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

/// 分页栏的左键仅属于分页控件，右键与桌宠主体一样打开设置，不被栏位吞掉。
#[gpui_kit::test]
fn pager_right_click_opens_settings_and_left_click_keeps_overlay(cx: &mut TestAppContext) {
    let f = fixture(cx);
    cx.update_window(f.overlay, |_, window, cx| {
        window.render_frame(cx);
        window.hover("pet-overlay", cx);
        window.render_frame(cx);
        window.click("pet-pager", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(
        f.owner
            .read_with(cx, |owner, _| owner.settings_window.is_none())
    );
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.page().page_index), 0);

    cx.update_window(f.overlay, |_, window, cx| {
        window.render_frame(cx);
        window.right_click("pet-pager", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let settings = f
        .owner
        .read_with(cx, |owner, _| owner.settings_window.unwrap());
    assert!(cx.update_window(settings, |_, _, _| ()).is_ok());
    assert!(cx.update_window(f.overlay, |_, _, _| ()).is_ok());
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.page().page_index), 0);
}

/// 真分页只更新状态与落盘；指针无需恢复，离开窗口和失焦才隐藏控件。
#[gpui_kit::test]
fn page_only_mutation_keeps_pager_and_hides_on_exit_or_blur(cx: &mut TestAppContext) {
    use gpui_kit::{InputEvent as _, MouseExitEvent, MouseMoveEvent, VisualTestContext};

    let f = fixture(cx);
    f.owner.update(cx, |owner, cx| {
        owner.change(PetPreferenceChange::Layout(PetLayout::Row3), cx);
        owner.change(PetPreferenceChange::FocusedSlot(9), cx);
    });
    settle(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.page().page_index), 3);
    let (refreshes, _subscription) = bounds_refreshes(f.overlay, cx);
    let pointer = cx
        .update_window(f.overlay, |_, window, cx| {
            window.render_frame(cx);
            window.dispatch_event(
                MouseMoveEvent {
                    position: point(px(-1.), px(-1.)),
                    ..Default::default()
                }
                .to_platform_input(),
                cx,
            );
            window.render_frame(cx);
            window.hover("pet-overlay", cx);
            window.click("pet-previous", cx);
            window.mouse_position()
        })
        .unwrap();
    settle(cx, &f.owner);
    assert_eq!(refreshes.read_with(cx, |count, _| *count), 0, "page-only mutation must not refresh native bounds");
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.page().page_index), 2);
    let saved =
        loki_metris_service::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
    assert_eq!(saved.pet_window.focused_slot, 6);
    f.owner.update(cx, |owner, cx| owner.change(PetPreferenceChange::FocusedSlot(0), cx));
    settle(cx, &f.owner);
    assert_eq!(refreshes.read_with(cx, |count, _| *count), 0, "focused-slot mutation must not refresh native bounds");
    let saved = loki_metris_service::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
    assert_eq!(saved.pet_window.focused_slot, 0);
    cx.update_window(f.overlay, |_, window, cx| {
        assert_eq!(window.mouse_position(), pointer, "page-only changes must preserve the pointer");
        window.render_frame(cx);
        assert!(window.find("pet-pager").bounds().contains(&pointer));
        assert!(window.try_find("pet-previous").is_some());
        assert!(window.try_find("pet-next").is_some());
        window.dispatch_event(
            MouseMoveEvent {
                position: point(px(-1.), px(-1.)),
                ..Default::default()
            }
            .to_platform_input(),
            cx,
        );
        window.dispatch_event(
            MouseExitEvent {
                position: point(px(-1.), px(-1.)),
                ..Default::default()
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
        assert!(window.try_find("pet-pager").is_none());
        window.hover("pet-overlay", cx);
        window.render_frame(cx);
        assert!(window.try_find("pet-pager").is_some());
        window.activate_window();
    })
    .unwrap();
    cx.run_until_parked();
    let mut visual = VisualTestContext::from_window(f.overlay, cx);
    visual.deactivate_window();
    cx.update_window(f.overlay, |_, window, cx| {
        window.render_frame(cx);
        assert!(!window.is_window_active());
        assert!(window.try_find("pet-pager").is_none());
    })
    .unwrap();
}

/// 真实异步持久化遇到损坏配置只回滚页码，不重写文件或重新应用原生窗口。
#[gpui_kit::test]
fn page_only_save_failure_rolls_back_without_bounds_refresh(cx: &mut TestAppContext) {
    let f = fixture(cx);
    cx.run_until_parked();
    let (refreshes, _subscription) = bounds_refreshes(f.overlay, cx);
    let (pointer, bounds) = cx.update_window(f.overlay, |_, window, cx| {
        window.render_frame(cx);
        window.hover("pet-overlay", cx);
        (window.mouse_position(), window.bounds())
    }).unwrap();
    let config = f.root.path().join("monitor");
    std::fs::create_dir_all(&config).unwrap();
    let path = loki_metris_service::monitor::settings::settings_path(&config);
    std::fs::write(&path, b"{broken").unwrap();
    f.owner.update(cx, |owner, cx| owner.change(PetPreferenceChange::TurnPage(1), cx));
    settle(cx, &f.owner);
    assert_eq!(refreshes.read_with(cx, |count, _| *count), 0, "failed page save must not apply or roll back native bounds");
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.focused_slot), 0);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.last_error.as_ref().map(|error| error.code)), Some("error.monitor.settingsInvalid"));
    assert_eq!(f.service.pet_settings_revision(), 0);
    assert_eq!(std::fs::read(path).unwrap(), b"{broken");
    cx.update_window(f.overlay, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.mouse_position(), pointer);
        assert_eq!(window.bounds(), bounds);
        assert!(window.try_find("pet-pager").is_some());
    }).unwrap();
}

/// 四个实际宿主字段仍须刷新原生窗口，分页限制不能跳过属性更新。
#[gpui_kit::test]
fn host_field_mutations_still_refresh_bounds(cx: &mut TestAppContext) {
    let f = fixture(cx);
    cx.run_until_parked();
    let (refreshes, _subscription) = bounds_refreshes(f.overlay, cx);
    for change in [
        PetPreferenceChange::Layout(PetLayout::Row3),
        PetPreferenceChange::Size(80),
        PetPreferenceChange::AlwaysOnTop(false),
        PetPreferenceChange::Locked(true),
    ] {
        let before = refreshes.read_with(cx, |count, _| *count);
        f.owner.update(cx, |owner, cx| owner.change(change, cx));
        settle(cx, &f.owner);
        assert!(refreshes.read_with(cx, |count, _| *count) > before, "host field change {change:?} must still refresh bounds");
    }
}

/// 已处理方向键必须通知原生宿主消费，并且单事件只翻一页、显示键盘分页栏。
#[gpui_kit::test]
fn overlay_arrow_keys_consume_once_and_leave_unrelated_keys(cx: &mut TestAppContext) {
    use gpui_kit::{InputEvent as _, KeyDownEvent, Keystroke, MouseExitEvent, MouseMoveEvent};

    let f = fixture(cx);
    f.owner.update(cx, |owner, cx| {
        owner.change(PetPreferenceChange::Layout(PetLayout::Row3), cx);
        owner.change(PetPreferenceChange::FocusedSlot(9), cx);
    });
    settle(cx, &f.owner);
    cx.update_window(f.overlay, |_, window, cx| {
        window.activate_window();
        window.render_frame(cx);
        window.click("pet-overlay", cx);
        let outside = point(px(-1.), px(-1.));
        window.dispatch_event(MouseMoveEvent { position: outside, ..Default::default() }.to_platform_input(), cx);
        window.dispatch_event(MouseExitEvent { position: outside, ..Default::default() }.to_platform_input(), cx);
    }).unwrap();
    cx.run_until_parked();
    for (key, expected) in [("left", 2), ("right", 3), ("right", 0), ("left", 3)] {
        cx.update_window(f.overlay, |_, window, cx| {
            window.render_frame(cx);
            let result = window.dispatch_event(KeyDownEvent {
                keystroke: Keystroke::parse(key).unwrap(),
                is_held: false,
                prefer_character_input: false,
            }.to_platform_input(), cx);
            assert!(!result.propagate, "handled {key} must not return to native fallback");
            window.render_frame(cx);
            assert!(window.is_window_active());
            assert!(window.focused(cx).is_some());
            assert!(window.try_find("pet-pager").is_some());
        }).unwrap();
        settle(cx, &f.owner);
        assert_eq!(f.owner.read_with(cx, |owner, _| owner.page().page_index), expected);
        let saved = loki_metris_service::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
        assert_eq!(usize::from(saved.pet_window.focused_slot), expected * 3);
    }
    cx.update_window(f.overlay, |_, window, cx| {
        let result = window.dispatch_event(KeyDownEvent {
            keystroke: Keystroke::parse("z").unwrap(),
            is_held: false,
            prefer_character_input: false,
        }.to_platform_input(), cx);
        assert!(result.propagate, "unrelated keys must remain available to the host");
    }).unwrap();
    settle(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.page().page_index), 3);
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - tests_keyboard.rs：指针在桌宠内时切换键盘输入模式不能伪装为离开并清空分页栏。；设置滑块键盘步长为1；观察值更新会保存辅助增减而程序同步相同值不重复提交。
include!("tests_keyboard.rs");
