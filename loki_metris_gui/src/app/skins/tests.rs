//! 原生窗口验证目录搜索、创建失败草稿和导入信任，不操作真实宿主。

use super::*;
use gpui_kit::{AppContext, Entity, TestAppContext, test::TestWindowExt};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// 宿主轮询关闭的是进程级owner，真实GUI回归各用独立测试进程。
fn isolated(name: &str) -> bool {
    const KEY: &str = "LOKI_METRIS_SKIN_UI_TEST";
    if std::env::var(KEY).as_deref() == Ok(name) {
        return false;
    }
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("app::skins::tests::{name}"),
            "--nocapture",
        ])
        .env(KEY, name)
        .status()
        .unwrap();
    assert!(status.success(), "isolated skin UI test failed: {name}");
    true
}
/// 测试退出仅回收独占资源服务，不启动任何CDP宿主查询。
struct Finish(Arc<crate::usage_service::UsageService>);
impl Drop for Finish {
    /// 测试结束时关闭独占换肤服务。
    fn drop(&mut self) {
        self.0.finish();
    }
}
/// 外部文件线程完成后推进真实GPUI回执，失败必须有界报告。
fn settle(cx: &mut TestAppContext, shell: &Entity<Shell>, done: impl Fn(&Shell) -> bool) {
    for _ in 0..500 {
        cx.run_until_parked();
        if shell.read_with(cx, |this, _| done(this)) {
            return;
        }
        cx.executor().advance_clock(Duration::from_millis(30));
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("skin UI operation did not settle");
}
/// 成功宿主快照是fixture，生产文件操作仍使用隔离UsageService。
fn setup(
    this: &mut Shell,
    service: Arc<crate::usage_service::UsageService>,
    entries: Vec<crate::usage_service::SkinEntry>,
    cx: &mut gpui_kit::Context<Shell>,
) {
    this.page = Page::Skins;
    this.dashboard.service = Some(service);
    this.skins.host = Some(SkinHostKind::Codex);
    this.skins.entries = Some(entries);
    this.skins.status = Some(crate::skins::SkinStatus {
        installed: false,
        skin_id: None,
        source: None,
        package_type: None,
        skin_name: None,
        version: String::new(),
        affected_pages: 0,
        compatibility: None,
        instance_id: None,
    });
    this.skins.last_read = Some(Instant::now());
    cx.notify();
}

/// 真实失败目录回执保留已绘制预览及缓存，不把失败误当成空库退役。
#[gpui_kit::test]
fn failed_catalog_receipt_keeps_preview_assets_and_entries(cx: &mut TestAppContext) {
    if isolated("failed_catalog_receipt_keeps_preview_assets_and_entries") { return; }
    let root = tempfile::tempdir().unwrap();
    let service = crate::usage_service::UsageService::in_test_directory(root.path().join("data")).unwrap();
    let _finish = Finish(service.clone());
    let entries = service.skin_catalog().unwrap().blocking_recv().unwrap().unwrap().entries;
    let preview = entries[0].preview.clone().unwrap();
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    shell.update(cx, |this, cx| setup(this, service.clone(), entries, cx));
    for _ in 0..4 {
        cx.update_window(handle, |_, window, app| window.render_frame(app)).unwrap();
        cx.run_until_parked();
    }
    assert!(cx.read(|app| app.has_asset::<super::preview::PreviewImageDecoder>(&preview)));
    service.finish();
    shell.update(cx, |this, cx| this.refresh_skins(true, cx));
    settle(cx, &shell, |this| !this.skins.loading);
    shell.read_with(cx, |this, _| {
        assert!(Arc::ptr_eq(this.skins.entries.as_ref().unwrap()[0].preview.as_ref().unwrap(), &preview));
        assert_eq!(this.skins.catalog_error.as_ref().unwrap().code, "usage_service_closed");
        assert_eq!(this.skins.host_error.as_ref().unwrap().code, "usage_service_closed");
        assert!(this.skins.status.is_none());
        assert!(!this.skin_writable());
    });
    assert!(cx.read(|app| app.has_asset::<super::preview::PreviewImageDecoder>(&preview)));
}

/// 已排队回执随后generation失效时，原目录、缓存、错误和读取时间均不可被迟到结果改写。
#[gpui_kit::test]
fn stale_catalog_receipt_does_not_retire_preview_or_update_state(cx: &mut TestAppContext) {
    if isolated("stale_catalog_receipt_does_not_retire_preview_or_update_state") { return; }
    let root = tempfile::tempdir().unwrap();
    let service = crate::usage_service::UsageService::in_test_directory(root.path().join("data")).unwrap();
    let _finish = Finish(service.clone());
    let entries = service.skin_catalog().unwrap().blocking_recv().unwrap().unwrap().entries;
    let preview = entries[0].preview.clone().unwrap();
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    shell.update(cx, |this, cx| setup(this, service.clone(), entries, cx));
    for _ in 0..4 {
        cx.update_window(handle, |_, window, app| window.render_frame(app)).unwrap();
        cx.run_until_parked();
    }
    service.finish();
    shell.update(cx, |this, cx| {
        this.refresh_skins(true, cx);
        assert!(this.skins.query.is_some());
        this.skins.generation = this.skins.generation.wrapping_add(1);
        this.skins.last_read = None;
    });
    // 已关闭服务同步返回错误；正式task仍保留并由前台调度器完整执行。
    cx.run_until_parked();
    shell.read_with(cx, |this, _| {
        assert!(this.skins.query.is_some());
        assert!(this.skins.loading);
        assert!(this.skins.last_read.is_none());
        assert!(this.skins.catalog_error.is_none());
        assert!(this.skins.host_error.is_none());
        assert!(this.skins.status.is_some());
        assert!(Arc::ptr_eq(this.skins.entries.as_ref().unwrap()[0].preview.as_ref().unwrap(), &preview));
    });
    assert!(cx.read(|app| app.has_asset::<super::preview::PreviewImageDecoder>(&preview)));
}
#[gpui_kit::test]
/// 四列卡片高度有限，实时搜索筛选真实库，离页返回仍保留搜索实体。
fn catalog_search_keeps_native_input_and_card_height(cx: &mut TestAppContext) {
    if isolated("catalog_search_keeps_native_input_and_card_height") {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let service =
        crate::usage_service::UsageService::in_test_directory(root.path().join("data")).unwrap();
    let _finish = Finish(service.clone());
    let entries = service
        .skin_catalog()
        .unwrap()
        .blocking_recv()
        .unwrap()
        .unwrap()
        .entries;
    let ids: Vec<_> = entries
        .iter()
        .map(|entry| format!("skin-card-{}-{:?}", entry.skin.id, entry.skin.source))
        .collect();
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1492., 900.);
    shell.update(cx, |this, cx| setup(this, service, entries, cx));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        for id in &ids {
            let bounds = window.find(id.clone()).bounds();
            assert!(
                bounds.size.height < gpui_kit::px(420.),
                "card stretched: {bounds:?}"
            );
        }
        let first = window.find(ids[0].clone()).bounds();
        let fourth = window.find(ids[3].clone()).bounds();
        assert_eq!(first.origin.y, fourth.origin.y);
        let search = shell.read(cx).skins.search.clone().unwrap();
        search.update(cx, |input, cx| input.set_value("Pikachu", window, cx));
        window.render_frame(cx);
        assert!(window.try_find("skin-card-pikachu-Builtin").is_some());
        assert!(window.try_find(ids[1].clone()).is_none());
        window.click("nav-settings", cx);
        window.click("nav-skins", cx);
        assert_eq!(shell.read(cx).skins.search.as_ref().unwrap(), &search);
        assert_eq!(search.read(cx).value().as_ref(), "Pikachu");
    })
    .unwrap();
}
#[gpui_kit::test]
/// 创建失败保留同一输入实体，回车不会只关闭模态丢失草稿。
fn failed_create_preserves_draft_and_enter_does_not_discard_dialog(cx: &mut TestAppContext) {
    if isolated("failed_create_preserves_draft_and_enter_does_not_discard_dialog") {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let source =
        crate::usage_service::UsageService::in_test_directory(root.path().join("source")).unwrap();
    let _source_finish = Finish(source.clone());
    let entries = source
        .skin_catalog()
        .unwrap()
        .blocking_recv()
        .unwrap()
        .unwrap()
        .entries;
    std::fs::create_dir_all(root.path().join("target")).unwrap();
    std::fs::write(root.path().join("target/skins"), b"blocked directory").unwrap();
    let service =
        crate::usage_service::UsageService::in_test_directory(root.path().join("target")).unwrap();
    let _finish = Finish(service.clone());
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    shell.update(cx, |this, cx| setup(this, service, entries, cx));
    let inputs = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("skin-create", cx);
            let ModalKind::Create(inputs) = &shell.read(cx).skins.modal.as_ref().unwrap().kind
            else {
                panic!("create dialog missing")
            };
            let inputs = inputs.clone();
            inputs[0].update(cx, |input, cx| {
                input.set_value("Keep this name", window, cx)
            });
            inputs[1].update(cx, |input, cx| input.set_value("Fixture", window, cx));
            window.render_frame(cx);
            window.click("skin-dialog-confirm", cx);
            inputs
        })
        .unwrap();
    settle(cx, &shell, |this| !this.skins.busy);
    shell.update(cx, |this, _| {
        assert!(matches!(this.skins.feedback, Some(Err(_))));
        assert!(matches!(
            this.skins.modal.as_ref().map(|modal| &modal.kind),
            Some(ModalKind::Create(_))
        ));
    });
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(inputs[0].read(cx).value().as_ref(), "Keep this name");
        let focus = shell.read(cx).skins.modal.as_ref().unwrap().focus.clone();
        window.focus(&focus, cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.simulate_keystrokes(handle, "enter");
    assert!(shell.read_with(cx, |this, _| this.skins.modal.is_some()));
}

#[gpui_kit::test]
/// 保存面板在离页后才返回时不创建文件；取消也释放入口预约。
fn late_export_picker_does_not_write_after_leaving_skins(cx: &mut TestAppContext) {
    if isolated("late_export_picker_does_not_write_after_leaving_skins") {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let service =
        crate::usage_service::UsageService::in_test_directory(root.path().join("data")).unwrap();
    let _finish = Finish(service.clone());
    let entries = service
        .skin_catalog()
        .unwrap()
        .blocking_recv()
        .unwrap()
        .unwrap()
        .entries;
    let reference = skin_reference(&entries[0].skin);
    let (_, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    shell.update(cx, |this, cx| setup(this, service, entries, cx));
    shell.update(cx, |this, cx| this.export_skin(reference.clone(), cx));
    assert!(shell.read_with(cx, |this, _| this.skins.picking));
    shell.update(cx, |this, _| {
        this.leave_skins();
        this.page = Page::Settings;
    });
    let destination = root.path().join("must-not-exist.zip");
    cx.simulate_new_path_selection(|_| Some(destination.clone()));
    settle(cx, &shell, |this| !this.skins.picking);
    assert!(!destination.exists());
    assert!(shell.read_with(cx, |this, _| this.skins.feedback.is_none()));
    shell.update(cx, |this, cx| {
        this.page = Page::Skins;
        this.export_skin(reference, cx);
    });
    cx.simulate_new_path_selection(|_| None);
    settle(cx, &shell, |this| !this.skins.picking);
    assert!(shell.read_with(cx, |this, _| this.skins.feedback.is_none()));
}

/// 成功回执保留原始名称，跨设置页切换语言后实际提示树必须同步译文。
#[gpui_kit::test]
fn skin_success_feedback_follows_language_changes(cx: &mut TestAppContext) {
    use crate::preferences::LanguageChoice;
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    let name = "审计 %{count} theme";
    shell.update(cx, |this, cx| {
        this.page = Page::Skins;
        this.skins.host = Some(SkinHostKind::Codex);
        this.skins.entries = Some(Vec::new());
        this.accept_skin_outcome(Outcome::Created(crate::skins::SkinDescriptor {
            id: "locale-fixture".into(), source: loki_metris_core::SkinSource::User,
            package_type: loki_metris_core::SkinPackageType::Theme,
            name: name.into(), author: "Fixture".into(), version: "1.0.0".into(),
            preview_data_url: String::new(), supported_color_modes: Vec::new(),
        }), cx);
        cx.notify();
    });
    assert!(cx.simulate_accessibility_activation(handle).is_some());
    for (language, expected) in [
        (LanguageChoice::English, format!("Created user theme {name}.")),
        (LanguageChoice::Chinese, format!("已创建用户主题 {name}。")),
        (LanguageChoice::English, format!("Created user theme {name}.")),
    ] {
        cx.update_window(handle, |_, window, cx| shell.update(cx, |this, cx| {
            this.navigate(Page::Settings, cx);
            this.select_language(language, window, cx);
        })).unwrap();
        settle(cx, &shell, |this| this.preference_save.pending.is_none());
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |this, cx| this.navigate(Page::Skins, cx));
            window.render_frame(cx);
            let tree = window.debug_a11y_tree_json().expect("实际皮肤提示树缺失");
            assert!(tree.contains(&expected), "提示未使用当前语言: {expected}\n{tree}");
        }).unwrap();
        shell.read_with(cx, |this, _| assert_eq!(this.preferences.language, language));
    }
}
