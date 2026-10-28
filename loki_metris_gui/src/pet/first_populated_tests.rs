//! 首图定位使用真实异步偏好服务和隔离文件；展示输入经正式core投影，不触碰用户配置。

use super::{tests::{Fixture, fixture, settle}, *};
use gpui_kit::TestAppContext;
use loki_metris_core::{AiTool, HookBehavior, HookTransition, ImageFormat};

/// 在默认页外构造一条正式迁移和可读素材，同时把观察代数置为当前服务值。
fn first_image_fixture(cx: &mut TestAppContext) -> Fixture {
    let f = fixture(cx);
    let image = crate::monitor_media::image_decode::MonitorImageSource {
        id: "first-image".into(),
        bytes: Arc::from(&include_bytes!("../../tests/fixtures/monitor-loop.gif")[..]),
        format: ImageFormat::Gif,
    };
    let mut draft = AiProfileDraft::default_for(AiTool::Codex);
    draft.slot = 10;
    draft.hooks.iter_mut().find(|hook| hook.behavior == HookBehavior::Idle)
        .unwrap().image = image.id.clone();
    f.owner.update(cx, |owner, _| {
        owner.drafts = vec![draft];
        owner.images.insert(image.id.clone(), image);
        let mut revision = 0;
        loki_metris_core::apply_pet_overlay_transition(
            &mut owner.states, &mut revision, AiTool::Codex, 9,
            HookTransition::Display(HookBehavior::Idle),
        );
        owner.observed = (
            f.service.agent_settings_revision(), f.service.pet_settings_revision(),
            f.service.monitor_catalog_revision(), u64::MAX,
        );
    });
    assert!(f.owner.read_with(cx, |owner, _| {
        let page = owner.page();
        page.has_any_image && !page.page_has_image && page.page_index == 0
    }));
    f
}

/// 损坏文件只能由测试自身修复；偏好服务必须真实报告错误且不产生新版本。
fn broken_settings(f: &Fixture) -> std::path::PathBuf {
    let config = f.root.path().join("monitor");
    std::fs::create_dir_all(&config).unwrap();
    let path = loki_metris_service::monitor::settings::settings_path(&config);
    std::fs::write(&path, b"{broken").unwrap();
    path
}

/// 仅恢复磁盘字节，不调用任何会改变服务代数的设置API。
fn repair_settings(path: &std::path::Path) {
    std::fs::write(path, serde_json::to_vec(&MonitorSettings::default()).unwrap()).unwrap();
}

/// 正常轮询必须在所有代数不变时重试失败的首图保存，同时限制失败重试频率。
#[gpui_kit::test]
fn first_image_failed_save_retries_on_poll_without_revision_change(cx: &mut TestAppContext) {
    let f = first_image_fixture(cx);
    let path = broken_settings(&f);
    f.owner.update(cx, |owner, cx| owner.focus_first_populated(cx));
    settle(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.focused_slot), 0);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.last_error.as_ref().map(|error| error.code)),
        Some("error.monitor.settingsInvalid"));
    assert_eq!(f.service.pet_settings_revision(), 0);
    assert_eq!(std::fs::read(&path).unwrap(), b"{broken");
    let revisions = f.owner.read_with(cx, |owner, _| owner.observed);

    repair_settings(&path);
    f.owner.update(cx, |owner, cx| owner.refresh(cx));
    settle(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.focused_slot), 0,
        "a failed automatic save must wait before retrying");
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.observed), revisions);
    cx.executor().advance_clock(Duration::from_secs(2));
    f.owner.update(cx, |owner, cx| owner.refresh(cx));
    settle(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.focused_slot), 9,
        "unchanged revisions must not permanently consume the automatic focus attempt");
    assert!(f.owner.read_with(cx, |owner, _| owner.last_error.is_none()));
    assert_eq!(f.service.pet_settings_revision(), 1);
    let saved = loki_metris_service::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
    assert_eq!(saved.pet_window.focused_slot, 9);
}

/// 首次定位成功后，实际手动翻页和后续轮询都保留用户选择，不重复保存首图位置。
#[gpui_kit::test]
fn first_image_success_does_not_override_later_manual_page(cx: &mut TestAppContext) {
    let f = first_image_fixture(cx);
    f.owner.update(cx, |owner, cx| {
        owner.focus_first_populated(cx);
        for _ in 0..5 { owner.refresh(cx); }
    });
    settle(cx, &f.owner);
    assert_eq!(f.service.pet_settings_revision(), 1, "in-flight polls must not queue duplicate focus writes");
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.focused_slot), 9);
    f.owner.update(cx, |owner, cx| owner.change(PetPreferenceChange::TurnPage(1), cx));
    settle(cx, &f.owner);
    cx.executor().advance_clock(Duration::from_secs(3));
    f.owner.update(cx, |owner, cx| owner.refresh(cx));
    settle(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.focused_slot), 0);
    assert_eq!(f.service.pet_settings_revision(), 2);
    let saved = loki_metris_service::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
    assert_eq!(saved.pet_window.focused_slot, 0);
}

/// 自动请求在途时接受手动分页，两次真实失败后手动重试成功，旧回执不能复活自动跳页。
fn pending_focus_keeps_manual_intent(change: PetPreferenceChange, expected: u8, cx: &mut TestAppContext) {
    let f = first_image_fixture(cx);
    let path = broken_settings(&f);
    f.owner.update(cx, |owner, cx| {
        owner.focus_first_populated(cx);
        owner.change(change, cx);
        assert_eq!(owner.pending.len(), 1);
    });
    settle(cx, &f.owner);
    assert_eq!(f.service.pet_settings_revision(), 0);
    assert_eq!(std::fs::read(&path).unwrap(), b"{broken");
    repair_settings(&path);
    cx.executor().advance_clock(Duration::from_secs(3));
    f.owner.update(cx, |owner, cx| owner.refresh(cx));
    settle(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.focused_slot), 0,
        "manual intent must cancel the older automatic retry even when its own save fails");
    assert_eq!(f.service.pet_settings_revision(), 0);

    f.owner.update(cx, |owner, cx| owner.change(change, cx));
    settle(cx, &f.owner);
    f.owner.update(cx, |owner, cx| owner.refresh(cx));
    settle(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.focused_slot), expected);
    assert_eq!(f.service.pet_settings_revision(), 1);
    let saved = loki_metris_service::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
    assert_eq!(saved.pet_window.focused_slot, expected);
}

/// 自动定位在途的手动上一页／下一页意图不被保存失败后的重试覆盖。
#[gpui_kit::test]
fn first_image_pending_failure_respects_manual_turn_page(cx: &mut TestAppContext) {
    pending_focus_keeps_manual_intent(PetPreferenceChange::TurnPage(1), 4, cx);
}

/// 显式选择槽位同样拥有用户语义，不能被旧自动请求的失败回执重新排入。
#[gpui_kit::test]
fn first_image_pending_failure_respects_manual_focused_slot(cx: &mut TestAppContext) {
    pending_focus_keeps_manual_intent(PetPreferenceChange::FocusedSlot(6), 6, cx);
}

/// 两个有图行为和图片都经真实服务持久化，只有Hook迁移作为正式core展示输入注入。
fn service_catalog_fixture(cx: &mut TestAppContext) -> (Fixture, AiProfileDraft) {
    let f = fixture(cx);
    let original = f.root.path().join("original.gif");
    std::fs::write(&original, include_bytes!("../../tests/fixtures/monitor-loop.gif")).unwrap();
    let imported = f.service.import_monitor_image(original.clone()).unwrap().blocking_recv().unwrap().unwrap();
    let other = f.service.import_monitor_image(original).unwrap().blocking_recv().unwrap().unwrap();
    let mut codex = AiProfileDraft::default_for(AiTool::Codex);
    codex.slot = 6;
    codex.hooks[0].image = imported.id.clone();
    let mut claude = AiProfileDraft::default_for(AiTool::ClaudeCode);
    claude.slot = 10;
    claude.hooks[0].image = other.id;
    let codex = f.service.save_monitor_profile_draft(codex).unwrap().blocking_recv().unwrap().unwrap();
    f.service.save_monitor_profile_draft(claude).unwrap().blocking_recv().unwrap().unwrap();
    let drafts = f.service.list_monitor_profile_drafts().unwrap().blocking_recv().unwrap().unwrap();
    let images = f.service.list_monitor_images().unwrap().blocking_recv().unwrap().unwrap();
    f.owner.update(cx, |owner, _| {
        owner.drafts = drafts.drafts;
        owner.accept_images(images);
        let mut revision = 0;
        for (tool, slot) in [(AiTool::Codex, 5), (AiTool::ClaudeCode, 9)] {
            loki_metris_core::apply_pet_overlay_transition(
                &mut owner.states, &mut revision, tool, slot,
                HookTransition::Display(HookBehavior::Idle),
            );
        }
        owner.observed = (
            f.service.agent_settings_revision(), f.service.pet_settings_revision(),
            f.service.monitor_catalog_revision(), u64::MAX,
        );
    });
    (f, codex)
}

/// 真正异步设置和图库快照都必须完成，按真实截止等待，不能把busy结束当作查询已经完成。
fn settle_catalog(cx: &mut TestAppContext, owner: &Entity<Controller>) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        cx.run_until_parked();
        if owner.read_with(cx, |owner, _| !owner.loading && !owner.display_applying && !owner.busy && owner.pending.is_empty()) {
            return;
        }
        cx.executor().advance_clock(Duration::from_millis(30));
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("pet catalog receipt did not settle within 30s of real time");
}

/// 首次原生几何独立保存画布位置；排空其防抖和回执，才能精确区分首图保存与位置保存次数。
fn settle_first_position(cx: &mut TestAppContext, owner: &Entity<Controller>) {
    cx.executor().advance_clock(Duration::from_millis(250));
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        cx.run_until_parked();
        if owner.read_with(cx, |owner, _| owner.position_task.is_none()) { return; }
        cx.executor().advance_clock(Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("initial geometry position receipt did not settle within 30s of real time");
}

/// 首图失败期间清除旧行为素材，重试必须读取新图库后选择另一页的真实有图位置。
#[gpui_kit::test]
fn first_image_retry_waits_for_changed_service_catalog(cx: &mut TestAppContext) {
    let (f, mut codex) = service_catalog_fixture(cx);
    let path = broken_settings(&f);
    f.owner.update(cx, |owner, cx| owner.focus_first_populated(cx));
    settle(cx, &f.owner);
    assert_eq!(f.service.pet_settings_revision(), 0);
    repair_settings(&path);
    let prior_catalog = f.service.monitor_catalog_revision();
    codex.hooks[0].image.clear();
    f.service.save_monitor_profile_draft(codex).unwrap().blocking_recv().unwrap().unwrap();
    assert_eq!(f.service.monitor_catalog_revision(), prior_catalog + 1);

    cx.executor().advance_clock(Duration::from_secs(2));
    f.owner.update(cx, |owner, cx| owner.refresh(cx));
    settle_catalog(cx, &f.owner);
    settle_first_position(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.focused_slot), 9,
        "retry must not commit the removed cached image at slot 5");
    assert!(f.owner.read_with(cx, |owner, _| owner.page().page_has_image));
    assert_eq!(f.service.pet_settings_revision(), 2, "focus and initial position must each commit once");
    let saved = loki_metris_service::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
    assert_eq!(saved.pet_window.focused_slot, 9);
    assert!(saved.pet_overlay_position.is_some(), "the second transaction must persist the native position");
    f.owner.update(cx, |owner, cx| owner.refresh(cx));
    settle_catalog(cx, &f.owner);
    settle_first_position(cx, &f.owner);
    assert_eq!(f.service.pet_settings_revision(), 2, "later polls must not repeat either commit");
}

/// 查询在途时服务再次提交档案，旧代数回执不能消费首图机会，即使数据恰好读到新内容。
#[gpui_kit::test]
fn first_image_query_receipt_rechecks_current_catalog_revision(cx: &mut TestAppContext) {
    let (f, mut codex) = service_catalog_fixture(cx);
    f.owner.update(cx, |owner, cx| {
        // 模拟真实启动的首次几何／快照加载，尚不能消费首图定位。
        owner.geometry_restored = false;
        owner.observed.2 = u64::MAX;
        owner.refresh(cx);
    });
    assert!(f.owner.read_with(cx, |owner, _| owner.loading));
    codex.hooks[0].image.clear();
    f.service.save_monitor_profile_draft(codex).unwrap().blocking_recv().unwrap().unwrap();

    settle_catalog(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.focused_slot), 0,
        "a completed query from an older catalog version must not commit focus");
    // 首次原生几何会独立保存画布位置，不能把其时序与首图分页提交混为一次计数。
    settle_first_position(cx, &f.owner);
    let before_focus = f.service.pet_settings_revision();
    assert!(before_focus <= 1, "initial position must not create a save loop");
    let initial_saved = loki_metris_service::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
    assert!(initial_saved.pet_overlay_position.is_some(), "initial position must actually persist");
    assert_eq!(initial_saved.pet_window.focused_slot, 0,
        "older catalog receipt must not commit focus even after position persistence");
    f.owner.update(cx, |owner, cx| owner.refresh(cx));
    settle_catalog(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.focused_slot), 9);
    assert!(f.owner.read_with(cx, |owner, _| owner.page().page_has_image));
    assert_eq!(f.service.pet_settings_revision(), before_focus + 1, "current catalog must commit focus exactly once");
    assert_eq!(loki_metris_service::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap().pet_window.focused_slot, 9);
    f.owner.update(cx, |owner, cx| owner.refresh(cx));
    settle_catalog(cx, &f.owner);
    assert_eq!(f.service.pet_settings_revision(), before_focus + 1);
    assert!(f.owner.read_with(cx, |owner, _| owner.first_populated == FirstPopulatedFocus::Done && owner.pending.is_empty()));
}

/// 部分查询失败不能消费旧图库；原样修复且图库代数不变，周期刷新仍恢复定位。
#[gpui_kit::test]
fn first_image_partial_snapshot_failure_retries_without_new_catalog_revision(cx: &mut TestAppContext) {
    let (f, _) = service_catalog_fixture(cx);
    let path = broken_settings(&f);
    f.owner.update(cx, |owner, cx| owner.focus_first_populated(cx));
    settle(cx, &f.owner);
    repair_settings(&path);
    let profiles_path = f.root.path().join("monitor/monitor-profiles.json");
    let profiles = std::fs::read(&profiles_path).unwrap();
    std::fs::write(&profiles_path, b"{broken").unwrap();
    // 使用真实偏好变化触发快照刷新；默认未锁定时再次解锁应为 noop。
    f.service.change_pet_preferences(PetPreferenceChange::Locked(true), PetSizeRange { min: 64, max: 135 })
        .unwrap().blocking_recv().unwrap().unwrap();
    let catalog_revision = f.service.monitor_catalog_revision();
    cx.executor().advance_clock(Duration::from_secs(2));
    f.owner.update(cx, |owner, cx| owner.refresh(cx));
    settle_catalog(cx, &f.owner);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.focused_slot), 0,
        "partial catalog reads must not treat the cached first image as current");
    assert_eq!(f.service.pet_settings_revision(), 1);

    std::fs::write(profiles_path, profiles).unwrap();
    cx.executor().advance_clock(Duration::from_secs(2));
    for _ in 0..10 {
        f.owner.update(cx, |owner, cx| owner.refresh(cx));
        settle_catalog(cx, &f.owner);
        if f.owner.read_with(cx, |owner, _| owner.settings.pet_window.focused_slot == 5) {
            break;
        }
        // 原生bounds观察可能恰好提交位置，和生产100ms轮询一样再取当前偏好版本。
        cx.executor().advance_clock(Duration::from_millis(100));
    }
    assert_eq!(f.service.monitor_catalog_revision(), catalog_revision);
    assert_eq!(f.owner.read_with(cx, |owner, _| owner.settings.pet_window.focused_slot), 5);
    assert!(f.owner.read_with(cx, |owner, _| owner.page().page_has_image));
    let saved = loki_metris_service::monitor::settings::load_monitor_settings(&f.root.path().join("monitor")).unwrap();
    assert_eq!(saved.pet_window.focused_slot, 5);
}
