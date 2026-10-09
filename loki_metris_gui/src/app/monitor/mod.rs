//! 原生监控工作台直接展示环回宿主快照，不访问第三方配置。

use super::Shell;
mod state;
mod catalog;
mod management;
mod picker;
mod images;
mod hooks;
#[cfg(test)]
mod catalog_tests;
use crate::monitor::listener::HookRelayStatus;
use gpui_kit::{AnyElement,Context,FontWeight,IntoElement,Role,base::TestSupportExt as _,component::{ActiveTheme,button::Button},div,prelude::*,px,rgb};

/// 窗口只保留已读投影，端口、计数和事件由监控宿主拥有。
#[derive(Default)]
pub(super) struct MonitorView {
    route:state::Route,
    catalog:state::Catalog,
    hooks:hooks::HooksView,
    status:Option<HookRelayStatus>,
    error:Option<&'static str>,
    last_read:Option<std::time::Instant>,
}

impl Shell {
    /// 工作台以参考三秒节奏刷新；可见但失焦的窗口仍显示最新中继状态。
    pub(super) fn refresh_monitor(&mut self,force:bool,cx:&mut Context<Self>) {
        if !force&&self.monitor.last_read.is_some_and(|read|read.elapsed()<std::time::Duration::from_secs(3)) {return}
        self.monitor.last_read=Some(std::time::Instant::now());
        let next=self.dashboard.service.as_ref().ok_or("monitor_service_unavailable").and_then(|service|service.monitor_status());
        match next {Ok(status)=>{self.monitor.status=Some(status);self.monitor.error=None;},Err(error)=>{self.monitor.error=Some(error);}}
        cx.notify();
    }

    /// 当前检查点提供真实工作台；其它监控子页在各自闭环接入后增加导航。
    pub(super) fn monitor_page(&self,window:&mut gpui_kit::Window,cx:&mut Context<Self>)->AnyElement {
        let mut tabs=div().w_full().flex().items_center().p_2().gap_1().border_y_1().border_color(cx.theme().border).bg(rgb(if cx.theme().mode.is_dark(){0x191a1c}else{0xf9f7f4}));
        for (route,id,key) in [(state::Route::Workbench,"monitor-workbench-tab","workbench"),(state::Route::Management,"monitor-management-tab","management"),(state::Route::Images,"monitor-images-tab","images"),(state::Route::Hooks,"monitor-hooks-tab","settings")] {
            tabs=tabs.child(gpui_kit::base::Button::new(id).selected(self.monitor.route==route).accessibility_label(format!("{}：{}",self.t(&format!("monitor.navigation.{key}.label")),self.t(&format!("monitor.navigation.{key}.description")))).child(self.t(&format!("monitor.navigation.{key}.label"))).px_3().font_weight(FontWeight::SEMIBOLD).h(px(40.)).rounded(px(12.)).when(self.monitor.route==route,|button|button.bg(cx.theme().accent).text_color(cx.theme().accent_foreground)).on_click(cx.listener(move|this,_,_,cx|this.select_monitor_route(route,cx))));
        }
        if self.monitor.route!=state::Route::Workbench {return div().w_full().flex_shrink_0().flex().flex_col().gap_5().child(tabs).child(match self.monitor.route {state::Route::Management=>self.monitor_management(window,cx),state::Route::Hooks=>self.monitor_hooks(cx),_=>self.monitor_images(window,cx)}).into_any_element()}
        let mut page=div().id("monitor-workbench").test_support().w_full().flex().flex_col().gap_5().child(tabs);
        if let Some(error)=self.monitor.error {
            page=page.child(div().id("monitor-error").test_support().role(Role::Alert).aria_label(self.t(error)).rounded(px(12.)).p_4().bg(cx.theme().danger.opacity(0.1)).text_color(cx.theme().danger).child(self.t(error))
                .child(Button::new("monitor-retry").label(self.t("ui.retry")).on_click(cx.listener(|this,_,_,cx|this.refresh_monitor(true,cx)))));
        }
        let Some(status)=&self.monitor.status else {return page.into_any_element()};
        let listening=if status.listening {self.t("monitor.workbench.listening").replace("%{address}",&status.bind_address)}else{self.t("monitor.workbench.offline")};
        let color=rgb(if status.listening {0x40c057}else{0xfa5252});
        page=page.child(super::surface::SpotlightCard::new("monitor-workbench-card",cx.theme().mode.is_dark()).test_support().gap_4().p_4()
            .child(div().w_full().flex().items_start().justify_between().gap_3()
                .child(div().text_size(px(22.)).font_weight(FontWeight::BOLD).child(self.t("monitor.workbench.title")))
                .child(div().id("monitor-listening").role(Role::Status).aria_label(listening.clone()).px(px(10.)).h(px(20.)).rounded_full().border_1().border_color(color.opacity(0.26)).bg(color.opacity(0.09)).text_color(if cx.theme().mode.is_dark(){rgb(0xd3f9d8)}else{color}).text_size(px(11.)).font_weight(FontWeight::BOLD).flex().items_center().child(listening)))
            .child(div().w_full().grid().grid_cols(if self.viewport_width>=768.{2}else{1}).gap_3()
                .child(self.monitor_metric("monitor-received","monitor.workbench.receivedLabel",status.received_count,cx))
                .child(self.monitor_metric("monitor-failed","monitor.workbench.failedLabel",status.failed_count,cx)))
            .child(if let Some(event)=&status.last_event {
                let tool=loki_metris_core::ai_tool_name(event.tool);
                let label=format!("{} {} / {}",self.t("monitor.workbench.lastEventLabel"),tool,event.hook_type);
                div().id("monitor-last-event").role(Role::Label).aria_label(label.clone()).text_size(px(14.)).child(label)
            }else{div().id("monitor-last-event").role(Role::Label).aria_label(self.t("monitor.workbench.noEvent")).text_size(px(14.)).text_color(cx.theme().muted_foreground).child(self.t("monitor.workbench.noEvent"))})
            .when_some(status.last_error.as_ref(),|card,error|card.child(div().id("monitor-last-error").role(Role::Alert).aria_label(error.clone()).rounded(px(12.)).p_4().bg(cx.theme().danger.opacity(0.1)).text_color(cx.theme().danger).child(self.t("monitor.workbench.lastError").replace("%{error}",error)))));
        page.into_any_element()
    }

    /// 指标块同时提供完整可访问标签与实际数值，周围表面没有操作。
    fn monitor_metric(&self,id:&'static str,key:&str,value:u64,cx:&Context<Self>)->AnyElement {
        div().id(id).test_support().role(Role::Label).aria_label(format!("{} {value}",self.t(key))).w_full().min_w_0().p(px(14.)).rounded(px(12.)).border_1().border_color(cx.theme().border).bg(rgb(if cx.theme().mode.is_dark(){0x26231f}else{0xf0ebe3})).flex().flex_col().gap_1()
            .child(div().text_size(px(12.)).text_color(cx.theme().muted_foreground).child(self.t(key)))
            .child(div().text_size(px(20.)).font_weight(FontWeight::BOLD).child(value.to_string())).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 后台重读失败保留上次指标并显示错误；从未成功读取时只显示失败和重试。
    #[gpui_kit::test]
    fn monitor_refresh_failure_keeps_previous_metrics(cx:&mut gpui_kit::TestAppContext) {
        use gpui_kit::test::TestWindowExt;
        let(handle,shell,_finish)=crate::app::tests::open_shell(cx,1440.,900.);
        cx.update_window(handle,|_,window,cx| {
            shell.update(cx,|this,cx| {
                this.page=crate::app::Page::Monitor;
                this.monitor.status=Some(HookRelayStatus{received_count:42,failed_count:3,..Default::default()});
                this.refresh_monitor(true,cx);
                assert_eq!(this.monitor.status.as_ref().unwrap().received_count,42);
                assert!(this.monitor.error.is_some());
            });
            window.render_frame(cx);
            assert!(window.try_find("monitor-error").is_some());
            assert!(window.try_find("monitor-received").is_some());
            shell.update(cx,|this,cx|{this.monitor.status=None;cx.notify();});
            window.render_frame(cx);
            assert!(window.try_find("monitor-retry").is_some());
            assert!(window.try_find("monitor-received").is_none());
        }).unwrap();
    }
    /// 工作台在原生导航中可达，真实指标布局既不挤压也不代理点击。
    #[gpui_kit::test]
    fn workbench_projection_has_two_readable_metric_columns(cx:&mut gpui_kit::TestAppContext) {
        use gpui_kit::test::TestWindowExt;
        let(handle,shell,_finish)=crate::app::tests::open_shell(cx,1440.,900.);
        cx.update_window(handle,|_,window,cx| {
            shell.update(cx,|this,_cx|{this.page=crate::app::Page::Monitor;this.monitor.status=Some(HookRelayStatus{listening:true,bind_address:"127.0.0.1:12345".into(),received_count:42,..Default::default()});});
            window.render_frame(cx);
            assert!(window.try_find("nav-monitor").is_some());
            let left=window.find("monitor-received").bounds();let right=window.find("monitor-failed").bounds();
            assert!(left.size.width>px(400.));assert_eq!(left.origin.y,right.origin.y);assert!(right.origin.x>left.right());
            assert!(window.try_find("monitor-workbench").is_some());
        }).unwrap();
    }
}
