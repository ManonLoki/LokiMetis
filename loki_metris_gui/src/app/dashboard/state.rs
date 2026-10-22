//! 查询加载与四态索引提示共享展示，扫描由应用服务的自动生命周期或显式来源动作触发。

use super::{Shell,Route};
use gpui_kit::{AnyElement,Context,FontWeight,div,prelude::*,px,rgb,
    component::{ActiveTheme,button::{Button,ButtonVariants}}};
use loki_metris_core::LocalIndexState;
use gpui_kit::base::TestSupportExt as _;

impl Shell {
    /// 首次查询的骨架保持参考的 44/112/280 高度与可读状态。
    pub(super) fn usage_loading(&self,cx:&Context<Self>)->AnyElement {
        let skeleton=|height|div().w_full().h(px(height)).rounded(px(16.)).bg(cx.theme().secondary);
        div().id("usage-loading").test_support().w_full().flex().flex_col().gap_5().role(gpui_kit::Role::Status).aria_label(self.t("usage_state.loading"))
            .child(skeleton(44.)).child(div().grid().grid_cols(if self.viewport_width>=1200.{4}else{2}).gap_4()
                .children((0..4).map(|_|skeleton(112.)))).child(skeleton(280.)).into_any_element()
    }

    /// 未扫描与失效索引不能让零值看起来可信；空扫描单独给蓝色提示。
    pub(super) fn local_index_notice(&self,state:LocalIndexState,cx:&mut Context<Self>)->AnyElement {
        let key=match state {LocalIndexState::NotScanned=>"not_scanned",LocalIndexState::NeedsRescan=>"needs_rescan",
            LocalIndexState::ReadyNoCalls=>"no_calls",LocalIndexState::Ready=>return div().into_any_element()};
        let color=rgb(if state==LocalIndexState::ReadyNoCalls {0x228be6}else{0xfd7e14});
        div().w_full().p_4().rounded(px(8.)).bg(color.alpha(0.1)).text_color(color).flex().flex_col().items_start().gap_3()
            .child(div().text_sm().font_weight(FontWeight::BOLD).child(self.t(&format!("usage_state.{key}_title"))))
            .child(div().text_sm().child(self.t(&format!("usage_state.{key}_body"))))
            .child(Button::new("usage-index-sources").ghost().text_color(color).bg(color.alpha(0.1)).label(self.t(&format!("usage_state.{key}_action")))
                .on_click(cx.listener(|this,_,_,cx|{if this.dashboard.client==0 {this.dashboard.client=1;}this.dashboard.route=Route::Sources;this.reload_usage(false,cx);})))
            .into_any_element()
    }
}
