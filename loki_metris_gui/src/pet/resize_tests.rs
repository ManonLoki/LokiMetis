//! 原生拖边必须收敛到core约束后的实际画布，不能只比较持久单格值。

use super::*;
use gpui_kit::TestAppContext;
use super::tests::{Fixture, fixture, settle};

/// 先通过真实store保存当前原点，避免同值尺寸回归混入首次位置写入。
fn save_current_position(f: &Fixture, cx: &mut TestAppContext) {
    let position = cx.update_window(f.overlay, |_, window, _| {
        let bounds = host::bounds(window);
        let scale = window.scale_factor();
        PetOverlayPosition {
            x: (f32::from(bounds.origin.x) * scale).round() as i32,
            y: ((f32::from(bounds.origin.y) + geometry::HEADER_HEIGHT as f32) * scale).round() as i32,
        }
    }).unwrap();
    let settings = f.service.save_pet_position(position).unwrap().blocking_recv().unwrap().unwrap();
    f.owner.update(cx, |owner, _| owner.settings = settings);
}

/// 校验最终真实窗口，并重复发bounds证明canonical尺寸不制造新保存或错误。
fn assert_geometry(f: &Fixture, before: loki_metris_core::PetWindowPreferences, cx: &mut TestAppContext) {
    let revision = f.service.pet_settings_revision();
    let path = f.root.path().join("monitor/monitor-settings.json");
    let bytes = std::fs::read(&path).unwrap();
    let (width, height) = geometry::dimensions(before.layout, before.pet_size);
    cx.update_window(f.overlay, |_, window, cx| {
        assert_eq!(window.bounds().size, size(px(width as f32), px(height as f32)),
            "{:?}的实际窗口必须与单格和布局一致", before.layout);
        for _ in 0..4 { window.bounds_changed(cx); }
    }).unwrap();
    settle(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window), before);
    assert!(f.owner.read_with(cx, |owner, _| owner.last_error.is_none()));
    assert_eq!(f.service.pet_settings_revision(), revision);
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

/// 监听公开窗口事件设定回归上限，防止连续defer反馈在单次调度内挂死测试。
fn bound_resize_feedback(f: &Fixture, cx: &mut TestAppContext) -> (Entity<usize>, gpui_kit::Subscription) {
    let count = cx.new(|_| 0_usize);
    let subscription = cx.update_window(f.overlay, |_, window, cx| {
        count.update(cx, |_, cx| cx.observe_window_bounds(window, |count, _, _| {
            *count += 1;
            assert!(*count < 32, "native resize failure repeatedly refreshed window bounds");
        }))
    }).unwrap();
    (count, subscription)
}

/// 最小单格值未变时，多格布局仍须回正被压小的窗口，单格保留合法最小值。
#[gpui_kit::test]
fn native_resize_at_minimum_restores_layout_without_changing_preferences(cx: &mut TestAppContext) {
    let f = fixture(cx);
    save_current_position(&f, cx);
    for layout in [PetLayout::Grid, PetLayout::Row, PetLayout::Column,
        PetLayout::Row3, PetLayout::Column3, PetLayout::Single] {
        f.owner.update(cx, |owner, cx| {
            owner.change(PetPreferenceChange::Layout(layout), cx);
        });
        settle(cx, &f.owner);
        let before = f.owner.read_with(cx, |owner, _| owner.settings.pet_window);
        assert_eq!(before.pet_size, 64);
        let revision = f.service.pet_settings_revision();
        let path = f.root.path().join("monitor/monitor-settings.json");
        let bytes = std::fs::read(&path).unwrap();
        // Windows固定64×96最小外框允许多格布局压小，只有单格的最小尺寸合法。
        cx.simulate_window_resize(f.overlay, size(px(64.), px(96.)));
        settle(cx, &f.owner);
        assert_geometry(&f, before, cx);
        assert_eq!(f.service.pet_settings_revision(), revision);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}

/// 已在上限的六种布局收到过大或比例偏差的原生矩形，仍回到相同配置的规范尺寸。
#[gpui_kit::test]
fn native_resize_at_maximum_restores_layout_and_aspect_ratio(cx: &mut TestAppContext) {
    let f = fixture(cx);
    save_current_position(&f, cx);
    for layout in [PetLayout::Grid, PetLayout::Row, PetLayout::Column,
        PetLayout::Row3, PetLayout::Column3, PetLayout::Single] {
        f.owner.update(cx, |owner, cx| owner.change(PetPreferenceChange::Layout(layout), cx));
        settle(cx, &f.owner);
        f.owner.update(cx, |owner, cx| {
            let max = owner.range(layout, cx).max;
            owner.change(PetPreferenceChange::Size(max), cx);
        });
        settle(cx, &f.owner);
        let before = f.owner.read_with(cx, |owner, _| owner.settings.pet_window);
        let revision = f.service.pet_settings_revision();
        cx.simulate_window_resize(f.overlay, size(px(4096.), px(4096.)));
        settle(cx, &f.owner);
        assert_geometry(&f, before, cx);
        let (width, height) = geometry::dimensions(layout, before.pet_size);
        cx.simulate_window_resize(f.overlay, size(px(width as f32 + 1.), px(height as f32)));
        settle(cx, &f.owner);
        assert_geometry(&f, before, cx);
        assert_eq!(f.service.pet_settings_revision(), revision);
    }
}

/// 正式store写入被屏障阻塞时的最后一次拖边，在成功回执后补采样并回正。
#[gpui_kit::test]
fn native_resize_during_pending_save_is_rechecked_after_receipt(cx: &mut TestAppContext) {
    let f = fixture(cx);
    save_current_position(&f, cx);
    let revision = f.service.pet_settings_revision();
    let held = crate::usage_service::block_pet_mutations(&f.service);
    f.owner.update(cx, |owner, cx| owner.change(PetPreferenceChange::AlwaysOnTop(false), cx));
    cx.run_until_parked();
    assert!(f.owner.read_with(cx, |owner, _| owner.busy));
    cx.simulate_window_resize(f.overlay, size(px(64.), px(96.)));
    // 已接受的同值尾项也必须补采样，不能在前一回执的busy期间吞掉最终拖边。
    f.owner.update(cx, |owner, cx| owner.change(PetPreferenceChange::Locked(false), cx));
    held.release();
    settle(cx, &f.owner);
    let before = f.owner.read_with(cx, |owner, _| owner.settings.pet_window);
    assert!(!before.always_on_top);
    assert_eq!(before.pet_size, 64);
    assert_geometry(&f, before, cx);
    assert_eq!(f.service.pet_settings_revision(), revision + 1);
}

/// 同值回正无需读配置；损坏文件保持原样，原生失败只报告一次且不排队重试。
#[gpui_kit::test]
fn native_resize_same_value_avoids_store_and_reports_native_failure(cx: &mut TestAppContext) {
    let f = fixture(cx);
    save_current_position(&f, cx);
    let path = f.root.path().join("monitor/monitor-settings.json");
    std::fs::write(&path, b"[").unwrap();
    let revision = f.service.pet_settings_revision();
    cx.simulate_window_resize(f.overlay, size(px(64.), px(96.)));
    settle(cx, &f.owner);
    assert_geometry(&f, f.owner.read_with(cx, |owner, _| owner.settings.pet_window), cx);
    f.owner.update(cx, |owner, _| owner.mouse_state.fail_next = true);
    cx.simulate_window_resize(f.overlay, size(px(64.), px(96.)));
    settle(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.last_error.as_ref().map(|error| error.code)),
        Some("error.monitor.petApplyFailed"));
    assert!(!f.owner.read_with(cx, |owner, _| owner.busy || !owner.pending.is_empty()));
    assert_eq!(f.service.pet_settings_revision(), revision);
    assert_eq!(std::fs::read(path).unwrap(), b"[");
}

/// 尺寸写入失败回到旧偏好的规范画布，不能恢复拖边后的矩形并无限重试。
#[gpui_kit::test]
fn native_resize_save_failure_restores_old_geometry_without_retry(cx: &mut TestAppContext) {
    let f = fixture(cx);
    save_current_position(&f, cx);
    let before = f.owner.read_with(cx, |owner, _| owner.settings.pet_window);
    let origin = cx.update_window(f.overlay, |_, window, _| window.bounds().origin).unwrap();
    let path = f.root.path().join("monitor/monitor-settings.json");
    let valid_bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, b"[").unwrap();
    let revision = f.service.pet_settings_revision();
    let _feedback_limit = bound_resize_feedback(&f, cx);
    cx.simulate_window_resize(f.overlay, size(px(200.), px(232.)));
    settle(cx, &f.owner);
    cx.update_window(f.overlay, |_, window, cx| {
        assert_eq!(window.bounds(), Bounds::new(origin, size(px(128.), px(160.))));
        for _ in 0..4 { window.bounds_changed(cx); }
    }).unwrap();
    settle(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window), before);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.last_error.as_ref().map(|error| error.code)),
        Some("error.monitor.settingsInvalid"));
    assert!(!f.owner.read_with(cx, |owner, _| owner.busy || !owner.pending.is_empty()));
    assert_eq!(f.service.pet_settings_revision(), revision);
    assert_eq!(std::fs::read(&path).unwrap(), b"[");
    // 修复配置后显式重试仍可成功，不能用永久错误门禁掩盖反馈循环。
    std::fs::write(&path, valid_bytes).unwrap();
    cx.simulate_window_resize(f.overlay, size(px(200.), px(232.)));
    settle(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.pet_size), 100);
    assert!(f.owner.read_with(cx, |owner, _| owner.last_error.is_none()));
    assert_eq!(f.service.pet_settings_revision(), revision + 1);
}

/// 当前工作区只夹紧运行尺寸；回滚不能把历史尺寸超过当前上限视为新拖边。
#[gpui_kit::test]
fn native_resize_save_failure_preserves_larger_historical_size(cx: &mut TestAppContext) {
    let f = fixture(cx);
    let range = PetSizeRange::for_work_area(2400., PetLayout::Grid);
    let settings = f.service.change_pet_preferences(PetPreferenceChange::Size(200), range)
        .unwrap().blocking_recv().unwrap().unwrap();
    f.owner.update(cx, |owner, cx| {
        owner.settings = settings;
        owner.display_geometry_unconfirmed = true;
        owner.apply_geometry(false, cx).unwrap();
    });
    cx.run_until_parked();
    f.owner.update(cx, |owner, _| owner.display_geometry_unconfirmed = false);
    save_current_position(&f, cx);
    let before = f.owner.read_with(cx, |owner, _| owner.settings.pet_window);
    let expected = f.owner.update(cx, |owner, cx| {
        geometry::dimensions(before.layout, owner.range(before.layout, cx).constrain(before.pet_size))
    });
    let path = f.root.path().join("monitor/monitor-settings.json");
    std::fs::write(&path, b"[").unwrap();
    let revision = f.service.pet_settings_revision();
    let _feedback_limit = bound_resize_feedback(&f, cx);
    cx.simulate_window_resize(f.overlay, size(px(200.), px(232.)));
    settle(cx, &f.owner);
    cx.update_window(f.overlay, |_, window, cx| {
        assert_eq!(window.bounds().size, size(px(expected.0 as f32), px(expected.1 as f32)));
        for _ in 0..4 { window.bounds_changed(cx); }
    }).unwrap();
    settle(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window), before);
    assert_eq!(before.pet_size, 200);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.last_error.as_ref().map(|error| error.code)),
        Some("error.monitor.settingsInvalid"));
    assert!(!f.owner.read_with(cx, |owner, _| owner.busy || !owner.pending.is_empty()));
    assert_eq!(f.service.pet_settings_revision(), revision);
    assert_eq!(std::fs::read(path).unwrap(), b"[");
}

/// 非整数DPI的宿主量化结果是稳定终点，相差一个真实像素才需要修正。
#[test]
fn native_resize_comparison_uses_physical_pixel_quantization() {
    for (layout, pet_size, scale) in [(PetLayout::Grid, 65, 1_f32),
        (PetLayout::Grid, 65, 1.25), (PetLayout::Grid, 65, 1.5), (PetLayout::Grid, 65, 2.),
        (PetLayout::Single, 84, 100_f32 / 96.)] {
        let (width, height) = geometry::dimensions(layout, pet_size);
        let width = (width as f32 * scale).round() / scale;
        let height = (height as f32 * scale).round() / scale;
        assert!(geometry::matches_dimensions(layout, pet_size, size(px(width), px(height)), scale));
        assert!(!geometry::matches_dimensions(layout, pet_size,
            size(px(width + 1. / scale), px(height)), scale));
    }
}
