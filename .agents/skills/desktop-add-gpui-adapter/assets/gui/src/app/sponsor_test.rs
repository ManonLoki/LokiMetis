//! 赞助页回归检查实际渲染几何，不以源码片段或 mock 布局替代窗口结果。

use super::*;
use crate::{app::{Page, tests::open_shell}, preferences::{LanguageChoice, ThemeChoice}};
use gpui_kit::{TestAppContext, size, test::TestWindowExt};

/// 宽窄窗口、中英文和明暗主题都不产生额外页眉、水平位移、卡片挤压或二维码失真。
#[gpui_kit::test]
fn sponsor_template_fills_content_and_keeps_cards_and_qr_inside_bounds(cx: &mut TestAppContext) {
    let (handle, shell, _finish) = open_shell(cx, 1440., 900.);
    for width in [960., 1100., 1440.] {
        cx.simulate_window_resize(handle, size(px(width), px(900.)));
        for theme in [ThemeChoice::Light, ThemeChoice::Dark] {
            for language in [LanguageChoice::Chinese, LanguageChoice::English] {
                cx.update_window(handle, |_, window, cx| {
                    shell.update(cx, |this, cx| {
                        this.page = Page::Sponsor;
                        this.select_language(language, cx);
                        this.select_theme(theme, window, cx);
                    });
                    window.render_frame(cx);
                    let main = window.find("main-content").bounds();
                    let viewport = window.find("page-content").bounds();
                    let page = window.find("sponsor-page").bounds();
                    assert!(window.try_find("page-heading").is_none(), "赞助模板不能再加通用标题与分割线");
                    assert_eq!(viewport, main, "赞助滚动槽应完整使用内容区");
                    assert_eq!(page.origin, viewport.origin, "赞助页不能叠加外层留白");
                    assert_eq!(page.size.width, viewport.size.width);
                    assert!(page.size.height >= viewport.size.height);
                    let intro = window.find("sponsor-intro").bounds();
                    let title = window.find("sponsor-title").bounds();
                    assert!((title.center().x - intro.center().x).abs() <= px(1.));
                    let cards = [0usize, 1, 2].map(|index| window.find(("sponsor-tier", index)).bounds());
                    for card in &cards {
                        assert!(card.origin.x >= page.origin.x && card.right() <= page.right());
                        assert!(card.size.width > px(0.) && card.size.height > px(0.));
                    }
                    if f32::from(viewport.size.width) >= 1000. {
                        for card in &cards[1..] {
                            assert_eq!(card.origin.y, cards[0].origin.y);
                            assert!((card.size.width - cards[0].size.width).abs() <= px(1.));
                            assert!((card.size.height - cards[0].size.height).abs() <= px(1.));
                        }
                    } else {
                        assert!(cards[0].bottom() < cards[1].origin.y);
                        assert!(cards[1].bottom() < cards[2].origin.y);
                    }
                    let payment = window.find("sponsor-payment").bounds();
                    assert!(cards.iter().all(|card| card.bottom() < payment.origin.y));
                    for index in [0usize, 1] {
                        let qr = window.find(("sponsor-qr-image", index)).bounds();
                        assert_eq!(qr.size, size(px(120.), px(136.)));
                        assert!(qr.origin.x >= payment.origin.x && qr.right() <= payment.right());
                    }
                }).expect("更新赞助模板布局");
            }
        }
    }
}
