//! 真实已解码内置素材验证预览框、Cover布局与独立辅助语义，不启动资源服务。

use super::view::skin_preview;
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
    image: Arc<RenderImage>,
    width: f32,
    selected: bool,
    applied: usize,
}

impl Render for PreviewProbe {
    /// 固定176px视口模拟生产预览槽，图片语义不吞并选择框或提交按钮。
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().downgrade();
        div().size_full().p(px(24.)).child(
            div().w(px(self.width)).flex().flex_col().child(
                div().id("preview-frame").test_support().relative().w_full().h(px(176.))
                    .flex_shrink_0().overflow_hidden().child(skin_preview(
                        ImageSource::Render(self.image.clone()), "preview".into(), "Pikachu preview".into(),
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

/// 宽框触发上下Cover裁剪，窄框触发左右裁剪；真实Img须始终等于176px视口。
#[gpui_kit::test]
fn decoded_preview_keeps_cover_element_inside_fixed_viewport(cx: &mut TestAppContext) {
    let (handle, probe) = open(cx, 322.);
    for scale in [1., 2.] {
        cx.simulate_window_scale_factor_change(handle, scale);
        for width in [322., 220.] {
            probe.update(cx, |probe, cx| { probe.width = width; cx.notify(); });
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                let frame = window.find("preview-frame").bounds();
                let image = window.find("preview-image").bounds();
                assert_eq!(frame.size, size(px(width), px(176.)));
                assert_eq!(image, frame, "Cover必须使用预览框而非被素材比例撑高的Img边界");
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


/// 安装0.3.12的实际SkinCard使用176px；生产壳层在四列／两列及1x／2x下都保持此视口。
#[gpui_kit::test]
fn production_skin_card_preview_matches_installed_height(cx: &mut TestAppContext) {
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1492., 900.);
    let image = Arc::new(Image::from_bytes(
        ImageFormat::Png,
        include_bytes!("../../../resources/builtin-skins/pikachu/qq2007-sky.png").to_vec(),
    ));
    let decoded = cx.update(|cx| image.to_image_data(cx.svg_renderer()).expect("解码真实Pikachu原图"));
    assert_eq!(decoded.size(0).width.0, 1920);
    assert_eq!(decoded.size(0).height.0, 1080);
    shell.update(cx, |this, cx| {
        this.page = super::Page::Skins;
        this.skins.host = Some(loki_metris_core::SkinHostKind::Codex);
        this.skins.entries = Some(vec![crate::usage_service::SkinEntry {
            skin: crate::skins::SkinDescriptor {
                id: "pikachu".into(),
                source: loki_metris_core::SkinSource::Builtin,
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
    for scale in [1., 2.] {
        cx.simulate_window_scale_factor_change(handle, scale);
        for width in [1492., 768.] {
            cx.simulate_window_resize(handle, size(px(width), px(900.)));
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                let preview = window.find("skin-preview-pikachu-Builtin").bounds();
                let image = window.find("skin-preview-pikachu-Builtin-image").bounds();
                let metadata = window.find("skin-metadata-pikachu-Builtin").bounds();
                assert_eq!(preview.size.height, px(176.),
                    "生产SkinCard的预览高度必须来自安装版合同，width={width}, scale={scale}");
                assert_eq!(image, preview, "原图的Cover边界必须使用生产预览视口");
                assert_eq!(metadata.origin.y, preview.bottom(), "元信息必须紧随真实预览底边");
                assert_eq!(preview.size.width / preview.size.height > 16. / 9., width == 1492.,
                    "真实响应式列宽应分别覆盖上下裁剪和左右裁剪");
            }).unwrap();
        }
    }
}
