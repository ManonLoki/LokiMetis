//! 真实已解码内置素材验证预览框、Cover布局与独立辅助语义，不启动资源服务。

use super::{SkinSuccess, view::skin_preview};
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Context, Entity, Image, ImageFormat, ImageSource,
    IntoElement, Render, RenderImage, Role, TestAppContext, Window, WindowBounds, WindowOptions,
    base::{Checkbox, CheckboxState, TestSupportExt as _},
    component::button::{Button, ButtonVariants}, div, point, prelude::*, px, size,
    test::TestWindowExt,
};
use std::sync::Arc;

/// 窄宽预览共用生产图片元素；选择框与下方按钮各自拥有动作。
struct PreviewProbe {
    /// 预览图。
    image: Arc<RenderImage>,
    /// 预览容器宽度。
    width: f32,
    /// 选择框勾选状态。
    selected: bool,
    /// 应用按钮点击次数。
    applied: usize,
}

impl Render for PreviewProbe {
    /// 固定160px可见槽模拟Card.Section负margin后的生产预览，图片语义不吞并选择框或提交按钮。
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().downgrade();
        let frame = super::preview::section_frame(
            div().id("preview-frame").test_support().relative().w_full().h(px(160.))
                .flex_shrink_0().overflow_hidden().rounded_t(px(19.)),
        );
        div().size_full().p(px(24.)).child(
            div().w(px(self.width)).flex().flex_col().child(
                frame.child(skin_preview(
                        ImageSource::Render(self.image.clone()), "preview".into(), "Pikachu preview".into(),
                        Default::default(),
                    )).child(
                        Checkbox::new("preview-select").checked(self.selected).accessibility_label("Select Pikachu")
                            .absolute().right(px(12.)).top(px(12.)).size(px(20.))
                            .on_change(move |checked, _, _, cx| {
                                let _ = entity.update(cx, |this, cx| {
                                    this.selected = checked == CheckboxState::Checked;
                                    cx.notify();
                                });
                            }),
                    ),
            ).child(
                Button::new("preview-apply").ghost().label("Apply Pikachu").w_full().h(px(36.))
                    .on_click(cx.listener(|this, _, _, cx| { this.applied += 1; cx.notify(); })),
            ),
        )
    }
}

/// 源PNG通过GPUI实际解码；完成解码后再布局，避免首帧尚未加载造成假通过。
fn open(cx: &mut TestAppContext, width: f32) -> (AnyWindowHandle, Entity<PreviewProbe>) {
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        let image = Image::from_bytes(ImageFormat::Png,
            include_bytes!("../../../resources/builtin-skins/pikachu/qq2007-sky.png").to_vec())
            .to_image_data(cx.svg_renderer()).expect("解码原始Pikachu素材");
        assert_eq!(image.size(0).width.0, 1920);
        assert_eq!(image.size(0).height.0, 1080);
        gpui_kit::open_window(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), size(px(600.), px(300.))))),
            ..Default::default()
        }, cx, |_, cx| {
            cx.new(|_| PreviewProbe { image, width, selected: false, applied: 0 })
        }).expect("创建真实预览测试窗口")
    })
}

/// 原始176px图像视口向外扩32px并上移16px；外层160px负责实际裁切。
#[gpui_kit::test]
fn decoded_preview_preserves_section_margin_and_outer_clip(cx: &mut TestAppContext) {
    let (handle, probe) = open(cx, 322.);
    for scale in [1., 2.] {
        cx.simulate_window_scale_factor_change(handle, scale);
        for width in [322., 220.] {
            probe.update(cx, |probe, cx| { probe.width = width; cx.notify(); });
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                let frame = window.find("preview-frame").bounds();
                let image = window.find("preview-image").bounds();
                assert_eq!(frame.size, size(px(width), px(160.)));
                assert_eq!(image, Bounds::new(frame.origin - point(px(16.), px(16.)), size(px(width + 32.), px(176.))));
                assert_outer_clip(window, frame);
            }).unwrap();
        }
    }
}

/// 图片原生角色与名称可读，点击图片留白不选择资源，选择框与按钮互不代理。
#[gpui_kit::test]
fn preview_exposes_image_name_and_keeps_sibling_controls_independent(cx: &mut TestAppContext) {
    let (handle, probe) = open(cx, 322.);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let image = window.find("preview");
        assert_eq!(image.role(), Some(Role::Image));
        assert_eq!(image.label(), Some("Pikachu preview"));
        assert_eq!(image.focused(), None);
        let selection = window.find("preview-select");
        assert!(!selection.path().starts_with(image.path()), "选择框必须是图片语义的Sibling");
        window.click_at("preview", point(px(8.), px(8.)), cx);
        assert!(!probe.read(cx).selected);
        assert_eq!(probe.read(cx).applied, 0);
        window.click("preview-select", cx);
        assert!(probe.read(cx).selected);
        assert_eq!(probe.read(cx).applied, 0);
        window.click("preview-apply", cx);
        assert!(probe.read(cx).selected);
        assert_eq!(probe.read(cx).applied, 1);
    }).unwrap();
}


/// 安装0.3.12的Card.Section保留md16负margin；h176只留下160px，不能直接当可见高度。
#[gpui_kit::test]
fn production_skin_card_preview_matches_installed_section_crop(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1492., 900.);
    let image = Arc::new(Image::from_bytes(
        ImageFormat::Png,
        include_bytes!("../../../resources/builtin-skins/pikachu/qq2007-sky.png").to_vec(),
    ));
    let decoded = cx.update(|cx| image.to_image_data(cx.svg_renderer()).expect("解码真实Pikachu原图"));
    assert_eq!(decoded.size(0).width.0, 1920);
    assert_eq!(decoded.size(0).height.0, 1080);
    populate_catalog(&shell, image, loki_metris_core::SkinSource::Builtin, cx);
    // 确认真实Arc<Image>走完异步Asset解码后再查绘制图元。
    for _ in 0..3 {
        cx.update_window(handle, |_, window, cx| window.render_frame(cx)).unwrap();
        cx.run_until_parked();
    }
    for scale in [1., 2.] {
        cx.simulate_window_scale_factor_change(handle, scale);
        for width in [1492., 768.] {
            cx.simulate_window_resize(handle, size(px(width), px(900.)));
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                let preview = window.find("skin-preview-pikachu-Builtin").bounds();
                let image = window.find("skin-preview-pikachu-Builtin-image").bounds();
                let metadata = window.find("skin-metadata-pikachu-Builtin").bounds();
                assert_eq!(preview.size.height, px(160.),
                    "Card.Section负margin必须在生产壳层真实裁去顶部16px，width={width}, scale={scale}");
                assert_eq!(image, Bounds::new(preview.origin - point(px(16.), px(16.)),
                    size(preview.size.width + px(32.), px(176.))), "原图必须仍在扩宽且上移的176px视口内Cover");
                assert_outer_clip(window, preview);
                assert_eq!(metadata.origin.y, preview.bottom(), "元信息必须紧随真实预览底边");
                let card = window.find("skin-card-pikachu-Builtin").bounds();
                assert_eq!(preview.origin, card.origin + point(px(1.), px(1.)),
                    "overflow-hidden沿Card的padding edge裁切，不能移到外border-box");
                assert_eq!(card.size.width - px(2.), preview.size.width,
                    "外层可见槽沿用真实卡片内宽，不改变响应式列宽");
            }).unwrap();
        }
    }
}

/// 实际macOS图片自身无圆角且由父R19链裁切；其它平台继续检查独立原viewport。
fn assert_outer_clip(window: &Window, viewport: Bounds<gpui_kit::Pixels>) {
    let scale = window.scale_factor();
    let outer = viewport.scale(scale);
    let sprites = window.painted_polychrome_sprites();
    #[cfg(target_os = "macos")]
    {
        let sprite = sprites.iter().find(|sprite| sprite.content_mask.bounds == outer)
            .expect("真实图片必须仍使用原160px矩形viewport");
        assert_ne!(sprite.parent_clip.head, 0, "图片必须继承实际父圆角链，不能只用矩形mask");
        assert_eq!(sprite.corner_radii, gpui_kit::Corners::all(px(0.).scale(scale)),
            "图片自身不得再乘同一R19，否则AA边缘会平方");
    }
    #[cfg(not(target_os = "macos"))]
    {
        let sprite = sprites.iter().find(|sprite| sprite.rounded_clip_bounds == outer)
            .expect("独立图片圆角必须保存160px原viewport");
        assert_eq!(sprite.corner_radii.top_left, px(19.).scale(scale));
        assert_eq!(sprite.corner_radii.top_right, px(19.).scale(scale));
        assert_eq!(sprite.corner_radii.bottom_left, px(0.).scale(scale));
        assert_eq!(sprite.corner_radii.bottom_right, px(0.).scale(scale));
        assert_eq!(sprite.content_mask.bounds, outer);
    }
}

/// 单一成功目录fixture只注入展示输入，真实选择和画布仍由生产Shell建立。
fn populate_catalog(shell: &Entity<crate::app::Shell>, image: Arc<Image>,
    source: loki_metris_core::SkinSource, cx: &mut TestAppContext) {
    shell.update(cx, |this, cx| {
        this.page = super::Page::Skins;
        this.skins.host = Some(loki_metris_core::SkinHostKind::Codex);
        this.skins.entries = Some(vec![crate::usage_service::SkinEntry {
            skin: crate::skins::SkinDescriptor {
                id: "pikachu".into(),
                source,
                package_type: loki_metris_core::SkinPackageType::LegacySkin,
                name: "Pikachu".into(),
                author: "ManonLoki".into(),
                version: "0.1.0".into(),
                preview_data_url: String::new(),
                supported_color_modes: vec![loki_metris_core::ColorMode::Light],
            },
            preview: Some(image),
        }]);
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
        cx.notify();
    });

}

/// 用户选择框同属负margin的section；可见部分直接调用原生Checkbox，不由整卡代点。
#[gpui_kit::test]
fn user_skin_checkbox_keeps_section_offset_and_own_actions(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1492., 900.);
    let image = Arc::new(Image::from_bytes(ImageFormat::Png,
        include_bytes!("../../../resources/builtin-skins/pikachu/qq2007-sky.png").to_vec()));
    populate_catalog(&shell, image, loki_metris_core::SkinSource::User, cx);
    for scale in [1., 2.] {
        cx.simulate_window_scale_factor_change(handle, scale);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let outer = window.find("skin-preview-pikachu-User").bounds();
            let selection = window.find("skin-select-pikachu");
            let bounds = selection.bounds();
            assert_eq!(bounds.origin.y, outer.origin.y - px(4.));
            assert_eq!(bounds.right(), outer.right() + px(4.));
            assert_eq!(bounds.size, size(px(20.), px(20.)));
            assert_eq!(selection.label(), Some("Select Pikachu"));
            #[cfg(target_os = "macos")]
            {
                // top/right -4的圆角外点仍在Checkbox矩形内，真实父Hitbox必须拒绝。
                window.click_at("skin-select-pikachu", point(px(15.), px(5.)), cx);
                assert!(shell.read(cx).skins.selected.is_empty(), "父R19外不能选择用户皮肤");
            }
            window.click_at("skin-select-pikachu", point(px(6.), px(12.)), cx);
            assert_eq!(shell.read(cx).skins.selected.len(), 1);
            window.click_at("skin-preview-pikachu-User", point(px(8.), px(80.)), cx);
            assert_eq!(shell.read(cx).skins.selected.len(), 1, "图片不能代理选择动作");
            assert!(!shell.read(cx).skins.busy, "选择不能代理应用皮肤动作");
            window.click_at("skin-select-pikachu", point(px(6.), px(12.)), cx);
            assert!(shell.read(cx).skins.selected.is_empty());
        }).unwrap();
    }
}

/// 真实主窗Tab覆盖用户卡片并完整回绕，不能漏掉控件或进入未选宿主。
#[gpui_kit::test]
fn user_skin_card_controls_follow_toolbar_tab_order(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    let image = Arc::new(Image::from_bytes(ImageFormat::Png,
        include_bytes!("../../../resources/builtin-skins/pikachu/qq2007-sky.png").to_vec()));
    populate_catalog(&shell, image, loki_metris_core::SkinSource::User, cx);
    assert!(shell.read_with(cx, |this, _| this.skin_writable()));
    for (width, height, feedback) in [(1440., 900., false), (1440., 900., true), (800., 300., true)] {
        cx.simulate_window_resize(handle, size(px(width), px(height)));
        shell.update(cx, |this, cx| {
            this.skins.feedback = feedback.then(|| Ok(SkinSuccess::Created { name: "Fixture".into() }));
            cx.notify();
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            for _ in 0..16 {
                if window.find("skin-refresh").focused() == Some(true) { break; }
                window.press("tab", cx);
                window.render_frame(cx);
            }
            assert_eq!(window.find("skin-refresh").focused(), Some(true));
            let mut controls = Vec::new();
            if feedback { controls.push("skins-dismiss-feedback"); }
            controls.extend(["skin-select-pikachu", "skin-apply-pikachu-User", "skin-actions-pikachu"]);
            for id in &controls {
                window.press("tab", cx);
                window.render_frame(cx);
                assert_eq!(window.find(*id).focused(), Some(true), "Tab必须进入{id}，{width}×{height}");
            }
            window.press("tab", cx);
            window.render_frame(cx);
            assert_eq!(window.find("nav-dashboard").focused(), Some(true),
                "末项Tab必须回绕到导航首项，跳过未选宿主，{width}×{height}");
            window.press("shift-tab", cx);
            window.render_frame(cx);
            assert_eq!(window.find("skin-actions-pikachu").focused(), Some(true),
                "首项Shift+Tab必须回绕到资源菜单，跳过未选宿主，{width}×{height}");
            for id in controls.iter().rev().skip(1).copied().chain(["skin-refresh"]) {
                window.press("shift-tab", cx);
                window.render_frame(cx);
                assert_eq!(window.find(id).focused(), Some(true), "Shift+Tab必须进入{id}，{width}×{height}");
            }
        }).unwrap();
    }
}

/// 周期目录查询不能夺走刷新按钮焦点；无预览纯主题仍可直接Tab选择并继续操作。
#[gpui_kit::test]
fn background_skin_refresh_preserves_keyboard_focus(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    let image = Arc::new(Image::from_bytes(ImageFormat::Png,
        include_bytes!("../../../resources/builtin-skins/pikachu/qq2007-sky.png").to_vec()));
    populate_catalog(&shell, image, loki_metris_core::SkinSource::User, cx);
    shell.update(cx, |this, cx| {
        let entry = &mut this.skins.entries.as_mut().unwrap()[0];
        entry.skin.package_type = loki_metris_core::SkinPackageType::Theme;
        entry.preview = None;
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        for _ in 0..16 {
            if window.find("skin-refresh").focused() == Some(true) { break; }
            window.press("tab", cx);
            window.render_frame(cx);
        }
        assert_eq!(window.find("skin-refresh").focused(), Some(true));
    }).unwrap();
    // 实际周期查询唯一的瞬时展示状态；不触发文件、宿主或定时器副作用。
    shell.update(cx, |this, cx| { this.skins.loading = true; cx.notify(); });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("skin-refresh").focused(), Some(true),
            "后台刷新不能清除键盘焦点并把下一次Tab送回侧栏");
        window.press("tab", cx);
        window.render_frame(cx);
        assert_eq!(window.find("skin-select-pikachu").focused(), Some(true));
        window.press("space", cx);
        window.render_frame(cx);
        assert_eq!(shell.read(cx).skins.selected.len(), 1);
    }).unwrap();
    shell.update(cx, |this, cx| { this.skins.loading = false; cx.notify(); });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("skin-select-pikachu").focused(), Some(true));
        for id in ["skin-apply-pikachu-User", "skin-actions-pikachu"] {
            window.press("tab", cx);
            window.render_frame(cx);
            assert_eq!(window.find(id).focused(), Some(true));
        }
    }).unwrap();
}
