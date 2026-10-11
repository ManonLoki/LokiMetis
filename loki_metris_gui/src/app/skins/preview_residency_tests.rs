//! 目录变更经生产采用入口和Canvas，验证真实Asset及图集身份，不清空整个缓存冒充退役。

use super::*;
use gpui_kit::{
    AnyWindowHandle, AppContext, Entity, Image, ImageFormat, RenderImageParams, TestAppContext,
    test::TestWindowExt,
};
use loki_metris_core::{ColorMode, SkinHostKind, SkinPackageType, SkinSource};

/// 小PNG保留真实编码与解码路径，不使用绕过Asset的Render来源。
fn image(color: [u8; 4]) -> Arc<Image> {
    let pixels = image::RgbaImage::from_pixel(4, 4, image::Rgba(color));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(pixels)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    Arc::new(Image::from_bytes(ImageFormat::Png, bytes.into_inner()))
}

/// 目录fixture只提供合法展示输入，缓存与绘制始终由生产逻辑建立。
fn entry(id: &str, preview: Arc<Image>) -> SkinEntry {
    SkinEntry {
        skin: crate::skins::SkinDescriptor {
            id: id.into(),
            source: SkinSource::Builtin,
            package_type: SkinPackageType::LegacySkin,
            name: id.into(),
            author: "test".into(),
            version: "0.1.0".into(),
            preview_data_url: String::new(),
            supported_color_modes: vec![ColorMode::Light],
        },
        preview: Some(preview),
    }
}

/// 采用入口只代表成功目录；外部回执的失败与generation检查另经真实异步测试覆盖。
fn adopt(shell: &Entity<Shell>, entries: Vec<SkinEntry>, cx: &mut TestAppContext) {
    shell.update(cx, |this, cx| {
        this.page = crate::app::Page::Skins;
        this.skins.host = Some(SkinHostKind::Codex);
        this.adopt_skin_catalog(entries, cx);
        cx.notify();
    });
}

/// 每轮经过正式render/prepaint/paint，再驱动真实后台Asset任务。
fn draw(handle: AnyWindowHandle, cx: &mut TestAppContext) {
    for _ in 0..4 {
        cx.update_window(handle, |_, window, app| window.render_frame(app))
            .unwrap();
        cx.run_until_parked();
    }
}

/// 必须先证明键存在，避免读取探针启动新的解码任务。
fn ready(source: &Arc<Image>, cx: &mut TestAppContext) -> Arc<RenderImage> {
    assert!(cx.read(|app| app.has_asset::<PreviewImageDecoder>(source)));
    cx.update(|app| {
        app.fetch_asset::<PreviewImageDecoder>(source)
            .expect("预览必须Ready")
            .expect("PNG必须成功")
    })
}

/// 精确查询实际输出第0帧，不上传、不探测加载源，也不把GPU物理页释放当成键退役。
fn in_atlas(handle: AnyWindowHandle, image: &Arc<RenderImage>, cx: &mut TestAppContext) -> bool {
    cx.update_window(handle, |_, window, _| {
        window.has_image_frame_atlas_entry(RenderImageParams {
            image_id: image.id,
            frame_index: 0,
        })
    })
    .unwrap()
}

/// 新Arc和元数据更新复用同内容；两卡共享时最后一个目录引用消失才清缓存与图集。
#[gpui_kit::test]
fn same_content_and_shared_cards_reuse_until_final_removal(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    let a = image([230, 30, 20, 255]);
    adopt(
        &shell,
        vec![entry("first", a.clone()), entry("second", a.clone())],
        cx,
    );
    draw(handle, cx);
    let decoded = ready(&a, cx);
    let weak = Arc::downgrade(&decoded);
    assert!(in_atlas(handle, &decoded, cx));
    let same = image([230, 30, 20, 255]);
    assert!(!Arc::ptr_eq(&a, &same));
    assert_eq!(a.id(), same.id());
    let mut changed = entry("second", same.clone());
    changed.skin.name = "metadata changed".into();
    adopt(&shell, vec![changed], cx);
    draw(handle, cx);
    assert!(Arc::ptr_eq(&decoded, &ready(&same, cx)));
    assert!(in_atlas(handle, &decoded, cx));
    shell.update(cx, |this, cx| {
        this.leave_skins();
        this.page = crate::app::Page::Settings;
        cx.notify();
    });
    draw(handle, cx);
    assert!(
        cx.read(|app| app.has_asset::<PreviewImageDecoder>(&a)),
        "离页保留当前目录缓存"
    );
    adopt(&shell, Vec::new(), cx);
    assert!(!cx.read(|app| app.has_asset::<PreviewImageDecoder>(&a)));
    assert!(!in_atlas(handle, &decoded, cx));
    drop(decoded);
    draw(handle, cx);
    assert!(weak.upgrade().is_none(), "弱登记不能继续拥有旧解码像素");
}

/// A→B→A保持内容身份但重新分配输出身份，旧退役不能误删重新出现的A。
#[gpui_kit::test]
fn replaced_preview_and_aba_retire_only_old_render_identity(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    let a = image([200, 10, 20, 255]);
    let b = image([10, 20, 200, 255]);
    adopt(&shell, vec![entry("skin", a.clone())], cx);
    draw(handle, cx);
    let old_a = ready(&a, cx);
    adopt(&shell, vec![entry("skin", b.clone())], cx);
    draw(handle, cx);
    let old_b = ready(&b, cx);
    assert!(!cx.read(|app| app.has_asset::<PreviewImageDecoder>(&a)));
    assert!(!in_atlas(handle, &old_a, cx));
    assert!(in_atlas(handle, &old_b, cx));
    let new_source = image([200, 10, 20, 255]);
    adopt(&shell, vec![entry("skin", new_source.clone())], cx);
    draw(handle, cx);
    let new_a = ready(&new_source, cx);
    assert_eq!(a.id(), new_source.id());
    assert_ne!(old_a.id, new_a.id);
    assert!(!in_atlas(handle, &old_a, cx));
    assert!(!in_atlas(handle, &old_b, cx));
    assert!(in_atlas(handle, &new_a, cx));
    assert!(!cx.read(|app| app.has_asset::<PreviewImageDecoder>(&b)));
}

/// pending和后台已完成但尚未发布均可退役，迟到任务不能把目录外内容放回缓存。
#[gpui_kit::test]
fn pending_and_worker_completed_previews_do_not_reappear_after_removal(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    for worker_completed in [false, true] {
        let source = image(if worker_completed {
            [70, 80, 90, 255]
        } else {
            [40, 50, 60, 255]
        });
        adopt(&shell, vec![entry("skin", source.clone())], cx);
        cx.update_window(handle, |_, window, app| window.render_frame(app))
            .unwrap();
        assert!(cx.read(|app| app.has_asset::<PreviewImageDecoder>(&source)));
        if worker_completed {
            while cx.dispatcher.tick(true) {}
        }
        assert!(
            cx.update(|app| app.fetch_asset::<PreviewImageDecoder>(&source))
                .is_none()
        );
        adopt(&shell, Vec::new(), cx);
        cx.run_until_parked();
        draw(handle, cx);
        assert!(!cx.read(|app| app.has_asset::<PreviewImageDecoder>(&source)));
    }
}

/// 真实失败结果同样占Asset键，目录删除后不能无限保留失败项。
#[gpui_kit::test]
fn failed_preview_asset_is_removed_with_its_catalog_entry(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    let source = Arc::new(Image::from_bytes(ImageFormat::Png, b"invalid PNG".to_vec()));
    adopt(&shell, vec![entry("broken", source.clone())], cx);
    draw(handle, cx);
    assert!(cx.read(|app| app.has_asset::<PreviewImageDecoder>(&source)));
    assert!(
        cx.update(|app| app.fetch_asset::<PreviewImageDecoder>(&source))
            .unwrap()
            .is_err()
    );
    adopt(&shell, Vec::new(), cx);
    assert!(!cx.read(|app| app.has_asset::<PreviewImageDecoder>(&source)));
}

/// Ready尚未被Canvas看到时没有图集，但正式采用入口仍必须清Asset并释放像素。
#[gpui_kit::test]
fn ready_before_prepaint_is_released_without_a_tracker_entry(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    let source = image([1, 2, 3, 255]);
    shell.update(cx, |this, cx| {
        this.adopt_skin_catalog(vec![entry("unpainted", source.clone())], cx);
        assert!(cx.fetch_asset::<PreviewImageDecoder>(&source).is_none());
    });
    cx.run_until_parked();
    let decoded = ready(&source, cx);
    let weak = Arc::downgrade(&decoded);
    assert!(!in_atlas(handle, &decoded, cx));
    adopt(&shell, Vec::new(), cx);
    assert!(!cx.read(|app| app.has_asset::<PreviewImageDecoder>(&source)));
    drop(decoded);
    assert!(weak.upgrade().is_none());
}

/// Metal测试的偏好仍只写内存，失败退出也回收本测试工作线程。
#[cfg(target_os = "macos")]
struct MetalWriter(crate::preferences::Writer);
#[cfg(target_os = "macos")]
impl Drop for MetalWriter {
    /// 不等待其他测试的后台资源，只收尾本实例偏好队列。
    fn drop(&mut self) {
        self.0.finish();
    }
}

/// 真实Metal连续替换16种内容，所有旧Asset、输出弱引用及图集键均退役，窗口始终存活。
#[cfg(target_os = "macos")]
#[test]
fn actual_metal_repeated_catalog_changes_retire_every_historical_preview() {
    use gpui_kit::{HeadlessAppContext, NoopTextSystem, PlatformAtlas, px, size};
    use std::{cell::RefCell, rc::Rc};
    let atlases: Rc<RefCell<Vec<Weak<dyn PlatformAtlas>>>> = Rc::default();
    let captured = atlases.clone();
    let mut cx = HeadlessAppContext::with_platform(
        Arc::new(NoopTextSystem),
        Arc::new(crate::assets::Assets),
        move || {
            let renderer =
                gpui_kit::platform::current_headless_renderer()?.expect("必须创建真实Metal渲染器");
            captured
                .borrow_mut()
                .push(Arc::downgrade(&renderer.sprite_atlas()));
            Ok(Some(renderer))
        },
    );
    cx.update(gpui_kit::init);
    let preferences = crate::preferences::Preferences {
        language: crate::preferences::LanguageChoice::English,
        theme: crate::preferences::ThemeChoice::Light,
        ..Default::default()
    };
    let writer = crate::preferences::Writer::in_memory(preferences.clone());
    let _finish = MetalWriter(writer.clone());
    let handle = cx
        .open_window(size(px(1440.), px(900.)), move |window, app| {
            app.new(|cx| {
                let mut shell = Shell::new(preferences, writer, window, cx);
                shell.agent_settings.value = Some(crate::monitor::settings::MonitorSettings {
                    enabled_ai_tools: loki_metris_core::public_monitor_ai_tools().collect(),
                    ..Default::default()
                });
                shell
            })
        })
        .unwrap();
    let shell = handle.update(&mut cx, |_, _, cx| cx.entity()).unwrap();
    let handle: AnyWindowHandle = handle.into();
    let mut history = Vec::new();
    for index in 0..16 {
        let source = image([index * 13, 90, 200, 255]);
        shell.update(&mut cx, |this, cx| {
            this.page = crate::app::Page::Skins;
            this.skins.host = Some(SkinHostKind::Codex);
            this.adopt_skin_catalog(vec![entry("changing", source.clone())], cx);
            cx.notify();
        });
        for _ in 0..4 {
            cx.update_window(handle, |_, window, app| window.render_frame(app))
                .unwrap();
            cx.run_until_parked();
        }
        assert!(cx.update(|app| app.has_asset::<PreviewImageDecoder>(&source)));
        let decoded = cx.update(|app| {
            app.fetch_asset::<PreviewImageDecoder>(&source)
                .unwrap()
                .unwrap()
        });
        let atlas = atlases.borrow()[0].upgrade().unwrap();
        assert!(
            atlas
                .get_or_insert_with(
                    RenderImageParams {
                        image_id: decoded.id,
                        frame_index: 0
                    }
                    .into(),
                    &mut || Ok(None)
                )
                .unwrap()
                .is_some()
        );
        for (old_source, old_id, old_weak) in &history {
            assert!(!cx.update(|app| app.has_asset::<PreviewImageDecoder>(old_source)));
            assert!(
                Weak::upgrade(old_weak).is_none(),
                "连续替换不能保留历史解码像素"
            );
            assert!(
                atlas
                    .get_or_insert_with(
                        RenderImageParams {
                            image_id: *old_id,
                            frame_index: 0
                        }
                        .into(),
                        &mut || Ok(None)
                    )
                    .unwrap()
                    .is_none()
            );
        }
        history.push((source, decoded.id, Arc::downgrade(&decoded)));
    }
    shell.update(&mut cx, |this, cx| {
        this.adopt_skin_catalog(Vec::new(), cx);
        cx.notify();
    });
    cx.update_window(handle, |_, window, app| window.render_frame(app))
        .unwrap();
    cx.run_until_parked();
    let atlas = atlases.borrow()[0].upgrade().unwrap();
    for (source, id, weak) in history {
        assert!(!cx.update(|app| app.has_asset::<PreviewImageDecoder>(&source)));
        assert!(weak.upgrade().is_none());
        assert!(
            atlas
                .get_or_insert_with(
                    RenderImageParams {
                        image_id: id,
                        frame_index: 0
                    }
                    .into(),
                    &mut || Ok(None)
                )
                .unwrap()
                .is_none()
        );
    }
}
