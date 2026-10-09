//! 原生窗口验证tbody行绘制、列尺寸、交互隔离和稳定行owner，不启动数据服务。

use super::{BrandTableRow, table_row::RowState};
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Context, Element, Entity, IntoElement, Render,
    Rgba, Role, ScrollDelta, TestAppContext, Window, WindowBounds, WindowOptions, accesskit, div, point,
    prelude::*, px, size,
    base::{Checkbox, CheckboxState, TestSupportExt as _},
    component::{ChildElement, Sizable, button::{Button, ButtonVariants}, table::{Table, TableBody, TableCell, TableHeader, TableHead, TableRow}},
    test::TestWindowExt,
};
use std::{cell::RefCell, rc::Rc, time::Duration};

/// 测试只保存弱owner观察句柄，卸载不能被夹具强引用掩盖。
type Owners = Rc<RefCell<[Option<gpui_kit::WeakEntity<RowState>>; 2]>>;

/// 同一实体可反转顺序、卸载邻居和切换主题，子按钮及选择框独立拥有动作。
struct Probe {
    dark: bool,
    shown: [bool; 2],
    reversed: bool,
    selected: [bool; 2],
    clicks: [usize; 2],
    owners: Owners,
}

impl Render for Probe {
    /// 横向内容宽于视口；行稳定ID与数据身份绑定，排序只改变辅助序号。
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let order = if self.reversed { [1, 0] } else { [0, 1] };
        let body = TableBody::new().children(order.into_iter().filter_map(|index| {
            if !self.shown[index] { return None; }
            let owners = self.owners.clone();
            let weak = cx.entity().downgrade();
            Some(BrandTableRow::new(("brand-row", index), self.dark)
                .observe_owner(move |owner| owners.borrow_mut()[index] = Some(owner.downgrade()))
                .child(TableCell::new().w(px(200.)).min_w(px(200.)).flex_none().h(px(64.)).p_2().child(format!("Row {index}")))
                .child(TableCell::new().w(px(200.)).min_w(px(200.)).flex_none().h(px(64.)).p_2()
                    .child(Button::new(("row-action", index)).ghost().label("Child action")
                        .on_click(cx.listener(move |this, _, _, cx| { this.clicks[index] += 1; cx.notify(); }))))
                .child(TableCell::new().w(px(200.)).min_w(px(200.)).flex_none().h(px(64.)).p_2()
                    .child(Checkbox::new(("row-select", index)).size(px(20.)).checked(self.selected[index]).accessibility_label(format!("Select row {index}"))
                        .on_change(move |checked, _, _, cx| { let _ = weak.update(cx, |this, cx| { this.selected[index] = checked == CheckboxState::Checked; cx.notify(); }); })))
            )
        }));
        div().id("row-probe-page").test_support().relative().size_full().p(px(24.)).child(
            div().id("row-scroll").test_support().w(px(540.)).h(px(240.)).overflow_x_scroll().child(
                Table::new().w(px(600.)).child(TableHeader::new().child(TableRow::new().children(
                    ["Name", "Action", "Selection"].map(|name| TableHead::new().w(px(200.)).min_w(px(200.)).flex_none().child(name))
                ))).child(body)
            )
        ).child(div().id("row-outside").test_support().absolute().right(px(4.)).bottom(px(4.)).size(px(20.)))
    }
}

/// 真实测试窗口初始化组件主题，不读取真实用户目录、索引或任何宿主服务。
fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Probe>, Owners) {
    cx.update(gpui_kit::init);
    let owners: Owners = Rc::new(RefCell::new([None, None]));
    let (handle, probe) = cx.update(|cx| gpui_kit::open_window(WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), size(px(720.), px(360.))))),
        ..Default::default()
    }, cx, |_, cx| cx.new(|_| Probe { dark: false, shown: [true; 2], reversed: false, selected: [false; 2], clicks: [0; 2], owners: owners.clone() })).unwrap());
    (handle, probe, owners)
}

/// 只读取真实行边界上的背景quad；子按钮底色或table整体底色不能冒充行悬停。
fn row_color(index: usize, window: &Window) -> Rgba {
    let bounds = window.find(("brand-row", index)).bounds().scale(window.scale_factor());
    window.painted_quads().into_iter().filter(|quad| quad.bounds == bounds)
        .filter_map(|quad| quad.background.as_solid().map(Rgba::from))
        .find(|color| color.a > 0. && color.a < 0.1)
        .unwrap_or(gpui_kit::transparent_black().into())
}

/// 两行相同几何仍有独立hover，160ms端点必须是6%品牌色且适配1x/2x。
#[gpui_kit::test]
fn actual_row_hover_reaches_brand_six_percent_and_isolates_siblings(cx: &mut TestAppContext) {
    let (handle, _, owners) = open(cx);
    for scale in [1., 2.] {
        cx.simulate_window_scale_factor_change(handle, scale);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find(("brand-row", 0usize)).role(), Some(Role::Row));
            let row = window.find(("brand-row", 0usize)).bounds();
            assert_eq!(row.size.width, px(600.));
            assert_eq!(row.size.height, px(64.));
            let first = window.within(("brand-row", 0usize)).find(("table-cell", 0usize));
            assert_eq!(first.role(), Some(Role::Cell));
            assert_eq!(first.bounds().size.width, px(200.));
            assert_ne!(owners.borrow()[0].as_ref().unwrap().upgrade().unwrap().entity_id(), owners.borrow()[1].as_ref().unwrap().upgrade().unwrap().entity_id());
            window.hover(("brand-row", 0usize), cx);
            window.render_frame(cx);
        }).unwrap();
        cx.executor().advance_clock(Duration::from_millis(160));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let color = row_color(0, window);
            assert!((color.a - 0.06).abs() < 1e-6, "真实行悬停必须达到6%accent，当前{color:?}");
            assert!((color.r - 165. / 255.).abs() < 1e-6);
            assert_eq!(row_color(1, window).a, 0.);
            window.hover("row-outside", cx);
            window.render_frame(cx);
        }).unwrap();
        cx.executor().advance_clock(Duration::from_millis(160));
        cx.update_window(handle, |_, window, cx| { window.render_frame(cx); assert_eq!(row_color(0, window).a, 0.); }).unwrap();
    }
}

/// 进入/离开反向及主题重定向首帧保持连续；行排序后邻居不继承hover状态。
#[gpui_kit::test]
fn actual_row_motion_reverses_and_theme_changes_without_jumps(cx: &mut TestAppContext) {
    let (handle, probe, _) = open(cx);
    cx.update_window(handle, |_, window, cx| { window.render_frame(cx); window.hover(("row-action", 0usize), cx); window.render_frame(cx); }).unwrap();
    cx.executor().advance_clock(Duration::from_millis(60));
    let middle = cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let middle = row_color(0, window);
        assert!(middle.a > 0. && middle.a < 0.06, "60ms必须仍在真实160ms过渡中");
        window.hover("row-outside", cx);
        window.render_frame(cx);
        assert_eq!(row_color(0, window), middle);
        middle
    }).unwrap();
    cx.executor().advance_clock(Duration::from_millis(20));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(row_color(0, window).a < middle.a);
        window.hover(("row-action", 0usize), cx);
        window.render_frame(cx);
    }).unwrap();
    cx.executor().advance_clock(Duration::from_millis(160));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let light = row_color(0, window);
        probe.update(cx, |this, cx| { this.dark = true; cx.notify(); });
        window.render_frame(cx);
        assert_eq!(row_color(0, window), light);
    }).unwrap();
    cx.executor().advance_clock(Duration::from_millis(160));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let dark = row_color(0, window);
        assert!((dark.r - 242. / 255.).abs() < 1e-6);
        assert!((dark.g - 160. / 255.).abs() < 1e-6);
        assert!((dark.a - 0.06).abs() < 1e-6);
        probe.update(cx, |this, cx| { this.reversed = true; cx.notify(); });
        window.render_frame(cx);
        assert_eq!(row_color(1, window).a, 0.);
    }).unwrap();
}

/// 行与单元格留白不代理子动作，按钮/选择框hover及点击各自有效，卸载释放真实owner。
#[gpui_kit::test]
fn actual_row_preserves_child_events_and_releases_removed_owner(cx: &mut TestAppContext) {
    let (handle, probe, owners) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click_at(("brand-row", 0usize), point(px(4.), px(4.)), cx);
        assert_eq!(probe.read(cx).clicks, [0, 0]);
        assert_eq!(probe.read(cx).selected, [false, false]);
        window.click(("row-action", 0usize), cx);
        assert_eq!(probe.read(cx).clicks, [1, 0]);
        assert_eq!(probe.read(cx).selected, [false, false]);
        window.click(("row-select", 0usize), cx);
        assert_eq!(probe.read(cx).clicks, [1, 0]);
        assert_eq!(probe.read(cx).selected, [true, false]);
        window.hover(("row-action", 0usize), cx);
        window.render_frame(cx);
    }).unwrap();
    cx.executor().advance_clock(Duration::from_millis(160));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!((row_color(0, window).a - 0.06).abs() < 1e-6, "子控件hover也必须作用于真实父行");
        let neighbor = owners.borrow()[1].as_ref().unwrap().upgrade().unwrap().entity_id();
        probe.update(cx, |this, cx| { this.shown[0] = false; cx.notify(); });
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(owners.borrow()[0].as_ref().unwrap().upgrade().is_none(), "组件卸载必须释放行owner");
        assert_eq!(owners.borrow()[1].as_ref().unwrap().upgrade().unwrap().entity_id(), neighbor);
        window.click(("row-action", 1usize), cx);
        assert_eq!(probe.read(cx).clicks, [1, 1]);
    }).unwrap();
}

/// 显式位置只改变辅助索引，大小传递经原生Cell并保留真实三列尺寸。
#[test]
fn row_index_is_one_based_and_independent_from_stable_identity() {
    let row = BrandTableRow::new("stable-root-id", false).with_ix(4).with_size(gpui_kit::component::Size::Small);
    assert_eq!(Element::id(&row), Some("stable-root-id".into()));
    assert_eq!(row.a11y_role(), Some(Role::Row));
    let mut node = accesskit::Node::new(Role::Row);
    row.write_a11y_info(&mut node);
    assert_eq!(node.row_index(), Some(5));
}

/// 同一张Table让品牌行与既有行消费完全相同的Size和Cell，避免复制padding公式。
struct SizeProbe {
    size: gpui_kit::component::Size,
}

impl Render for SizeProbe {
    /// 原列宽与横向滚动约束同时作用于两种行，只有背景renderer不同。
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let cell = |id| TableCell::new().w(px(300.)).min_w(px(300.)).flex_none()
            .child(div().id(id).test_support().w(px(20.)).h(px(16.)));
        div().size_full().p(px(24.)).child(div().id("size-scroll").test_support().w(px(400.)).h(px(240.)).overflow_x_scroll()
            .child(Table::new().w(px(600.)).flex_none().with_size(self.size).child(TableBody::new()
                .child(BrandTableRow::new("size-brand-row", false).child(cell("size-brand-content")).child(cell("size-brand-second")))
                .child(TableRow::new().child(cell("size-legacy-content")).child(cell("size-legacy-second"))))))
    }
}

/// Small/Large真实Cell边界与既有行一致；水平滚动后列宽及Row语义不变。
#[gpui_kit::test]
fn actual_row_forwards_cell_sizes_and_keeps_horizontal_scroll(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (handle, probe) = cx.update(|cx| gpui_kit::open_window(WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), size(px(700.), px(350.))))),
        ..Default::default()
    }, cx, |_, cx| cx.new(|_| SizeProbe { size: gpui_kit::component::Size::Small })).unwrap());
    for row_size in [gpui_kit::component::Size::Small, gpui_kit::component::Size::Large] {
        probe.update(cx, |this, cx| { this.size = row_size; cx.notify(); });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let brand_cell = window.within("size-brand-row").find(("table-cell", 0usize)).bounds();
            let legacy_cell = window.within(("table-row", 1usize)).find(("table-cell", 0usize)).bounds();
            assert_eq!(brand_cell.size, legacy_cell.size, "Table Size必须传递到实际Cell");
            assert_eq!(brand_cell.size.width, px(300.));
            let brand_content = window.find("size-brand-content").bounds();
            let legacy_content = window.find("size-legacy-content").bounds();
            assert_eq!(brand_content.origin - brand_cell.origin, legacy_content.origin - legacy_cell.origin);
            let before = window.find("size-brand-row").bounds();
            window.scroll("size-scroll", ScrollDelta::Pixels(point(px(-50.), px(0.))), cx);
            window.render_frame(cx);
            let after = window.find("size-brand-row").bounds();
            assert!(after.origin.x < before.origin.x, "原行必须随原生横向滚动移动");
            assert_eq!(after.size.width, px(600.));
            assert_eq!(window.within("size-brand-row").find(("table-cell", 1usize)).bounds().size.width, px(300.));
        }).unwrap();
    }
}
