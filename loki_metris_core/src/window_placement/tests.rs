//! 参数化验证鼠标屏优先、局部坐标、混合 DPI 与损坏旧状态兼容。

use super::*;

/// 测试只构造屏内 DIP；全局负原点由原生转换层单独验证。
fn display(key: &str, width: f64, height: f64, scale: f64) -> WindowDisplayArea {
    WindowDisplayArea {
        key: key.into(), primary: key == "primary", scale,
        work_area: WindowDipRect {
            origin: WindowDipPoint { x: 0., y: 24. },
            size: WindowDipSize { width, height },
        },
    }
}

/// 主窗固定已批准尺寸，不借用测试宿主状态。
fn request(saved: Option<&WindowStateV2>) -> WindowPlacementRequest<'_> {
    WindowPlacementRequest {
        role: WindowPlacementRole::Main, saved, legacy_content_size: None,
        legacy_maximized: false, frame_insets: WindowFrameInsets::default(),
    }
}

/// 历史屏内坐标可完整落在两屏时拒绝猜测屏幕身份，但尺寸仍由恢复请求保留。
#[test]
fn legacy_migration_rejects_ambiguous_screen_local_coordinates() {
    let screens = [display("primary", 2000., 1200., 2.), display("secondary", 2000., 1200., 1.)];
    assert!(migrate_legacy_main_window_state(&screens, WindowDipPoint { x: 40., y: 40. },
        WindowDipSize { width: 1200., height: 800. }, WindowFrameInsets::default(), true).is_none());
}

/// 唯一完整解释保留旧位置与最大化；标题栏导致越界时不误认该屏。
#[test]
fn legacy_migration_requires_outer_frame_inside_unique_work_area() {
    let screens = [display("primary", 1000., 700., 2.), display("secondary", 1600., 1000., 1.)];
    let state = migrate_legacy_main_window_state(&screens, WindowDipPoint { x: 40., y: 40. },
        WindowDipSize { width: 1200., height: 800. }, WindowFrameInsets { top: 30., ..Default::default() }, true).unwrap();
    assert_eq!(state.display_key, "secondary"); assert!(state.maximized);
    assert_eq!(state.origin, WindowDipPoint { x: 40., y: 40. });
    assert!(migrate_legacy_main_window_state(&screens[1..], WindowDipPoint { x: 40., y: 200. },
        WindowDipSize { width: 1200., height: 800. }, WindowFrameInsets { top: 30., ..Default::default() }, false).is_none());
}

/// 不完整或非法旧内容尺寸不能借屏幕可用范围获得可信身份。
#[test]
fn legacy_migration_rejects_invalid_dimensions_and_non_finite_positions() {
    let screens = [display("primary", 2000., 1200., 2.)];
    assert!(migrate_legacy_main_window_state(&screens, WindowDipPoint { x: f64::NAN, y: 40. },
        WindowDipSize { width: 1200., height: 800. }, WindowFrameInsets::default(), false).is_none());
    assert!(migrate_legacy_main_window_state(&screens, WindowDipPoint { x: 40., y: 40. },
        WindowDipSize { width: 900., height: 800. }, WindowFrameInsets::default(), false).is_none());
}

/// 样本普通窗口处于横向可移动距离的四分之一、纵向的二分之一。
fn saved(display: &WindowDisplayArea) -> WindowStateV2 {
    capture_window_state(display, WindowDipPoint { x: 200., y: 274. },
        WindowDipSize { width: 1200., height: 700. }, WindowFrameInsets::default(), true).unwrap()
}

/// 最大化外框不会冒充普通尺寸；系统迁屏只映射普通锚点并更新真实身份与缩放。
#[test]
fn maximized_display_change_keeps_normal_size_and_maps_anchor() {
    let screens = [display("primary", 2000., 1200., 2.), display("secondary", 1600., 1000., 1.25)];
    let old = saved(&screens[0]);
    let next = reconcile_maximized_window_display(&screens, "secondary", &old, WindowFrameInsets::default()).unwrap();
    assert_eq!(next.display_key, "secondary");
    assert_eq!(next.content_size, old.content_size);
    assert_eq!(next.origin, WindowDipPoint { x: 100., y: 174. });
    assert_eq!(next.anchor, old.anchor);
    assert_eq!(next.scale, 1.25);
    assert_eq!(next.work_area, screens[1].work_area);
    assert!(next.maximized);
}

/// 当前屏已拔除、重复身份或先前记录损坏时保留原记录，不凭主屏生成新几何。
#[test]
fn maximized_display_change_rejects_unknown_ambiguous_and_invalid_state() {
    let primary = display("primary", 2000., 1200., 2.);
    let old = saved(&primary);
    assert!(reconcile_maximized_window_display(std::slice::from_ref(&primary), "removed", &old, WindowFrameInsets::default()).is_none());
    assert!(reconcile_maximized_window_display(&[primary.clone(), primary.clone()], "primary", &old, WindowFrameInsets::default()).is_none());
    let mut invalid = old; invalid.version = 99;
    assert!(reconcile_maximized_window_display(&[primary], "primary", &invalid, WindowFrameInsets::default()).is_none());
}

/// 有指针目标时即使保存屏和当前屏均有效，也只选择入口指针屏。
#[test]
fn pointer_display_wins_over_saved_and_current() {
    let screens = [display("primary", 2000., 1200., 2.), display("pointer", 2400., 1400., 1.25), display("current", 1800., 1100., 1.5)];
    let state = saved(&screens[0]);
    let plan = restore_window_placement(&screens, WindowDisplaySelection { pointer: Some("pointer"), current: Some("current") }, request(Some(&state))).unwrap();
    assert_eq!(plan.display_key, "pointer");
    assert_eq!(plan.content_size, state.content_size);
    assert!(plan.maximized);
    assert_eq!(plan.origin, WindowDipPoint { x: 300., y: 374. });
}

/// 鼠标取得失败、拔屏和当前屏丢失分别使用保存屏、当前屏和主屏。
#[test]
fn missing_displays_follow_deterministic_fallback_order() {
    let screens = [display("primary", 2000., 1200., 2.), display("saved", 1800., 1100., 1.)];
    let selection = WindowDisplaySelection { pointer: Some("removed"), current: Some("primary") };
    assert_eq!(choose_window_display(&screens, selection, Some("saved")).unwrap().key, "saved");
    assert_eq!(choose_window_display(&screens, selection, Some("removed")).unwrap().key, "primary");
    assert_eq!(choose_window_display(&screens, WindowDisplaySelection::default(), None).unwrap().key, "primary");
    assert!(choose_window_display(&[], selection, None).is_none());
}

/// 坏拓扑不能优先于有效当前屏；没有主屏标志时仍可选择有效屏。
#[test]
fn invalid_topology_is_skipped_without_panicking() {
    let mut bad = display("primary", 2000., 1200., 2.);
    bad.work_area.size.width = f64::NAN;
    let screens = [bad, display("current", 1600., 1000., 1.)];
    assert_eq!(choose_window_display(&screens, WindowDisplaySelection { pointer: Some("primary"), current: Some("current") }, None).unwrap().key, "current");
    assert_eq!(choose_window_display(&screens, WindowDisplaySelection::default(), None).unwrap().key, "current");
}

/// 相同屏幕保留普通位置，设备 scale 改变不会再次乘除屏内 DIP。
#[test]
fn same_display_preserves_local_dip_geometry_across_scale_changes() {
    let original = display("primary", 2000., 1200., 1.);
    let state = saved(&original);
    for scale in [1., 1.25, 1.5, 2.] {
        let screens = [display("primary", 2000., 1200., scale)];
        let plan = restore_window_placement(&screens, WindowDisplaySelection::default(), request(Some(&state))).unwrap();
        assert_eq!(plan.origin, state.origin);
        assert_eq!(plan.content_size, state.content_size);
    }
}

/// 工作区缩小时同屏位置只夹紧一次，不反复居中或覆盖普通内容尺寸。
#[test]
fn resized_work_area_clamps_same_display_without_resizing_content() {
    let original = display("primary", 2000., 1200., 2.);
    let mut state = saved(&original);
    state.origin = WindowDipPoint { x: 700., y: 500. };
    let screens = [display("primary", 1500., 900., 2.)];
    let plan = restore_window_placement(&screens, WindowDisplaySelection::default(), request(Some(&state))).unwrap();
    assert_eq!(plan.origin, WindowDipPoint { x: 300., y: 224. });
    assert_eq!(plan.content_size, state.content_size);
    assert!(!plan.overflows_work_area);
}

/// 无状态主窗按正常尺寸居中；有菜单栏时以可用区而非完整屏幕居中。
#[test]
fn default_main_window_is_centered_in_work_area() {
    let screens = [display("primary", 2000., 1200., 2.)];
    let plan = restore_window_placement(&screens, WindowDisplaySelection::default(), request(None)).unwrap();
    assert_eq!(plan.content_size, WindowDipSize { width: 1440., height: 900. });
    assert_eq!(plan.origin, WindowDipPoint { x: 280., y: 174. });
    assert!(!plan.maximized);
}

/// 屏小于主窗最小值时保留有效内容尺寸，同时将可操作标题区放在可用区起点。
#[test]
fn small_work_area_preserves_minimum_contract_and_reachable_title() {
    let screens = [display("primary", 800., 500., 1.)];
    let mut input = request(None);
    input.legacy_content_size = Some(WindowDipSize { width: 960., height: 640. });
    let plan = restore_window_placement(&screens, WindowDisplaySelection::default(), input).unwrap();
    assert_eq!(plan.content_size, WindowDipSize { width: 960., height: 640. });
    assert_eq!(plan.origin, screens[0].work_area.origin);
    assert!(plan.overflows_work_area);
}

/// 单轴边框使外框超出可用区时，仅该轴靠近可操作起点，另一轴仍居中。
#[test]
fn frame_only_overflow_preserves_other_axis_centering() {
    let screens = [display("primary", 1600., 660., 1.5)];
    let mut input = request(None);
    input.legacy_content_size = Some(WindowDipSize { width: 960., height: 640. });
    input.frame_insets = WindowFrameInsets { left: 8., right: 8., top: 32., bottom: 8. };
    let plan = restore_window_placement(&screens, WindowDisplaySelection::default(), input).unwrap();
    assert_eq!(plan.origin, WindowDipPoint { x: 312., y: 24. });
    assert_eq!(plan.content_size, WindowDipSize { width: 960., height: 640. });
    assert!(plan.overflows_work_area);
}

/// 新字段损坏时仅丢弃新记录，仍保留有效旧内容尺寸与最大化。
#[test]
fn malformed_new_state_falls_back_to_legacy_dimensions() {
    let screens = [display("primary", 2000., 1200., 2.)];
    let mut state = saved(&screens[0]);
    state.anchor.x = f64::NAN;
    let mut input = request(Some(&state));
    input.legacy_content_size = Some(WindowDipSize { width: 1100., height: 680. });
    input.legacy_maximized = true;
    let plan = restore_window_placement(&screens, WindowDisplaySelection::default(), input).unwrap();
    assert_eq!(plan.content_size, input.legacy_content_size.unwrap());
    assert_eq!(plan.origin, WindowDipPoint { x: 450., y: 284. });
    assert!(plan.maximized);
}

/// 不满足主窗最小合同的记录不能带入其位置；有效旧尺寸继续可用。
#[test]
fn undersized_main_state_is_rejected_independently() {
    let screens = [display("primary", 2000., 1200., 2.)];
    let mut state = saved(&screens[0]);
    state.content_size.width = 50.;
    let plan = restore_window_placement(&screens, WindowDisplaySelection::default(), request(Some(&state))).unwrap();
    assert_eq!(plan.content_size, WindowDipSize { width: 1440., height: 900. });
    assert!(!plan.maximized);
}

/// 无边框桌宠以实际布局尺寸位于鼠标屏右下，默认保留 16 DIP 距离。
#[test]
fn pet_default_uses_target_bottom_right_and_current_layout_size() {
    let screens = [display("primary", 2000., 1200., 2.), display("pointer", 1600., 1000., 1.)];
    let mut input = request(None);
    input.role = WindowPlacementRole::Pet(WindowDipSize { width: 256., height: 152. });
    let plan = restore_window_placement(&screens, WindowDisplaySelection { pointer: Some("pointer"), current: None }, input).unwrap();
    assert_eq!(plan.origin, WindowDipPoint { x: 1328., y: 856. });
    assert!(!plan.maximized);
}

/// 配置窗忽略主窗或桌宠旧状态，重开仍在入口目标屏居中且不最大化。
#[test]
fn pet_configuration_never_restores_persistent_window_geometry() {
    let screens = [display("primary", 2000., 1200., 2.)];
    let state = saved(&screens[0]);
    let mut input = request(Some(&state));
    input.role = WindowPlacementRole::PetConfiguration;
    input.legacy_maximized = true;
    let plan = restore_window_placement(&screens, WindowDisplaySelection::default(), input).unwrap();
    assert_eq!(plan.content_size, WindowDipSize { width: 320., height: 470. });
    assert_eq!(plan.origin, WindowDipPoint { x: 840., y: 389. });
    assert!(!plan.maximized);
}

/// 目标屏标题栏尺寸进入外框夹紧，但不改变保存的内容尺寸。
#[test]
fn frame_insets_are_included_in_clamping_and_capture_anchor() {
    let screens = [display("primary", 2000., 1200., 2.)];
    let insets = WindowFrameInsets { left: 8., top: 32., right: 8., bottom: 8. };
    let state = capture_window_state(&screens[0], WindowDipPoint { x: 392., y: 254. }, WindowDipSize { width: 1200., height: 700. }, insets, false).unwrap();
    assert_eq!(state.anchor, WindowDipPoint { x: 0.5, y: 0.5 });
    let mut input = request(Some(&state));
    input.frame_insets = insets;
    let plan = restore_window_placement(&screens, WindowDisplaySelection::default(), input).unwrap();
    assert_eq!(plan.origin, state.origin);
    assert_eq!(plan.content_size, state.content_size);
}

/// 同一旧位置可归属多个屏时拒绝猜测；损坏候选不算有效解释。
#[test]
fn legacy_position_requires_exactly_one_valid_interpretation() {
    let a = saved(&display("primary", 2000., 1200., 2.));
    let b = saved(&display("other", 2000., 1200., 1.));
    assert!(unique_legacy_window_state(&[]).is_none());
    assert_eq!(unique_legacy_window_state(std::slice::from_ref(&a)), Some(&a));
    assert!(unique_legacy_window_state(&[a.clone(), b]).is_none());
    let mut invalid = a.clone(); invalid.version = 3;
    assert_eq!(unique_legacy_window_state(&[a.clone(), invalid]), Some(&a));
}

/// 工作区局部负原点不会被缩放或归零，跨屏锚点仍按各自可移动区域计算。
#[test]
fn local_negative_work_origin_remains_valid() {
    let mut screen = display("primary", 2000., 1200., 1.5);
    screen.work_area.origin = WindowDipPoint { x: -20., y: -40. };
    let plan = restore_window_placement(&[screen], WindowDisplaySelection::default(), request(None)).unwrap();
    assert_eq!(plan.origin, WindowDipPoint { x: 260., y: 110. });
}

/// 拒绝非有限边距和几何，不能将原生读回失败伪装成可持久化状态。
#[test]
fn capture_and_restore_reject_non_finite_geometry() {
    let screen = display("primary", 2000., 1200., 2.);
    let invalid_insets = WindowFrameInsets { top: f64::INFINITY, ..WindowFrameInsets::default() };
    assert!(capture_window_state(&screen, WindowDipPoint { x: 0., y: 0. }, WindowDipSize { width: 1200., height: 700. }, invalid_insets, false).is_none());
    let mut input = request(None); input.frame_insets = invalid_insets;
    assert!(restore_window_placement(&[screen], WindowDisplaySelection::default(), input).is_none());
}

/// 持久化字段能够往返，且未知版本独立失败关闭。
#[test]
fn state_roundtrip_preserves_units_and_version() {
    let state = saved(&display("primary", 2000., 1200., 2.));
    let encoded = serde_json::to_string(&state).unwrap();
    let decoded: WindowStateV2 = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, state);
    assert!(decoded.is_valid());
    let mut future = decoded; future.version = 4;
    assert!(!future.is_valid());
}
