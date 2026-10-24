//! 真实桌宠窗口跨位置共用图库 ID；只观察缓存、解码结果与实际图元，不读取播放器私有状态。

use super::super::tests::{Fixture, fixture};
use crate::monitor_media::image_decode::{MonitorImageDecoder, MonitorImageSource};
use gpui_kit::{AppContext, TestAppContext, test::TestWindowExt};
use loki_metris_core::{
    AiProfileDraft, AiTool, HookBehavior, HookTransition, ImageFormat, PetPreferenceChange,
    PetSizeRange, change_pet_preferences,
};
use std::sync::Arc;

/// 两个正式行为投影指向同一不可变原图，位于默认二乘二布局的相邻两页。
fn shared_fixture(cx: &mut TestAppContext) -> (Fixture, MonitorImageSource) {
    let f = fixture(cx);
    let source = MonitorImageSource {
        id: "pet-shared-id".into(),
        bytes: Arc::from(&include_bytes!("../../../tests/fixtures/monitor-loop.gif")[..]),
        format: ImageFormat::Gif,
    };
    f.owner.update(cx, |owner, cx| {
        owner.drafts = [(AiTool::Codex, 1), (AiTool::ClaudeCode, 5)]
            .into_iter()
            .map(|(tool, slot)| {
                let mut draft = AiProfileDraft::default_for(tool);
                draft.slot = slot;
                draft.hooks[0].image = source.id.clone();
                draft
            })
            .collect();
        owner.images.insert(source.id.clone(), source.clone());
        let mut revision = 0;
        for (tool, slot) in [(AiTool::Codex, 0), (AiTool::ClaudeCode, 4)] {
            loki_metris_core::apply_pet_overlay_transition(
                &mut owner.states,
                &mut revision,
                tool,
                slot,
                HookTransition::Display(HookBehavior::Idle),
            );
        }
        cx.notify();
    });
    // 先完成真实 Asset 解码，下一次分页后若 cache 被删，必须在异步回调循环前失败。
    cx.update(|cx| {
        cx.fetch_asset::<MonitorImageDecoder>(&source);
    });
    let mut ready = false;
    for _ in 0..500 {
        cx.run_until_parked();
        ready = cx.update(|cx| cx.fetch_asset::<MonitorImageDecoder>(&source).is_some());
        if ready {
            break;
        }
    }
    assert!(ready, "真实共享 GIF 解码必须有界完成");
    cx.update_window(f.overlay, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(!window.painted_polychrome_sprites().is_empty());
    })
    .unwrap();
    (f, source)
}

/// 只用正式 core 页码变换驱动被测 renderer，不替换解码、窗口或资源清理路径。
fn turn(f: &Fixture, delta: isize, cx: &mut gpui_kit::App) {
    f.owner.update(cx, |owner, cx| {
        let old = owner.settings.pet_window;
        owner.settings.pet_window = change_pet_preferences(
            old,
            PetPreferenceChange::TurnPage(delta),
            PetSizeRange::for_work_area(900., old.layout),
        )
        .unwrap();
        cx.notify();
    });
}

/// 旧位置离屏后新位置必须沿用同一解码 Arc 和图集，不能在每帧移除当前页的 cache。
#[gpui_kit::test]
fn shared_id_survives_actual_window_page_change_without_redecode(cx: &mut TestAppContext) {
    let (f, source) = shared_fixture(cx);
    cx.update_window(f.overlay, |_, window, cx| {
        let original = window
            .get_asset::<MonitorImageDecoder>(&source, cx)
            .unwrap()
            .unwrap();
        let tile = window.painted_polychrome_sprites()[0].tile;
        turn(&f, 1, cx);
        window.render_frame(cx);
        let current = window
            .get_asset::<MonitorImageDecoder>(&source, cx)
            .expect("跨页复用同 ID 时不能使已完成资源重新进入 Loading")
            .unwrap();
        assert!(
            Arc::ptr_eq(&original, &current),
            "跨页不能复制相同素材的解码结果"
        );
        for _ in 0..8 {
            window.render_frame(cx);
            let current = window
                .get_asset::<MonitorImageDecoder>(&source, cx)
                .unwrap()
                .unwrap();
            assert!(Arc::ptr_eq(&original, &current));
            assert_eq!(window.painted_polychrome_sprites()[0].tile, tile);
        }
        turn(&f, -1, cx);
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(Arc::ptr_eq(
            &original,
            &window
                .get_asset::<MonitorImageDecoder>(&source, cx)
                .unwrap()
                .unwrap()
        ));
    })
    .unwrap();
}

/// 最后可见位置离开时释放 cache，之后空白帧不得继续删除其它 owner 新建的同 ID 资源。
#[gpui_kit::test]
fn last_visible_shared_id_retires_once_and_blank_frames_preserve_new_asset_owner(
    cx: &mut TestAppContext,
) {
    let (f, source) = shared_fixture(cx);
    let retired = cx
        .update_window(f.overlay, |_, window, cx| {
            let retired = Arc::downgrade(
                &window
                    .get_asset::<MonitorImageDecoder>(&source, cx)
                    .unwrap()
                    .unwrap(),
            );
            turn(&f, 1, cx);
            window.render_frame(cx);
            assert!(
                window
                    .get_asset::<MonitorImageDecoder>(&source, cx)
                    .is_some(),
                "共享页切换必须保留已有资源"
            );
            turn(&f, 1, cx);
            window.render_frame(cx);
            assert!(
                !cx.has_asset::<MonitorImageDecoder>(&source),
                "最后挂载离屏必须释放资源 cache"
            );
            assert!(window.painted_polychrome_sprites().is_empty());
            assert!(
                retired.upgrade().is_none(),
                "最后可见位置离开后不能残留解码像素 owner"
            );
            cx.fetch_asset::<MonitorImageDecoder>(&source);
            for _ in 0..8 {
                window.render_frame(cx);
                assert!(
                    cx.has_asset::<MonitorImageDecoder>(&source),
                    "离屏旧位置不能反复清理新 owner 的同 ID cache"
                );
            }
            retired
        })
        .unwrap();
    cx.run_until_parked();
    cx.update_window(f.overlay, |_, window, cx| {
        turn(&f, -2, cx);
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(
            window
                .get_asset::<MonitorImageDecoder>(&source, cx)
                .is_some(),
            "重新进入必须使用可读资源"
        );
        assert!(
            !window.painted_polychrome_sprites().is_empty(),
            "重新进入必须实际绘制图片"
        );
        assert!(
            retired.upgrade().is_none(),
            "重新进入不能复活已经释放的旧像素 owner"
        );
    })
    .unwrap();
}

/// 总量与质量重绘保留当前播放器实体、解码 Arc 和实际图片图元。
#[gpui_kit::test]
fn today_summary_updates_reuse_player_and_decoded_asset(cx: &mut TestAppContext) {
    use loki_metris_core::{Completeness, Confidence, TodayUsageDisplay};
    let (f, source) = shared_fixture(cx);
    let view = f
        .owner
        .read_with(cx, |owner, _| owner.overlay_view.clone().unwrap());
    let player = view.read_with(cx, |view, _| view.players.get(&0).unwrap().view.entity_id());
    let original = cx
        .update_window(f.overlay, |_, window, cx| {
            window
                .get_asset::<MonitorImageDecoder>(&source, cx)
                .unwrap()
                .unwrap()
        })
        .unwrap();
    for tokens in [0, 110, 100_000_000, u64::MAX] {
        f.owner.update(cx, |owner, cx| {
            owner.today = TodayUsageDisplay::Value {
                total_tokens: tokens,
                completeness: Completeness::Partial,
                confidence: Confidence::Derived,
            };
            cx.notify();
        });
        cx.update_window(f.overlay, |_, window, cx| {
            window.render_frame(cx);
            assert!(Arc::ptr_eq(
                &original,
                &window
                    .get_asset::<MonitorImageDecoder>(&source, cx)
                    .unwrap()
                    .unwrap()
            ));
            assert!(!window.painted_polychrome_sprites().is_empty());
        })
        .unwrap();
        assert_eq!(
            view.read_with(cx, |view, _| view.players.get(&0).unwrap().view.entity_id()),
            player
        );
    }
}
