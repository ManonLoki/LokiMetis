//! 桌宠与另一原生播放器共用图片，正式分页和关闭必须遵守全 App 最后 owner 清理。

use super::super::tests::{Fixture, fixture};
use crate::monitor::{
    image_decode::{DecodedMonitorImage, MonitorImageDecoder, MonitorImageSource},
    image_playback::MonitorImageView,
};
use gpui_kit::{
    AnyWindowHandle, AppContext, Context, Entity, IntoElement, ObjectFit, Render, TestAppContext,
    Window, div, prelude::*, test::TestWindowExt,
};
use loki_metris_core::{
    AiProfileDraft, AiTool, HookBehavior, HookTransition, ImageFormat, PetPreferenceChange,
    PetSizeRange, change_pet_preferences,
};
use std::sync::Arc;

/// 空投影模拟真实位置卸载，图片仍由生产播放器完整绘制。
struct PlaybackWindow {
    image: Entity<MonitorImageView>,
    shown: bool,
}
impl Render for PlaybackWindow {
    /// 不代理资源清理或图元，只选择当前真实视图投影。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .when(self.shown, |div| div.child(self.image.clone()))
            .into_any_element()
    }
}

/// 测试额外强持播放器，覆盖窗口已关闭但 Entity 尚未释放的原生路径。
struct SharedWindows {
    pet: Fixture,
    other: AnyWindowHandle,
    root: Entity<PlaybackWindow>,
    image: Entity<MonitorImageView>,
    source: MonitorImageSource,
}

/// 一个正式行为只在第一页使用原图，下一页为空且由 core 决定页码。
fn shared_windows(cx: &mut TestAppContext) -> SharedWindows {
    let pet = fixture(cx);
    let source = MonitorImageSource {
        id: "pet-other-window-shared-gif".into(),
        bytes: Arc::from(&include_bytes!("../../../tests/fixtures/monitor-loop.gif")[..]),
        format: ImageFormat::Gif,
    };
    pet.owner.update(cx, |owner, cx| {
        let mut draft = AiProfileDraft::default_for(AiTool::Codex);
        draft.slot = 1;
        draft.hooks[0].image = source.id.clone();
        owner.drafts = vec![draft];
        owner.images.insert(source.id.clone(), source.clone());
        let mut revision = 0;
        loki_metris_core::apply_pet_overlay_transition(
            &mut owner.states,
            &mut revision,
            AiTool::Codex,
            0,
            HookTransition::Display(HookBehavior::Idle),
        );
        cx.notify();
    });
    let other_source = source.clone();
    let other = cx.add_window(move |window, cx| {
        let image =
            cx.new(|cx| MonitorImageView::new(other_source, ObjectFit::Contain, window, cx));
        image.update(cx, |image, cx| image.set_mounted(true, window, cx));
        PlaybackWindow { image, shown: true }
    });
    let root = other.update(cx, |_, _, cx| cx.entity()).unwrap();
    let image = root.read_with(cx, |root, _| root.image.clone());
    SharedWindows {
        pet,
        other: other.into(),
        root,
        image,
        source,
    }
}

/// 两个真实窗口完成解码及实际绘制后返回同一个缓存 Arc。
fn draw_ready(f: &SharedWindows, cx: &mut TestAppContext) -> Arc<DecodedMonitorImage> {
    for _ in 0..500 {
        for handle in [f.pet.overlay, f.other] {
            cx.update_window(handle, |_, window, app| {
                window.render_frame(app);
                window.render_frame(app);
            })
            .unwrap();
        }
        cx.run_until_parked();
        let painted = [f.pet.overlay, f.other].into_iter().all(|handle| {
            cx.update_window(handle, |_, window, _| {
                !window.painted_polychrome_sprites().is_empty()
            })
            .unwrap()
        });
        if painted {
            assert!(cx.read(|app| app.has_asset::<MonitorImageDecoder>(&f.source)));
            return cx
                .update_window(f.other, |_, window, app| {
                    window
                        .get_asset::<MonitorImageDecoder>(&f.source, app)
                        .unwrap()
                        .unwrap()
                })
                .unwrap();
        }
    }
    panic!("桌宠和第二播放器必须有界完成真实 GIF 绘制");
}

/// 只采用正式 core 偏好变换切换空白页，不写播放器或缓存的内部状态。
fn turn_to_blank(f: &SharedWindows, cx: &mut TestAppContext) {
    f.pet.owner.update(cx, |owner, cx| {
        let old = owner.settings.pet_window;
        owner.settings.pet_window = change_pet_preferences(
            old,
            PetPreferenceChange::TurnPage(1),
            PetSizeRange::for_work_area(900., old.layout),
        )
        .unwrap();
        cx.notify();
    });
    cx.update_window(f.pet.overlay, |_, window, app| {
        window.render_frame(app);
        window.render_frame(app);
        assert!(window.painted_polychrome_sprites().is_empty());
    })
    .unwrap();
}

/// 桌宠先翻到空白页不能驱逐第二窗口；最后播放器卸载才清两个存活窗的图集和像素。
#[gpui_kit::test]
fn pet_page_exit_preserves_other_window_until_final_actual_unmount(cx: &mut TestAppContext) {
    let f = shared_windows(cx);
    let decoded = draw_ready(&f, cx);
    let weak = Arc::downgrade(&decoded);
    let tile = cx
        .update_window(f.other, |_, window, _| {
            window.painted_polychrome_sprites()[0].tile
        })
        .unwrap();
    turn_to_blank(&f, cx);
    assert!(
        cx.read(|app| app.has_asset::<MonitorImageDecoder>(&f.source)),
        "桌宠离屏不能删除第二窗口的实际 owner"
    );
    cx.update_window(f.other, |_, window, app| {
        window.render_frame(app);
        assert!(Arc::ptr_eq(
            &decoded,
            &window
                .get_asset::<MonitorImageDecoder>(&f.source, app)
                .unwrap()
                .unwrap()
        ));
        assert_eq!(window.painted_polychrome_sprites()[0].tile, tile);
        f.image
            .update(app, |view, cx| view.set_mounted(false, window, cx));
        f.root.update(app, |root, cx| {
            root.shown = false;
            cx.notify();
        });
        window.render_frame(app);
    })
    .unwrap();
    assert!(!cx.read(|app| app.has_asset::<MonitorImageDecoder>(&f.source)));
    for handle in [f.pet.overlay, f.other] {
        cx.update_window(handle, |_, window, _| {
            assert!(!window.has_image_atlas_entry(&decoded.image));
        })
        .unwrap();
    }
    drop(decoded);
    assert!(
        weak.upgrade().is_none(),
        "最后 owner 离开后必须释放真实解码像素"
    );
}

/// 第二窗口先关闭且外部强持 view，桌宠仍沿用同 Arc，随后空白页释放最后资源。
#[gpui_kit::test]
fn other_window_close_preserves_pet_then_pet_page_exit_retires_pixels(cx: &mut TestAppContext) {
    let f = shared_windows(cx);
    let decoded = draw_ready(&f, cx);
    let weak = Arc::downgrade(&decoded);
    let tile = cx
        .update_window(f.pet.overlay, |_, window, _| {
            window.painted_polychrome_sprites()[0].tile
        })
        .unwrap();
    cx.update_window(f.other, |_, window, _| window.remove_window())
        .unwrap();
    assert!(cx.update_window(f.other, |_, _, _| ()).is_err());
    assert!(!f.image.read_with(cx, |view, _| view.is_decoded()));
    assert!(
        cx.read(|app| app.has_asset::<MonitorImageDecoder>(&f.source)),
        "关闭第二窗口不能清理桌宠正在使用的资源"
    );
    cx.update_window(f.pet.overlay, |_, window, app| {
        window.render_frame(app);
        assert!(Arc::ptr_eq(
            &decoded,
            &window
                .get_asset::<MonitorImageDecoder>(&f.source, app)
                .unwrap()
                .unwrap()
        ));
        assert_eq!(window.painted_polychrome_sprites()[0].tile, tile);
    })
    .unwrap();
    turn_to_blank(&f, cx);
    assert!(!cx.read(|app| app.has_asset::<MonitorImageDecoder>(&f.source)));
    cx.update_window(f.pet.overlay, |_, window, _| {
        assert!(!window.has_image_atlas_entry(&decoded.image));
    })
    .unwrap();
    drop(decoded);
    assert!(
        weak.upgrade().is_none(),
        "已关闭且被强持的 view 不应保留解码像素"
    );
}
