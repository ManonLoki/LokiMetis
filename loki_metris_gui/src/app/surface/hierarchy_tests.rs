//! 原生层级卡片继续由子按钮拥有动作，静态次卡的装饰悬停不激活聚光材质。

use super::*;
use gpui_kit::{AppContext, Context, Render, Role, TestAppContext, WindowOptions,
    base::{Button, TestSupportExt as _}, test::TestWindowExt};
use std::{cell::RefCell, rc::Rc};

/// 保留静态次卡和辅助卡的弱owner，只观察资源与原生子输入。
struct Probe {
    /// 子按钮的真实激活次数。
    clicks: usize,
    /// 次卡与辅助卡的材质owner。
    owners: Rc<RefCell<Vec<gpui_kit::WeakEntity<MaterialState>>>>,
}

impl Render for Probe {
    /// 两档卡片使用相同尺寸，几何和输入不能因装饰等级改变。
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let owners = self.owners.clone();
        owners.borrow_mut().clear();
        div().size_full().p_4().flex().gap_4().children([
            ("secondary-static", CardPriority::Secondary), ("quiet-static", CardPriority::Quiet),
        ].into_iter().map(|(id, priority)| {
            let owners = owners.clone();
            StaticCard::new(id, true, StaticMaterial::Panel).priority(priority)
                .observe_material(move |owner| owners.borrow_mut().push(owner.downgrade()))
                .test_support().w(px(220.)).h(px(180.)).p_4().role(Role::Group).aria_label(id)
                .child(Button::new((id, 0usize)).child("Action")
                    .on_click(cx.listener(|this, _, _, cx| { this.clicks += 1; cx.notify(); })))
        }))
    }
}

/// 装饰不代理按钮点击，辅助卡零投影资源，静态次卡悬停不会创建聚光纹理。
#[gpui_kit::test]
fn decorated_static_hierarchy_keeps_native_input_and_material_ownership(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let owners = Rc::new(RefCell::new(Vec::new()));
    let (handle, probe) = cx.update(|app| gpui_kit::open_window(WindowOptions::default(), app,
        |_, app| app.new(|_| Probe { clicks: 0, owners: owners.clone() })).unwrap());
    cx.update_window(handle, |_, window, app| {
        window.render_frame(app);
        let secondary = owners.borrow()[0].upgrade().unwrap();
        let quiet = owners.borrow()[1].upgrade().unwrap();
        assert_eq!(secondary.read(app).audit().1.len(), 1, "次卡仅保留静态阴影");
        assert!(quiet.read(app).audit().1.is_empty(), "辅助卡不分配聚光或投影纹理");
        window.hover("secondary-static", app);
        window.render_frame(app);
        let (hovered, images) = secondary.read(app).audit();
        assert!(!hovered, "装饰悬停不能改变静态材质合同");
        assert_eq!(images.len(), 1, "装饰使用路径，不生成聚光纹理");
        for id in ["secondary-static", "quiet-static"] {
            let card = window.find(id);
            assert_eq!(card.role(), Some(Role::Group));
            assert_eq!(card.bounds().size.width, px(220.));
            let before = probe.read(app).clicks;
            window.click_at(id, gpui_kit::point(px(4.), px(4.)), app);
            assert_eq!(probe.read(app).clicks, before);
            window.click((id, 0usize), app);
            assert_eq!(probe.read(app).clicks, before + 1);
        }
    }).unwrap();
}
