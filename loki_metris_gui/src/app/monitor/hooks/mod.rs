//! 原生Hooks设置的查询、目录草稿、保存与写入回执。

mod state;
mod view;
use super::{Shell, state::Route};
use gpui_kit::{
    Context, Window,
    component::input::{InputEvent, InputState},
    prelude::*,
};
use loki_metris_core::{AiTool, HookError, public_monitor_ai_tools};
use state::Feedback;
pub(super) use state::HooksView;
#[cfg(test)]
mod tests;

impl Shell {
    /// Hooks工具集合只消费公共设置，与档案是否已经加载无关。
    fn monitor_hook_tools(&self) -> Vec<AiTool> {
        public_monitor_ai_tools()
            .filter(|tool| {
                self.agent_settings
                    .value
                    .as_ref()
                    .is_some_and(|settings| settings.enabled_ai_tools.contains(tool))
            })
            .collect()
    }
    /// 已停用标签回退到第一个启用项，空集合不创造可写默认工具。
    fn monitor_hook_tool(&self) -> Option<AiTool> {
        let tools = self.monitor_hook_tools();
        self.monitor
            .hooks
            .selected
            .filter(|tool| tools.contains(tool))
            .or_else(|| tools.first().copied())
    }
    /// 原生输入实体稳定，只有初始化、目录选择与保存回执重置输入。
    pub(in crate::app) fn sync_monitor_hook_inputs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != crate::app::Page::Monitor || self.monitor.route != Route::Hooks {
            return;
        }
        let placeholder = self.t("monitor.settings.directoryPlaceholder");
        for editor in &mut self.monitor.hooks.editors {
            let required = 2 + editor.saved.additional_config_paths.len();
            if editor
                .inputs
                .as_ref()
                .is_none_or(|inputs| inputs.len() != required)
            {
                editor.subscriptions.clear();
                let inputs = (0..required)
                    .map(|index| {
                        cx.new(|cx| {
                            let state = InputState::new(window, cx);
                            if index == 0 {
                                state.placeholder(placeholder.clone())
                            } else {
                                state
                            }
                        })
                    })
                    .collect::<Vec<_>>();
                let tool = editor.saved.tool;
                editor.subscriptions.push(cx.subscribe_in(
                    &inputs[0],
                    window,
                    move |this, input, event, _, cx| {
                        if matches!(event, InputEvent::Change) {
                            let value = input.read(cx).value().to_string();
                            let mut changed = false;
                            if let Some(editor) = this.monitor.hooks.editor_mut(tool) {
                                changed = editor.draft != value;
                                editor.draft = value;
                            }
                            if changed {
                                this.monitor.hooks.clear_feedback();
                            }
                            cx.notify();
                        }
                    },
                ));
                editor.inputs = Some(inputs);
                editor.reset = true;
            }
            if editor.reset {
                let values = std::iter::once(editor.draft.clone())
                    .chain(std::iter::once(editor.saved.config_path.clone()))
                    .chain(editor.saved.additional_config_paths.iter().cloned());
                for (input, value) in editor.inputs.as_ref().unwrap().iter().zip(values) {
                    input.update(cx, |input, cx| input.set_value(value, window, cx));
                }
                editor.reset = false;
            }
            // 重新定位只同步只读投影，不重置目录草稿的光标和撤销记录。
            let values = std::iter::once(&editor.saved.config_path)
                .chain(editor.saved.additional_config_paths.iter());
            for (input, value) in editor.inputs.as_ref().unwrap().iter().skip(1).zip(values) {
                if input.read(cx).value().as_str() != value {
                    input.update(cx, |input, cx| input.set_value(value.clone(), window, cx));
                }
            }
        }
    }

    /// 初始定位失败允许重试，重复成功查询不覆盖未提交路径。
    pub(super) fn load_monitor_hook_locations(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.monitor.hooks;
        state.loading = true;
        state.error = None;
        state.query = state.query.wrapping_add(1);
        let query = state.query;
        let work = self
            .dashboard
            .service
            .as_ref()
            .and_then(|service| service.list_monitor_hook_locations().ok());
        let Some(work) = work else {
            state.loading = false;
            state.error = Some(HookError::new("error.hooks.locationNotFound"));
            cx.notify();
            return;
        };
        state.query_task = Some(cx.spawn(async move |entity, cx| {
            let result = super::catalog::receipt(
                work,
                cx.background_executor().clone(),
                "error.hooks.locationNotFound",
            )
            .await;
            let _ = entity.update(cx, |this, cx| {
                let state = &mut this.monitor.hooks;
                if state.query != query {
                    return;
                }
                state.loading = false;
                match result {
                    Ok(locations) => state.accept_locations(locations),
                    Err(error) => state.error = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// 保存捕获本标签草稿；恢复默认仅提交空目录，不在GUI拼接配置文件名。
    fn save_monitor_hook_directory(&mut self, tool: AiTool, restore: bool, cx: &mut Context<Self>) {
        let state = &mut self.monitor.hooks;
        let Some(editor) = state.editor_mut(tool) else {
            return;
        };
        if editor.saving
            || (!restore && (!editor.dirty() || editor.draft.trim().is_empty()))
            || (restore && !editor.saved.is_custom)
        {
            return;
        }
        let submitted_draft = editor.draft.clone();
        let directory = if restore {
            String::new()
        } else {
            submitted_draft.clone()
        };
        let work = self
            .dashboard
            .service
            .as_ref()
            .and_then(|service| service.save_hook_config_directory(tool, directory).ok());
        let Some(work) = work else {
            state.feedback = Some((
                tool,
                Feedback::Failed(HookError::new("error.monitor.settingsWriteFailed")),
            ));
            cx.notify();
            return;
        };
        editor.saving = true;
        // 取消旧定位回执的UI观察，已接受的后台查询仍由服务owner收尾。
        state.invalidate_query();
        let feedback_revision = state.clear_feedback();
        let task = cx.spawn(async move |entity, cx| {
            let result = super::catalog::receipt(
                work,
                cx.background_executor().clone(),
                "error.monitor.settingsWriteFailed",
            )
            .await;
            let _ = entity.update(cx, |this, cx| {
                let state = &mut this.monitor.hooks;
                state.finish_directory_save(tool, &submitted_draft, feedback_revision, result);
                cx.notify();
            });
        });
        self.monitor.hooks.editor_mut(tool).unwrap().save_task = Some(task);
        cx.notify();
    }

    /// 选择器只修改草稿，取消不改变路径和反馈，也不写入Hooks。
    fn choose_monitor_hook_directory(&mut self, tool: AiTool, cx: &mut Context<Self>) {
        let Some(editor) = self.monitor.hooks.editor_mut(tool) else {
            return;
        };
        if editor.picking {
            return;
        }
        editor.picking = true;
        let feedback_revision = self.monitor.hooks.feedback_revision;
        let selected = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(self.t("monitor.settings.directoryDialogTitle").into()),
        });
        let task = cx.spawn(async move |entity, cx| {
            let result = selected.await;
            let _ = entity.update(cx, |this, cx| {
                let state = &mut this.monitor.hooks;
                if let Some(editor) = state.editor_mut(tool) {
                    editor.picking = false;
                    match result {
                        Ok(Ok(Some(paths))) if !paths.is_empty() => {
                            editor.draft = paths[0].to_string_lossy().into_owned();
                            editor.reset = true;
                            if state.feedback_revision == feedback_revision {
                                state.clear_feedback();
                            }
                        }
                        Ok(Ok(_)) => {}
                        _ => {
                            state.accept_feedback(
                                feedback_revision,
                                tool,
                                Feedback::Failed(HookError::new("error.hooks.locationNotFound")),
                            );
                        }
                    }
                }
                cx.notify();
            });
        });
        self.monitor.hooks.editor_mut(tool).unwrap().pick_task = Some(task);
        cx.notify();
    }

    /// 只能提交已保存定位；真实结果码决定激活、重启或人工复核提示。
    fn write_monitor_hooks(&mut self, tool: AiTool, cx: &mut Context<Self>) {
        if self.monitor.hooks.loading {
            return;
        }
        let state = &mut self.monitor.hooks;
        let Some(editor) = state.editor_mut(tool) else {
            return;
        };
        if editor.writing || editor.dirty() {
            return;
        }
        let work = self
            .dashboard
            .service
            .as_ref()
            .and_then(|service| service.write_monitor_hook_config(tool).ok());
        let Some(work) = work else {
            state.feedback = Some((
                tool,
                Feedback::Failed(HookError::new("error.hooks.writeFailed")),
            ));
            cx.notify();
            return;
        };
        editor.writing = true;
        let feedback_revision = state.clear_feedback();
        let task = cx.spawn(async move |entity, cx| {
            let result = super::catalog::receipt(
                work,
                cx.background_executor().clone(),
                "error.hooks.writeFailed",
            )
            .await;
            let _ = entity.update(cx, |this, cx| {
                let state = &mut this.monitor.hooks;
                if let Some(editor) = state.editor_mut(tool) {
                    editor.writing = false;
                }
                let refresh = result.is_ok();
                state.accept_feedback(
                    feedback_revision,
                    tool,
                    match result {
                        Ok(result) => Feedback::Written(result),
                        Err(error) => Feedback::Failed(error),
                    },
                );
                if refresh {
                    this.load_monitor_hook_locations(cx);
                }
                cx.notify();
            });
        });
        self.monitor.hooks.editor_mut(tool).unwrap().write_task = Some(task);
        cx.notify();
    }
}
