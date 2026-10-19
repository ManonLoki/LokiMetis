//! 真实 draw 边界捕获一次测试执行器时钟，逐元素工作不能改变本帧动画起点。

use gpui_kit::{AppContext, Context, IntoElement, Render, TestAppContext, Window, WindowOptions, canvas, div, prelude::*, px, test::TestWindowExt};
use std::{cell::RefCell, rc::Rc, time::{Duration, Instant}};

/// 注入推进时钟的闭包只修改 TestClock，不运行 scheduler 或重入 App。
struct Probe {
    /// 每帧采样到的时刻。
    samples: Rc<RefCell<Vec<Instant>>>,
    /// 推进测试时钟的闭包。
    advance: Rc<dyn Fn()>,
}
impl Render for Probe {
    /// render 和两个 sibling paint 之间推进时钟，检查都使用真实 draw 的同一时刻。
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.samples.borrow_mut().push(window.frame_animation_time());
        (self.advance)();
        div().children((0..2).map(|_| {
            let samples = self.samples.clone();
            let advance = self.advance.clone();
            canvas(|_, _, _| (), move |_, _, window, _| {
                samples.borrow_mut().push(window.frame_animation_time());
                advance();
            }).size(px(10.))
        }))
    }
}

/// 一帧内所有阶段完全一致，下一 draw 才采用已推进的执行器时钟而非墙钟。
#[gpui_kit::test]
fn frame_animation_clock_is_uniform_and_refreshes_only_at_draw(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let samples = Rc::new(RefCell::new(Vec::new()));
    let clock = cx.dispatcher.scheduler().clock();
    let advance: Rc<dyn Fn()> = Rc::new(move || clock.advance(Duration::from_millis(17)));
    let (handle, _) = cx.update(|cx| gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
        cx.new(|_| Probe { samples: samples.clone(), advance })
    }).unwrap());
    for _ in 0..2 {
        samples.borrow_mut().clear();
        cx.update_window(handle, |_, window, cx| {
            let expected = cx.background_executor().now();
            window.refresh();
            window.render_frame(cx);
            assert_eq!(*samples.borrow(), vec![expected; 3]);
            assert!(cx.background_executor().now() > expected);
        }).unwrap();
    }
}
