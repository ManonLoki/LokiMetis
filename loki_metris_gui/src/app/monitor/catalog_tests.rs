//! 使用真实窗口、原生事件、选择器响应和隔离文件服务覆盖监控编辑闭环。

use super::{Shell, state::{CatalogSuccess, Route}};
use gpui_kit::{AppContext, Context, Entity, TestAppContext, test::TestWindowExt};
use loki_metris_core::{AiProfileDraft, AiTool, HookBehavior, public_monitor_ai_tools};
use std::{sync::Arc, time::Duration};

/// 失败或正常退出都回收测试独占后台服务。
struct FinishService(Arc<crate::usage_service::UsageService>);
impl Drop for FinishService {
    /// 测试结束时关闭独占后台服务。
    fn drop(&mut self) {
        self.0.finish();
    }
}

/// Tokio 文件工作线程与 GPUI 假调度器之间有界等待，超时必须失败。
fn settle(cx: &mut TestAppContext, shell: &Entity<Shell>, done: impl Fn(&Shell) -> bool) {
    for _ in 0..500 {
        cx.run_until_parked();
        if shell.read_with(cx, |this, _| done(this)) {
            return;
        }
        cx.executor().advance_clock(Duration::from_millis(30));
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("monitor catalog did not settle");
}

/// 装载真实空档案与图库，不创建中性占位条目。
fn setup(
    this: &mut Shell,
    service: Arc<crate::usage_service::UsageService>,
    cx: &mut Context<Shell>,
) {
    this.page = crate::app::Page::Monitor;
    this.monitor.route = Route::Management;
    this.dashboard.service = Some(service);
    this.monitor.catalog.accept_profiles(
        public_monitor_ai_tools()
            .map(AiProfileDraft::default_for)
            .collect(),
    );
    this.monitor.catalog.images_loaded = true;
    cx.notify();
}

/// 逐帧只读真实图集；“全部帧都在”无法证明动画工作集，也不能证明全部释放。
fn resident_image_frames(window: &gpui_kit::Window, image: &gpui_kit::RenderImage) -> Vec<usize> {
    (0..image.frame_count())
        .filter(|&frame_index| {
            window.has_image_frame_atlas_entry(gpui_kit::RenderImageParams {
                image_id: image.id,
                frame_index,
            })
        })
        .collect()
}

/// 十二位真实按钮、跨 Agent 草稿和图片筛选均由窗口事件驱动。
#[gpui_kit::test]
fn management_controls_keep_per_agent_drafts_and_clear_unavailable_reference(
    cx: &mut TestAppContext,
) {
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    shell.update(cx, |this, cx| {
        this.page = crate::app::Page::Monitor;
        this.monitor.route = Route::Management;
        this.monitor.catalog.accept_profiles(
            public_monitor_ai_tools()
                .map(AiProfileDraft::default_for)
                .collect(),
        );
        this.monitor
            .catalog
            .editor_mut(AiTool::Codex)
            .unwrap()
            .draft
            .hooks[0]
            .image = "missing-id".into();
        this.monitor.catalog.images_loaded = true;
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let idle = window.find("monitor-behavior-idle").bounds();
        assert!(
            idle.size.height >= gpui_kit::px(350.),
            "four-behavior grid must not clip its form"
        );
        assert!(window.try_find("monitor-picker-clear-idle").is_some());
        window.click(("monitor-slot", 12usize), cx);
        assert_eq!(shell.read(cx).monitor.catalog.editors[0].draft.slot, 12);
        window.click("monitor-agent-Claude Code", cx);
        window.click(("monitor-slot", 4usize), cx);
        window.click("monitor-agent-Codex", cx);
        assert_eq!(shell.read(cx).monitor.catalog.editors[0].draft.slot, 12);
        window.click("monitor-picker-clear-idle", cx);
        assert!(
            shell.read(cx).monitor.catalog.editors[0].draft.hooks[0]
                .image
                .is_empty()
        );
    })
    .unwrap();
}

/// 保存期间继续输入不会被回执覆盖；保存的磁盘快照仍是提交时捕获的草稿。
#[gpui_kit::test]
fn save_receipt_preserves_newer_unsaved_text(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let service =
        crate::usage_service::UsageService::in_test_directory(root.path().join("data")).unwrap();
    let _finish = FinishService(service.clone());
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    shell.update(cx, |this, cx| setup(this, service.clone(), cx));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        shell.update(cx, |this, cx| {
            let input = this.monitor.catalog.editors[0].inputs.as_ref().unwrap()[0].clone();
            input.update(cx, |input, cx| input.replace_all("first", window, cx));
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.click("monitor-profile-save", cx);
        shell.update(cx, |this, cx| {
            let input = this.monitor.catalog.editors[0].inputs.as_ref().unwrap()[0].clone();
            input.update(cx, |input, cx| {
                input.replace_all("newer unsaved", window, cx)
            });
        });
    })
    .unwrap();
    settle(cx, &shell, |this| !this.monitor.catalog.saving);
    let saved = service
        .list_monitor_profile_drafts()
        .unwrap()
        .blocking_recv()
        .unwrap()
        .unwrap();
    assert_eq!(saved.drafts[0].hooks[0].content, "first");
    shell.read_with(cx, |this, _| {
        assert_eq!(
            this.monitor.catalog.editors[0].draft.hooks[0].content,
            "newer unsaved"
        );
        assert!(this.monitor.catalog.saved_tool.is_none());
    });
}

/// 文件选择器取消零写入；单图导入期间切换 Agent 仍绑定捕获的原草稿。
#[gpui_kit::test]
fn native_picker_cancel_and_import_receipt_follow_captured_agent(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("disposable-fixture.gif");
    std::fs::write(
        &path,
        include_bytes!("../../../tests/fixtures/monitor-loop.gif"),
    )
    .unwrap();
    let service =
        crate::usage_service::UsageService::in_test_directory(root.path().join("data")).unwrap();
    let _finish = FinishService(service.clone());
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    shell.update(cx, |this, cx| {
        setup(this, service.clone(), cx);
        this.monitor.catalog.feedback = Some(Ok(CatalogSuccess::ImagesUploaded { count: 4 }));
        this.upload_monitor_images(None, cx);
    });
    cx.simulate_path_prompt_response(|options| {
        assert!(options.multiple);
        None
    });
    settle(cx, &shell, |this| !this.monitor.catalog.uploading);
    shell.read_with(cx, |this, _| {
        assert_eq!(
            this.monitor
                .catalog
                .feedback
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap(),
            &CatalogSuccess::ImagesUploaded { count: 4 }
        )
    });
    assert!(
        service
            .list_monitor_images()
            .unwrap()
            .blocking_recv()
            .unwrap()
            .unwrap()
            .gallery
            .images
            .is_empty()
    );
    shell.update(cx, |this, cx| {
        this.upload_monitor_images(Some((AiTool::Codex, HookBehavior::Idle)), cx);
        this.select_monitor_tool(AiTool::ClaudeCode, cx);
    });
    cx.simulate_path_prompt_response(move |options| {
        assert!(!options.multiple);
        Some(vec![path])
    });
    settle(cx, &shell, |this| !this.monitor.catalog.uploading);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        shell.update(cx, |this, _| {
            assert_eq!(this.monitor_tool(), Some(AiTool::ClaudeCode));
            assert!(
                !this.monitor.catalog.editors[0].draft.hooks[0]
                    .image
                    .is_empty()
            );
            assert!(
                this.monitor.catalog.editors[1].draft.hooks[0]
                    .image
                    .is_empty()
            );
        });
    })
    .unwrap();
}

/// 上传回执跨页保留，切换语言后按当前词典重绘数量和原始文件名。
#[gpui_kit::test]
fn upload_feedback_follows_language_changes_without_reimport(cx: &mut TestAppContext) {
    use crate::preferences::LanguageChoice;
    for target in [None, Some((AiTool::Codex, HookBehavior::Idle))] {
        let root = tempfile::tempdir().unwrap();
        let filename = "审计 %{count} image.gif";
        let path = root.path().join(filename);
        std::fs::write(&path, include_bytes!("../../../tests/fixtures/monitor-loop.gif")).unwrap();
        let service = crate::usage_service::UsageService::in_test_directory(root.path().join("data")).unwrap();
        let _finish = FinishService(service.clone());
        let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
        shell.update(cx, |this, cx| {
            setup(this, service.clone(), cx);
            this.monitor.route = if target.is_some() { Route::Management } else { Route::Images };
            this.upload_monitor_images(target, cx);
        });
        cx.simulate_path_prompt_response(move |_| Some(vec![path]));
        settle(cx, &shell, |this| !this.monitor.catalog.uploading);
        let image_id = shell.read_with(cx, |this, _| this.monitor.catalog.images[0].id.clone());
        for (language, expected) in [
            (LanguageChoice::English, if target.is_some() {
                format!("Uploaded and automatically selected {filename}")
            } else { "Successfully uploaded 1 images.".into() }),
            (LanguageChoice::Chinese, if target.is_some() {
                format!("已上传并自动选中 {filename}")
            } else { "已成功上传 1 张图片。".into() }),
            (LanguageChoice::English, if target.is_some() {
                format!("Uploaded and automatically selected {filename}")
            } else { "Successfully uploaded 1 images.".into() }),
        ] {
            cx.update_window(handle, |_, window, cx| {
                shell.update(cx, |this, cx| {
                    this.navigate(crate::app::Page::Settings, cx);
                    this.select_language(language, window, cx);
                });
            }).unwrap();
            settle(cx, &shell, |this| this.preference_save.pending.is_none());
            cx.update_window(handle, |_, window, cx| {
                shell.update(cx, |this, cx| this.navigate(crate::app::Page::Monitor, cx));
                window.render_frame(cx);
                assert_eq!(window.find("monitor-catalog-feedback").label(), Some(expected.as_str()));
            }).unwrap();
            shell.read_with(cx, |this, _| {
                assert_eq!(this.preferences.language, language);
                assert_eq!(this.monitor.catalog.images.len(), 1);
                assert_eq!(this.monitor.catalog.images[0].id, image_id);
                if target.is_some() {
                    assert_eq!(this.monitor.catalog.editors[0].draft.hooks[0].image, image_id);
                }
            });
        }
    }
}

/// 有图片时格式区与右侧操作必须同处视口，不能仅存在于辅助树中。
#[gpui_kit::test]
fn gallery_toolbar_actions_remain_visible(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    shell.update(cx, |this, cx| {
        this.page = crate::app::Page::Monitor;
        this.monitor.route = Route::Images;
        this.monitor.catalog.images_loaded = true;
        this.monitor.catalog.images.push(super::state::ImageEntry {
            id: "fixture".into(),
            filename: "fixture.gif".into(),
            format: loki_metris_core::ImageFormat::Gif,
            source: Err(loki_metris_core::HookError::new(
                "error.monitor.imagesReadFailed",
            )),
            views: Default::default(),
            residency: Default::default(),
        });
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let page = window.find("monitor-images").bounds();
        for id in ["monitor-images-refresh", "monitor-images-upload"] {
            let bounds = window.find(id).bounds();
            assert!(
                bounds.size.width > gpui_kit::px(30.) && bounds.right() <= page.right(),
                "{id} must be inside {page:?}, got {bounds:?}"
            );
        }
    })
    .unwrap();
}

/// 三十个真实 GIF 只解码可见格子，离页后 CPU 结果与全部 GPU 帧同时释放。
#[gpui_kit::test]
fn gallery_decodes_visible_entries_and_releases_all_frames_on_leave(cx: &mut TestAppContext) {
    use crate::monitor::image_decode::{MonitorImageDecoder, MonitorImageSource};
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    let sources = (0..30)
        .map(|index| MonitorImageSource {
            id: format!("visible-fixture-{index}"),
            bytes: Arc::from(include_bytes!("../../../tests/fixtures/monitor-loop.gif").as_slice()),
            format: loki_metris_core::ImageFormat::Gif,
        })
        .collect::<Vec<_>>();
    shell.update(cx, |this, cx| {
        this.page = crate::app::Page::Monitor;
        this.monitor.route = Route::Images;
        this.monitor.catalog.images_loaded = true;
        this.monitor.catalog.images = sources
            .iter()
            .map(|source| super::state::ImageEntry {
                id: source.id.clone(),
                filename: format!("{}.gif", source.id),
                format: source.format,
                source: Ok(source.clone()),
                views: Default::default(),
                residency: Default::default(),
            })
            .collect();
        cx.notify();
    });
    for _ in 0..4 {
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        cx.run_until_parked();
    }
    let requested = shell.read_with(cx, |this, _| {
        this.monitor
            .catalog
            .images
            .iter()
            .filter(|entry| entry.residency.requested.get())
            .count()
    });
    cx.update_window(handle, |_, window, _| {
        let first = window.find("monitor-image-card-visible-fixture-0").bounds();
        let last = window
            .find("monitor-image-card-visible-fixture-29")
            .bounds();
        assert!(first.size.height > gpui_kit::px(300.));
        assert!(last.origin.y > gpui_kit::px(900.));
    })
    .unwrap();
    assert!(
        requested > 0 && requested < 30,
        "only visible entries may load; requested {requested}"
    );
    assert!(cx.read(|cx| cx.has_asset::<MonitorImageDecoder>(&sources[0])));
    assert!(!cx.read(|cx| cx.has_asset::<MonitorImageDecoder>(&sources[29])));
    let decoded = cx
        .update_window(handle, |_, window, cx| {
            window
                .get_asset::<MonitorImageDecoder>(&sources[0], cx)
                .unwrap()
                .unwrap()
        })
        .unwrap();
    let weak = Arc::downgrade(&decoded);
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let resident = resident_image_frames(window, &decoded.image);
        assert!(!resident.is_empty(), "可见 GIF 必须实际上传帧");
        #[cfg(target_os = "macos")]
        assert_eq!(resident.len(), 1, "单个可见位置仅保留当前实际绘制帧");
    })
    .unwrap();
    cx.simulate_window_visibility_change(handle, gpui_kit::WindowVisibility::Hidden);
    assert!(!cx.read(|cx| cx.has_asset::<MonitorImageDecoder>(&sources[0])));
    cx.update_window(handle, |_, window, cx| {
        assert!(resident_image_frames(window, &decoded.image).is_empty());
        // 隐藏后仍可能收到已排队绘帧，不得重新请求已释放资源。
        window.render_frame(cx);
    })
    .unwrap();
    assert!(!cx.read(|cx| cx.has_asset::<MonitorImageDecoder>(&sources[0])));
    cx.simulate_window_visibility_change(handle, gpui_kit::WindowVisibility::Visible);
    for _ in 0..4 {
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        cx.run_until_parked();
    }
    assert!(cx.read(|cx| cx.has_asset::<MonitorImageDecoder>(&sources[0])));
    let restored = cx.update_window(handle, |_, window, cx| {
        let restored = window.get_asset::<MonitorImageDecoder>(&sources[0], cx).unwrap().unwrap();
        assert_ne!(restored.image.id, decoded.image.id, "恢复确实产生新一代解码资源");
        let resident = resident_image_frames(window, &restored.image);
        assert!(!resident.is_empty(), "恢复后新一代图像实际上传帧");
        #[cfg(target_os = "macos")]
        assert_eq!(resident.len(), 1);
        restored
    }).unwrap();
    let restored_weak = Arc::downgrade(&restored);
    cx.update_window(handle, |_, window, cx| {
        shell.update(cx, |this, cx| {
            this.page = crate::app::Page::Settings;
            cx.notify();
        });
        window.render_frame(cx);
        assert!(resident_image_frames(window, &decoded.image).is_empty());
        assert!(resident_image_frames(window, &restored.image).is_empty());
    })
    .unwrap();
    for source in &sources {
        assert!(!cx.read(|cx| cx.has_asset::<MonitorImageDecoder>(source)));
    }
    drop(restored);
    assert!(restored_weak.upgrade().is_none(), "离页后新一代解码资源也必须释放");
    drop(decoded);
    assert!(
        weak.upgrade().is_none(),
        "no hidden view or pending cache may retain decoded pixels"
    );
}

/// 删除快照到达与窗口隐藏交错时，旧条目不能等待下一次绘帧才释放。
#[gpui_kit::test]
fn hidden_window_releases_retired_image_before_another_frame(cx: &mut TestAppContext) {
    use crate::monitor::image_decode::{MonitorImageDecoder, MonitorImageSource};
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    let source = MonitorImageSource {
        id: "retired-fixture".into(),
        bytes: Arc::from(include_bytes!("../../../tests/fixtures/monitor-loop.gif").as_slice()),
        format: loki_metris_core::ImageFormat::Gif,
    };
    shell.update(cx, |this, cx| {
        this.page = crate::app::Page::Monitor;
        this.monitor.route = Route::Images;
        this.monitor.catalog.images_loaded = true;
        this.monitor.catalog.images.push(super::state::ImageEntry {
            id: source.id.clone(),
            filename: "retired.gif".into(),
            format: source.format,
            source: Ok(source.clone()),
            views: Default::default(),
            residency: Default::default(),
        });
        cx.notify();
    });
    for _ in 0..4 {
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        cx.run_until_parked();
    }
    let decoded = cx
        .update_window(handle, |_, window, cx| {
            window
                .get_asset::<MonitorImageDecoder>(&source, cx)
                .unwrap()
                .unwrap()
        })
        .unwrap();
    cx.update_window(handle, |_, window, _| {
        assert!(!resident_image_frames(window, &decoded.image).is_empty(), "删除前真实图集有可释放的帧");
    }).unwrap();
    shell.update(cx, |this, _| {
        let old = this.monitor.catalog.images.pop().unwrap();
        this.monitor.catalog.retired_images.push(old);
    });
    cx.simulate_window_visibility_change(handle, gpui_kit::WindowVisibility::Hidden);
    assert!(shell.read_with(cx, |this, _| this.monitor.catalog.retired_images.is_empty()));
    assert!(!cx.read(|cx| cx.has_asset::<MonitorImageDecoder>(&source)));
    cx.update_window(handle, |_, window, _| {
        assert!(resident_image_frames(window, &decoded.image).is_empty())
    })
    .unwrap();
}
