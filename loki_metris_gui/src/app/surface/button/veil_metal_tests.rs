//! 真实按钮+Metal检查遮罩层叠、内圆角、缓存和卸载，不将纯数学结果充当GPU证据。

use super::*;
use gpui_kit::{
    AppContext, HeadlessAppContext, NoopTextSystem, Render, TestAppContext, WindowOptions, div,
    size, test::TestWindowExt,
};
use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

struct Probe {
    loading: bool,
    dark: bool,
    visible: bool,
    motion: Rc<RefCell<Option<gpui_kit::WeakEntity<ButtonMotion>>>>,
}
impl Render for Probe {
    /// 同一按钮ID切换状态，真实owner和Loader均使用生产实现。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let body = div().size_full().bg(gpui_kit::rgb(0x556677)).p(px(20.));
        if self.visible {
            let motion = self.motion.clone();
            let mut button =
                SurfaceButton::new("veil-probe", SurfaceButtonVariant::FilledBrand, self.dark)
                    .historical_size(HistoricalButtonSize::XSmall)
                    .w(px(120.))
                    .loading(self.loading)
                    .label("Write");
            button.motion_observer = Some(Box::new(move |owner| {
                *motion.borrow_mut() = Some(owner.downgrade())
            }));
            body.child(button)
        } else {
            body
        }
    }
}

/// 真实loader白色不得被后置遮罩染色；浅/深遮罩只能在内部改变背景。
#[test]
fn loading_veil_modulates_body_under_native_loader_on_actual_metal() {
    for dark in [false, true] {
        let mut cx = HeadlessAppContext::with_platform(
            Arc::new(NoopTextSystem),
            Arc::new(crate::assets::Assets),
            gpui_kit::platform::current_headless_renderer,
        );
        cx.update(gpui_kit::init);
        let handle = cx
            .open_window(size(px(180.), px(100.)), |_, app| {
                app.new(|_| Probe {
                    loading: true,
                    dark,
                    visible: true,
                    motion: Rc::default(),
                })
            })
            .unwrap()
            .into();
        let (bounds, loader_bounds) = cx
            .update_window(handle, |_, window, app| {
                window.render_frame(app);
                let button = window.find("veil-probe");
                let loader = gpui_kit::base::test_support::find(
                    window,
                    button.path(),
                    &"historical-button-loader".into(),
                )
                .unwrap();
                (button.bounds(), loader.bounds())
            })
            .unwrap();
        let image = cx.capture_screenshot(handle).expect("实际Metal遮罩读回");
        let scale = image.width() as f32 / 180.;
        let sample = |x: f32, y: f32| image.get_pixel((x * scale) as u32, (y * scale) as u32).0;
        let x = f32::from(bounds.origin.x);
        let y = f32::from(bounds.origin.y);
        let body = sample(x + 12., y + 15.);
        let base = colors(SurfaceButtonVariant::FilledBrand, dark, false, false).0;
        let green = (base.g * 255.).round() as u8;
        if dark {
            assert!(body[1] + 3 < green, "独立黑15%模糊块必须压暗背景");
        } else {
            assert!(body[1] > green + 5, "独立白15%模糊块必须提亮背景");
        }
        assert_eq!(
            sample(x, y),
            [0x55, 0x66, 0x77, 255],
            "padding圆角之外不能有遮罩"
        );
        let loader = sample(
            f32::from(loader_bounds.center().x),
            f32::from(loader_bounds.origin.y) + 1.25,
        );
        assert_eq!(loader, [255; 4], "Loader在遮罩上方，保持原生白色");
        // 等待只推进测试时钟；静态像素不得跟随旋转重新生成。
        cx.advance_clock(Duration::from_millis(300));
        cx.run_until_parked();
        cx.update_window(handle, |_, window, app| window.render_frame(app))
            .unwrap();
    }
}

/// 真元素使用TestAtlas检查条目生命周期；MetalAtlas未实现contains，不能用其默认false作证据。
#[gpui_kit::test]
fn loading_veil_reuses_texture_through_motion_and_releases_test_atlas(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let motion = Rc::new(RefCell::new(None));
    let (handle, probe) = cx.update(|app| {
        gpui_kit::open_window(WindowOptions::default(), app, |_, app| {
            app.new(|_| Probe {
                loading: false,
                dark: false,
                visible: true,
                motion: motion.clone(),
            })
        })
        .unwrap()
    });
    cx.update_window(handle, |_, window, app| window.render_frame(app))
        .unwrap();
    let owner = motion.borrow().as_ref().unwrap().clone();
    let image = |app: &App| owner.upgrade().expect("按钮已挂载").read(app).veil.image();
    cx.update_window(handle, |_, window, app| {
        assert!(image(app).is_none(), "静态非loading按钮零纹理");
        probe.update(app, |state, cx| {
            state.loading = true;
            cx.notify();
        });
        window.render_frame(app);
        assert!(image(app).is_none(), "100ms opacity在起点仍为0");
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_millis(50));
    cx.run_until_parked();
    let first = cx
        .update_window(handle, |_, window, app| {
            window.render_frame(app);
            let image = image(app).expect("真实过渡生成纹理");
            assert!(window.has_image_atlas_entry(&image));
            image
        })
        .unwrap();
    cx.executor().advance_clock(Duration::from_millis(150));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, app| {
        window.render_frame(app);
        assert_eq!(
            image(app).unwrap().id,
            first.id,
            "位移/透明度/旋转不重新上传"
        );
        probe.update(app, |state, cx| {
            state.loading = false;
            cx.notify();
        });
        window.render_frame(app);
        assert_eq!(image(app).unwrap().id, first.id, "反向起点连续保留同图");
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, app| {
        window.render_frame(app);
        assert!(image(app).is_none());
        assert!(
            !window.has_image_atlas_entry(&first),
            "透明终态清理实际atlas"
        );
        probe.update(app, |state, cx| {
            state.loading = true;
            cx.notify();
        });
        window.render_frame(app);
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    let last = cx
        .update_window(handle, |_, window, app| {
            window.render_frame(app);
            let image = image(app).unwrap();
            assert!(window.has_image_atlas_entry(&image));
            probe.update(app, |state, cx| {
                state.visible = false;
                cx.notify();
            });
            window.render_frame(app);
            image
        })
        .unwrap();
    cx.update_window(handle, |_, window, _| {
        assert!(
            !window.has_image_atlas_entry(&last),
            "卸载释放，无需后续可见帧"
        )
    })
    .unwrap();
    assert!(owner.upgrade().is_none(), "测试只保存弱owner");
}
