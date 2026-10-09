//! Agent 复选框只显示成功保存的选择，应用持有异步任务直到收到真实回执。

use super::super::Shell;
use crate::monitor::settings::MonitorSettings;
use gpui_kit::{AnyElement, Context, FontWeight, IntoElement, Task, Role,
    base::{Checkbox, CheckboxState}, component::{ActiveTheme, Icon}, div, prelude::*, px, rgb};
use loki_metris_core::{AiTool, SourceClientKind, dashboard_selection_for_ai_tools, public_ai_capabilities};

/// 页面之间共享已提交配置；查询代数阻止旧读取覆盖正在保存的新选择。
#[derive(Default)]
pub(in crate::app) struct AgentSettingsState {
    pub value: Option<MonitorSettings>,
    pub loading: bool,
    pub pending: bool,
    pub error: Option<&'static str>,
    pub service_revision: u64,
    query_revision: u64,
    query_task: Option<Task<()>>,
    save_task: Option<Task<()>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Cursor 在统一选择中保留，但不能伪装成看板物理来源。
    #[test]
    fn selection_indices_keep_stable_client_identity_and_ignore_cursor() {
        let state=AgentSettingsState{value:Some(MonitorSettings{enabled_ai_tools:vec![AiTool::Cursor,AiTool::Grok,AiTool::WorkBuddy],..Default::default()}),..Default::default()};
        assert_eq!(state.dashboard_indices(),vec![3,4]);
    }

    /// 保存后的能力收缩自动回退并清除不支持路由，旧控件回调不能重选停用项。
    #[gpui_kit::test]
    fn disabled_selection_falls_back_and_stale_selector_is_ignored(cx:&mut gpui_kit::TestAppContext) {
        let (handle,shell,_finish)=crate::app::tests::open_shell(cx,1440.,900.);
        cx.update_window(handle,|_,_,cx|shell.update(cx,|this,cx| {
            this.dashboard.client=3;this.dashboard.route=crate::app::dashboard::Route::Statistics;
            this.accept_agent_settings(MonitorSettings::default(),cx);assert_eq!(this.dashboard.client,1);
            this.select_usage_client(3,cx);assert_eq!(this.dashboard.client,1);
            this.accept_agent_settings(MonitorSettings{enabled_ai_tools:vec![AiTool::Cursor],..Default::default()},cx);
            assert_eq!(this.dashboard.client,0);assert_eq!(this.dashboard.route,crate::app::dashboard::Route::Overview);
        })).unwrap();
    }
}

impl AgentSettingsState {
    /// 固定客户端编号仍拥有各自筛选会话，展示子集不能重新编号。
    pub(in crate::app) fn dashboard_indices(&self) -> Vec<usize> {
        let Some(settings) = &self.value else {return Vec::new()};
        let (clients, wb) = dashboard_selection_for_ai_tools(&settings.enabled_ai_tools);
        clients.into_iter().map(|client| match client {
            SourceClientKind::Codex=>1, SourceClientKind::ClaudeCode=>2, SourceClientKind::GrokBuildCli=>3,
            SourceClientKind::WorkBuddy=>4,
        }).chain(wb.then_some(4)).collect()
    }
}

impl Shell {
    /// 初次查询前不猜测启用集合；已有值的后台刷新保持面板可用。
    pub(in crate::app) fn load_agent_settings(&mut self, cx: &mut Context<Self>) {
        let Some(service)=&self.dashboard.service else {return};
        let state=&mut self.agent_settings;
        if state.pending {return}
        state.query_revision=state.query_revision.wrapping_add(1);state.query_task.take();
        state.loading=state.value.is_none();state.error=None;
        let revision=state.query_revision;let service_revision=service.agent_settings_revision();
        let work=match service.monitor_settings() {Ok(work)=>work,Err(_)=>{state.loading=false;state.error=Some("error.monitor.settingsReadFailed");cx.notify();return}};
        state.query_task=Some(cx.spawn(async move|entity,cx| {
            let result=work.await;
            let _=entity.update(cx,|this,cx| {
                if this.agent_settings.query_revision!=revision {return}
                this.agent_settings.loading=false;this.agent_settings.service_revision=service_revision;
                match result {
                    Ok(Ok(settings))=>this.accept_agent_settings(settings,cx),
                    Ok(Err(error))=>this.agent_settings.error=Some(error.code),
                    Err(_)=>this.agent_settings.error=Some("error.monitor.settingsReadFailed"),
                }cx.notify();
            });
        }));cx.notify();
    }

    /// 保存不乐观勾选；所有复选框在提交期间一起禁用。
    fn toggle_agent(&mut self, tool:AiTool, checked:bool, cx:&mut Context<Self>) {
        if self.agent_settings.loading||self.agent_settings.pending {return}
        let Some(value)=&self.agent_settings.value else {return};
        let mut selected=value.enabled_ai_tools.clone();
        selected.retain(|candidate|*candidate!=tool);if checked {selected.push(tool);}
        let Some(service)=&self.dashboard.service else {return};
        let work=match service.save_enabled_ai_tools(selected) {Ok(work)=>work,Err(_)=>{self.agent_settings.error=Some("error.monitor.settingsWriteFailed");cx.notify();return}};
        self.agent_settings.query_revision=self.agent_settings.query_revision.wrapping_add(1);self.agent_settings.query_task.take();
        self.agent_settings.pending=true;self.agent_settings.error=None;
        self.agent_settings.save_task=Some(cx.spawn(async move|entity,cx| {
            let result=work.await;
            let _=entity.update(cx,|this,cx| {
                this.agent_settings.pending=false;
                match result {
                    Ok(Ok(settings))=>this.accept_agent_settings(settings,cx),
                    Ok(Err(error))=>this.agent_settings.error=Some(error.code),
                    Err(_)=>this.agent_settings.error=Some("error.monitor.settingsWriteFailed"),
                }cx.notify();
            });
        }));cx.notify();
    }

    /// 失效客户端按公开顺序回退；All 的启用集合变化同样重新聚合并清除旧游标。
    fn accept_agent_settings(&mut self, settings:MonitorSettings, cx:&mut Context<Self>) {
        let changed=self.agent_settings.value.as_ref().is_none_or(|value|value.enabled_ai_tools!=settings.enabled_ai_tools);
        self.agent_settings.value=Some(settings);self.agent_settings.error=None;
        if !changed {return}
        let enabled=self.agent_settings.dashboard_indices();
        if self.dashboard.client!=0&&!enabled.contains(&self.dashboard.client) {
            self.select_usage_client(enabled.first().copied().unwrap_or(0),cx);
        } else {
            self.dashboard.calls.page=None;
            self.reload_usage(false,cx);
        }
    }

    /// 原生 Checkbox 拥有键盘和辅助语义，方框与标签按参考产品的实际尺寸绘制。
    pub(super) fn agent_settings_panel(&self,cx:&mut Context<Self>)->AnyElement {
        let busy=self.agent_settings.loading||self.agent_settings.pending;
        let dark=cx.theme().mode.is_dark();
        let columns=if self.viewport_width>=992. {5}else if self.viewport_width>=768. {3}else{2};
        div().w_full().flex().flex_col().gap_3().p_5().rounded(px(20.)).border_1().border_color(cx.theme().border).bg(cx.theme().secondary)
            .child(div().flex().items_center().gap(px(10.))
                .child(div().text_size(px(19.2)).line_height(gpui_kit::relative(1.3)).font_weight(FontWeight(650.)).child(self.t("settings.agent_panel_title")))
                .child(div().h(px(20.)).px(px(10.)).rounded_full().border_1().border_color(rgb(0x228be6).opacity(0.26)).bg(rgb(0x228be6).opacity(0.09))
                    .flex().items_center().text_size(px(11.)).font_weight(FontWeight::BOLD).text_color(rgb(if dark{0xd0ebff}else{0x228be6}))
                    .child(if self.locale=="en-US"{self.t("settings.agent_panel_badge").to_uppercase()}else{self.t("settings.agent_panel_badge")})))
            .child(div().grid().grid_cols(columns).gap(px(10.)).children(public_ai_capabilities().iter().enumerate().filter_map(|(index,capability)| {
                let tool=capability.monitor_tool?;
                let checked=self.agent_settings.value.as_ref().is_some_and(|settings|settings.enabled_ai_tools.contains(&tool));
                let entity=cx.entity().downgrade();
                let border=if busy {rgb(if dark {0x424242}else{0xdee2e6}).into()}else if checked {cx.theme().primary}else {rgb(if dark {0x424242}else{0xced4da}).into()};
                let background=if busy {rgb(if dark {0x2e2e2e}else{0xe9ecef}).into()}else if checked {cx.theme().primary}else{rgb(if dark {0x2e2e2e}else{0xffffff}).into()};
                Some(Checkbox::new(("enabled-agent",index)).checked(checked).disabled(busy).accessibility_label(capability.name)
                    .flex().items_center().gap(px(12.)).min_w_0().text_size(px(14.)).line_height(gpui_kit::relative(20./14.))
                    .font_weight(FontWeight::NORMAL)
                    .text_color(if busy {cx.theme().muted_foreground}else{cx.theme().foreground})
                    .child(div().size(px(20.)).flex_shrink_0().rounded(px(8.)).border_1().border_color(border).bg(background).flex().items_center().justify_center()
                        .when(checked,|square|square.child(Icon::default().data(include_bytes!("../../../assets/icons/check.svg")).size(px(12.)).text_color(rgb(if busy {if dark {0x696969}else{0xadb5bd}}else{0xffffff})))))
                    .child(capability.name)
                    .on_change(move|next,_,_,cx|{let _=entity.update(cx,|this,cx|this.toggle_agent(tool,next==CheckboxState::Checked,cx));}))
            })))
            .when_some(self.agent_settings.error,|panel,error|panel.child(div().id("agent-settings-error").role(Role::Alert).aria_label(self.t("ui.failureTitle"))
                .w_full().p_4().rounded(px(12.)).bg(cx.theme().danger.opacity(0.1)).text_color(cx.theme().danger)
                .child(div().font_weight(FontWeight::SEMIBOLD).child(self.t("ui.failureTitle")))
                .child(self.t(error))))
            .into_any_element()
    }
}
