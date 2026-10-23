//! 真实生产壳层的多行目录不能压掉正文，短窗必须以完整卡片进入原生滚动范围。

use super::*;
use gpui_kit::{Bounds, FontWeight, Pixels, ScrollDelta, SharedString, TextRun, Window, point, px, size};

/// 用完整边界证明子控件没有被卡片或滚动视口裁切，允许半个逻辑像素的取整差。
fn inside(outer: Bounds<Pixels>, inner: Bounds<Pixels>, label: &str) {
    let tolerance = px(0.51);
    assert!(inner.left() >= outer.left() - tolerance && inner.right() <= outer.right() + tolerance
        && inner.top() >= outer.top() - tolerance && inner.bottom() <= outer.bottom() + tolerance,
        "{label}被裁切：outer={outer:?}, inner={inner:?}");
}

/// 原生文字排版提供完整译文的固有宽度，不把已经截断的可访问名称当绘制证据。
fn label_width(window: &Window, text: String, weight: FontWeight) -> Pixels {
    let mut font = gpui_kit::font("Geist");
    font.weight = weight;
    let text = SharedString::from(text);
    let run = TextRun { len: text.len(), font, ..Default::default() };
    window.text_system().shape_line(text, px(14.), &[run], None).width
}

/// 六张真实目录卡在宽窄窗口、双语与缩放往返时保留正文和按钮，末行可实际滚入视口。
#[gpui_kit::test]
fn cards_keep_metadata_and_scroll_in_minimum_windows(cx: &mut TestAppContext) {
    if isolated("layout::cards_keep_metadata_and_scroll_in_minimum_windows") { return; }
    let root = tempfile::tempdir().unwrap();
    let service = crate::usage_service::UsageService::in_test_directory(root.path().join("data")).unwrap();
    let _finish = Finish(service.clone());
    let entries = service.skin_catalog().unwrap().blocking_recv().unwrap().unwrap().entries;
    assert_eq!(entries.len(), 6, "必须覆盖生产六卡多行目录");
    let ids: Vec<_> = entries.iter().map(|entry| {
        let suffix = format!("{}-{:?}", entry.skin.id, entry.skin.source);
        let mut badges = vec![format!("skin-kind-{suffix}"), format!("skin-source-{suffix}")];
        badges.extend(entry.skin.supported_color_modes.iter().map(|mode| format!("skin-mode-{suffix}-{mode:?}")));
        (format!("skin-card-{suffix}"), format!("skin-preview-{suffix}"),
            format!("skin-metadata-{suffix}"), format!("skin-apply-{suffix}"), badges)
    }).collect();
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    shell.update(cx, |this, cx| setup(this, service, entries, cx));
    for locale in ["en-US", "zh-CN"] {
        shell.update(cx, |this, cx| { this.locale = locale; cx.notify(); });
        for scale in [1., 1.5, 2.] {
            cx.simulate_window_scale_factor_change(handle, scale);
            for (width, height) in [(1440., 900.), (960., 640.), (1440., 900.)] {
                cx.simulate_window_resize(handle, size(px(width), px(height)));
                cx.update_window(handle, |_, window, cx| {
                    let scroll = shell.read(cx).page_scroll.handle.clone();
                    scroll.set_offset(point(px(0.), px(0.)));
                    window.render_frame(cx);
                    for (card_id, preview_id, metadata_id, apply_id, badges) in &ids {
                        let card = window.find(card_id.clone()).bounds();
                        let preview = window.find(preview_id.clone()).bounds();
                        let metadata = window.find(metadata_id.clone()).bounds();
                        let apply = window.find(apply_id.clone()).bounds();
                        assert_eq!(preview.size.height, px(160.));
                        assert!((metadata.top() - preview.bottom()).abs() <= px(0.51));
                        assert!(metadata.size.height >= apply.size.height + px(32.), "正文须容纳完整按钮与上下内距");
                        assert_eq!(apply.size.height, px(36.));
                        inside(card, metadata, metadata_id);
                        inside(metadata, apply, apply_id);
                        for id in badges {
                            let badge = window.find(id.clone()).bounds();
                            assert_eq!(badge.size.height, px(20.));
                            inside(metadata, badge, id);
                            assert!(badge.bottom() <= apply.top());
                        }
                    }
                    if height == 640. {
                        assert!(scroll.max_offset().y > px(0.), "多行完整卡片应产生纵向滚动而不是被压缩");
                        assert!(scroll.max_offset().x <= px(0.51), "不能以页面横向溢出换取正文空间");
                        window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), px(-10000.))), cx);
                        assert!(scroll.offset().y < px(0.));
                        for (_, _, metadata_id, apply_id, _) in ids.iter().rev().take(2) {
                            inside(scroll.bounds(), window.find(metadata_id.clone()).bounds(), metadata_id);
                            inside(scroll.bounds(), window.find(apply_id.clone()).bounds(), apply_id);
                        }
                        window.scroll("page-content", ScrollDelta::Pixels(point(px(0.), px(10000.))), cx);
                        assert!(scroll.offset().y.abs() <= px(0.51));
                        inside(scroll.bounds(), window.find(ids[0].3.clone()).bounds(), &ids[0].3);
                    }
                }).unwrap();
            }
        }
    }
}

/// 双语工具栏的完整文字、图标与动作必须在短窗内自然换行，反馈提示不能挤掉卡片正文。
#[gpui_kit::test]
fn toolbar_fits_translated_actions_and_feedback_without_clipping(cx: &mut TestAppContext) {
    if isolated("layout::toolbar_fits_translated_actions_and_feedback_without_clipping") { return; }
    let root = tempfile::tempdir().unwrap();
    let service = crate::usage_service::UsageService::in_test_directory(root.path().join("data")).unwrap();
    let _finish = Finish(service.clone());
    let entries = service.skin_catalog().unwrap().blocking_recv().unwrap().unwrap().entries;
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 960., 640.);
    shell.update(cx, |this, cx| setup(this, service, entries, cx));
    for locale in ["en-US", "zh-CN"] {
        shell.update(cx, |this, cx| {
            this.locale = locale;
            this.skins.feedback = Some(Ok(SkinSuccess::Exported));
            cx.notify();
        });
        cx.update_window(handle, |_, window, cx| {
            let scroll = shell.read(cx).page_scroll.handle.clone();
            scroll.set_offset(point(px(0.), px(0.)));
            window.render_frame(cx);
            let viewport = scroll.bounds();
            for id in ["skin-import", "skin-create", "skin-refresh"] {
                inside(viewport, window.find(id).bounds(), id);
            }
            let import = window.find("skin-import");
            let label = gpui_kit::base::test_support::find(window, import.path(), &"historical-button-label".into()).unwrap().bounds();
            let icon = gpui_kit::base::test_support::find(window, import.path(), &"historical-button-icon".into()).unwrap().bounds();
            assert_eq!(icon.size, size(px(16.), px(16.)));
            assert!(label.size.width + px(0.51) >= label_width(window, shell.read(cx).t("skins.action.import"), FontWeight::SEMIBOLD));
            assert!(label.right() <= import.bounds().right() - px(18.) + px(0.51));
            let create = window.find("skin-create").bounds();
            assert!(create.size.width >= label_width(window, shell.read(cx).t("skins.action.create"), FontWeight::NORMAL) + px(42.), "创建按钮须容纳完整译文、图标与内距");
            let card = window.find("skin-card-pikachu-Builtin").bounds();
            inside(card, window.find("skin-metadata-pikachu-Builtin").bounds(), "反馈后的卡片正文");
            inside(card, window.find("skin-apply-pikachu-Builtin").bounds(), "反馈后的Apply");
            assert!(window.find("skins-feedback").bounds().bottom() < card.top());
        }).unwrap();
    }
}
