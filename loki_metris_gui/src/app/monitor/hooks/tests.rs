//! 真实GPUI窗口验证Hooks路径草稿、禁用边界与原生选择器回执。

use loki_metris_service::monitor::hook_test_support::HookCommandText as _;
use super::*;
use gpui_kit::{AppContext, Entity, TestAppContext, test::TestWindowExt};
use std::{sync::Arc, time::Duration};

/// 每项回归只拥有自己的临时服务，退出等待已接受操作。
struct Finish(Arc<loki_metris_service::usage_service::UsageService>);
impl Drop for Finish {
    /// 服务收尾先于临时目录销毁。
    fn drop(&mut self) {
        self.0.finish();
    }
}

/// 等待真实文件线程回执，GPUI定时器仍使用测试时钟。
fn settle(cx: &mut TestAppContext, shell: &Entity<Shell>, done: impl Fn(&Shell) -> bool) {
    for _ in 0..500 {
        cx.run_until_parked();
        if shell.read_with(cx, |this, _| done(this)) {
            return;
        }
        cx.executor().advance_clock(Duration::from_millis(30));
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("hook UI receipt did not settle");
}

#[gpui_kit::test]
/// 目录提示换语言不保存或重建输入，不清除未保存目录与选区。
fn directory_placeholder_follows_language_without_resetting_input(cx: &mut TestAppContext) {
    use crate::{app::Page, preferences::LanguageChoice};
    let root = tempfile::tempdir().unwrap();
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    let inputs = cx.update_window(handle, |_, window, cx| {
        shell.update(cx, |this, _| {
            this.page = Page::Monitor;
            this.monitor.route = Route::Hooks;
            this.monitor.hooks.accept_locations(loki_metris_service::monitor::store::list_hook_config_locations(
                &loki_metris_service::monitor::settings::MonitorSettings::default(), root.path()));
        });
        window.render_frame(cx);
        let inputs: Vec<_> = shell.read(cx).monitor.hooks.editors.iter()
            .map(|editor| editor.inputs.as_ref().unwrap()[0].clone()).collect();
        for (index, input) in inputs.iter().enumerate() {
            input.update(cx, |input, cx| {
                input.replace_all(format!("unsaved directory {index}"), window, cx);
                input.set_selected_range(1..3, cx);
            });
        }
        inputs
    }).unwrap();
    cx.run_until_parked();
    let focuses: Vec<_> = inputs.iter().map(|input| input.read_with(cx, |input, _| input.presentation().focus_handle().clone())).collect();
    let revision = shell.read_with(cx, |this, _| this.monitor.hooks.feedback_revision);
    for (language, expected) in [
        (LanguageChoice::English, "Use an absolute path or a WSL UNC path"),
        (LanguageChoice::Chinese, "请输入绝对路径或 WSL UNC 路径"),
        (LanguageChoice::English, "Use an absolute path or a WSL UNC path"),
    ] {
        cx.update_window(handle, |_, window, cx| shell.update(cx, |this, cx| {
            this.navigate(Page::Settings, cx);
            this.select_language(language, window, cx);
        })).unwrap();
        settle(cx, &shell, |this| this.preference_save.pending.is_none());
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |this, cx| this.navigate(Page::Monitor, cx));
            window.render_frame(cx);
            for (index, editor) in shell.read(cx).monitor.hooks.editors.iter().enumerate() {
                assert_eq!(&editor.inputs.as_ref().unwrap()[0], &inputs[index]);
                assert_eq!(editor.draft, format!("unsaved directory {index}"));
                assert!(editor.dirty());
                let input = inputs[index].read(cx);
                assert_eq!(input.presentation().placeholder().as_str(), expected);
                assert_eq!(input.value().as_str(), editor.draft);
                assert_eq!(input.selected_range(), 1..3);
                assert_eq!(input.presentation().focus_handle(), &focuses[index]);
            }
        }).unwrap();
        cx.run_until_parked();
        shell.read_with(cx, |this, _| assert_eq!(this.monitor.hooks.feedback_revision, revision));
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0, "仅翻译提示不得写Hooks");
}

/// 保存路径只写被捕获工具，未保存路径阻止写入，切换标签保留其它草稿。
#[gpui_kit::test]
fn hook_directory_controls_preserve_tool_drafts_and_disable_unsaved_write(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("data");
    let settings =
        loki_metris_service::monitor::settings::update_monitor_settings(&directory.join("monitor"), |settings| {
            for tool in public_monitor_ai_tools() {
                settings.hook_directories.set(
                    tool,
                    root.path()
                        .join(loki_metris_core::ai_tool_name(tool))
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        })
        .unwrap();
    let service = loki_metris_service::usage_service::UsageService::in_test_directory(directory.clone()).unwrap();
    let _finish = Finish(service.clone());
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    shell.update(cx, |this, cx| {
        this.dashboard.service = Some(service);
        this.page = crate::app::Page::Monitor;
        this.monitor.route = Route::Hooks;
        this.monitor
            .hooks
            .accept_locations(loki_metris_service::monitor::store::list_hook_config_locations(
                &settings,
                root.path(),
            ));
        cx.notify();
    });
    let new_path = root.path().join("saved-codex");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("monitor-hooks-tab").is_some());
        let field = window.find("monitor-hook-directory").bounds();
        assert_eq!(field.size.height, gpui_kit::px(30.));
        let panel = window.find("monitor-hooks-panel").bounds();
        assert!(window.find("monitor-hook-write").bounds().right() <= panel.right());
        let input = shell.read(cx).monitor.hooks.editors[0]
            .inputs
            .as_ref()
            .unwrap()[0]
            .clone();
        input.update(cx, |input, cx| {
            input.replace_all(format!(" {} ", new_path.display()), window, cx)
        });
    })
    .unwrap();
    // 真实键盘输入事件完成后，下一次鼠标操作才读取更新的草稿。
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("monitor-hook-write", cx);
        assert!(shell.read(cx).monitor.hooks.feedback.is_none());
        window.click("monitor-hook-save", cx);
        window.click("monitor-hook-agent-Claude Code", cx);
    })
    .unwrap();
    settle(cx, &shell, |this| !this.monitor.hooks.editors[0].saving);
    shell.read_with(cx, |this, _| {
        assert_eq!(this.monitor_hook_tool(), Some(AiTool::ClaudeCode));
        assert_eq!(
            this.monitor.hooks.editors[0].saved.directory,
            new_path.to_string_lossy()
        );
    });
    let saved =
        loki_metris_service::monitor::settings::load_monitor_settings(&directory.join("monitor")).unwrap();
    assert_eq!(saved.hook_directories.codex, new_path.to_string_lossy());
    assert_eq!(
        saved.hook_directories.claude_code,
        settings.hook_directories.claude_code
    );
}

/// 文件选择器取消零保存，选择结果仍修改发起工具而非随后切换的标签。
#[gpui_kit::test]
fn hook_folder_picker_only_changes_captured_draft_and_cancel_keeps_feedback(
    cx: &mut TestAppContext,
) {
    let root = tempfile::tempdir().unwrap();
    let settings = loki_metris_service::monitor::settings::MonitorSettings::default();
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    shell.update(cx, |this, cx| {
        this.page = crate::app::Page::Monitor;
        this.monitor.route = Route::Hooks;
        this.monitor
            .hooks
            .accept_locations(loki_metris_service::monitor::store::list_hook_config_locations(
                &settings,
                root.path(),
            ));
        this.monitor.hooks.feedback = Some((
            AiTool::Codex,
            Feedback::Failed(HookError::new("error.hooks.writeFailed")),
        ));
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("monitor-hook-choose", cx);
    })
    .unwrap();
    cx.simulate_path_prompt_response(|options| {
        assert!(options.directories);
        assert!(!options.files);
        assert!(!options.multiple);
        None
    });
    settle(cx, &shell, |this| !this.monitor.hooks.editors[0].picking);
    assert!(shell.read_with(cx, |this, _| this.monitor.hooks.feedback.is_some()));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("monitor-hook-choose", cx);
        window.click("monitor-hook-agent-Claude Code", cx);
    })
    .unwrap();
    let selected = root.path().join("selected-draft");
    let picked = selected.clone();
    cx.simulate_path_prompt_response(move |_| Some(vec![picked]));
    settle(cx, &shell, |this| !this.monitor.hooks.editors[0].picking);
    shell.read_with(cx, |this, _| {
        assert_eq!(this.monitor_hook_tool(), Some(AiTool::ClaudeCode));
        assert_eq!(
            this.monitor.hooks.editors[0].draft,
            selected.to_string_lossy()
        );
        assert!(!this.monitor.hooks.editors[1].dirty());
    });
    assert!(!selected.exists());
}

/// WorkBuddy默认两份路径都以可复制输入显示；空集合没有可提交工具。
#[gpui_kit::test]
fn hook_workbuddy_additional_path_and_empty_selection_are_real_projections(
    cx: &mut TestAppContext,
) {
    let root = tempfile::tempdir().unwrap();
    let settings = loki_metris_service::monitor::settings::MonitorSettings::default();
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    shell.update(cx, |this, cx| {
        this.page = crate::app::Page::Monitor;
        this.monitor.route = Route::Hooks;
        this.monitor.hooks.selected = Some(AiTool::WorkBuddy);
        this.monitor
            .hooks
            .accept_locations(loki_metris_service::monitor::store::list_hook_config_locations(
                &settings,
                root.path(),
            ));
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("monitor-hook-extra-0").is_some());
        shell.update(cx, |this, cx| {
            this.agent_settings
                .value
                .as_mut()
                .unwrap()
                .enabled_ai_tools
                .clear();
            cx.notify();
        });
        window.render_frame(cx);
        assert!(window.try_find("monitor-hook-write").is_none());
        assert!(window.try_find("monitor-hook-feedback").is_some());
    })
    .unwrap();
}

/// 重复定位查询不能覆盖已输入但尚未提交的草稿。
#[test]
fn refreshed_hook_locations_keep_unsaved_directory_draft() {
    let root = tempfile::tempdir().unwrap();
    let settings = loki_metris_service::monitor::settings::MonitorSettings::default();
    let locations = loki_metris_service::monitor::store::list_hook_config_locations(&settings, root.path());
    let mut state = HooksView::default();
    state.accept_locations(locations.clone());
    state.editors[0].draft = "/unsaved".into();
    state.accept_locations(locations);
    assert_eq!(state.editors[0].draft, "/unsaved");
    assert!(state.editors[0].dirty());
}

/// 保存期间的旧写入刷新失效，回执不能覆盖用户后来输入的目录。
#[test]
fn saved_hook_directory_invalidates_interleaved_query_and_keeps_later_draft() {
    let root = tempfile::tempdir().unwrap();
    let settings = loki_metris_service::monitor::settings::MonitorSettings::default();
    let mut state = HooksView::default();
    state.accept_locations(loki_metris_service::monitor::store::list_hook_config_locations(
        &settings,
        root.path(),
    ));
    let tool = AiTool::Codex;
    let mut saved = state.editors[0].saved.clone();
    saved.directory = "/submitted".into();
    saved.config_path = "/submitted/hooks.json".into();
    state.editors[0].saving = true;
    state.editors[0].draft = "/submitted".into();
    state.invalidate_query();
    let feedback_revision = state.clear_feedback();
    // 旧 write 在保存等待期间完成，触发一份保存前路径查询。
    state.query += 1;
    state.loading = true;
    let stale_query = state.query;
    state.editors[0].draft = "/typed-later".into();
    state.editors[0].reset = false;
    state.clear_feedback();
    state.finish_directory_save(tool, "/submitted", feedback_revision, Ok(saved.clone()));
    assert_ne!(state.query, stale_query);
    assert!(!state.loading);
    assert_eq!(state.editors[0].saved.config_path, "/submitted/hooks.json");
    assert_eq!(state.editors[0].draft, "/typed-later");
    assert!(!state.editors[0].reset);
    assert!(state.editors[0].dirty());
    assert!(!state.editors[0].saving);
    assert!(state.feedback.is_none());
    // 没有后续输入时仍规范化提交草稿。
    state.editors[0].draft = " /submitted ".into();
    state.finish_directory_save(tool, " /submitted ", feedback_revision, Ok(saved));
    assert_eq!(state.editors[0].draft, "/submitted");
    assert!(state.editors[0].reset);
    assert!(!state.editors[0].dirty());
}

/// 用户已编辑路径后旧写入仍完成，但旧结果不能重新弹出成功或错误提示。
#[gpui_kit::test]
fn edited_path_hides_old_write_receipt_without_cancelling_owned_write(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("data");
    let hook_directory = root.path().join("codex");
    let settings =
        loki_metris_service::monitor::settings::update_monitor_settings(&directory.join("monitor"), |settings| {
            settings.enabled_ai_tools.clear();
            settings.hook_directories.codex = hook_directory.to_string_lossy().into_owned();
        })
        .unwrap();
    std::fs::create_dir_all(&hook_directory).unwrap();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(hook_directory.join(".lokimetis-hook-config.lock"))
        .unwrap();
    lock.lock().unwrap();
    let service = loki_metris_service::usage_service::UsageService::in_test_directory(directory).unwrap();
    let _finish = Finish(service.clone());
    service
        .start_test_hook_writer("/fixture/LokiMetris".into(), root.path().join("home"))
        .unwrap();
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    shell.update(cx, |this, cx| {
        this.dashboard.service = Some(service);
        this.page = crate::app::Page::Monitor;
        this.monitor.route = Route::Hooks;
        this.monitor
            .hooks
            .accept_locations(loki_metris_service::monitor::store::list_hook_config_locations(
                &settings,
                root.path(),
            ));
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("monitor-hook-write", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        let input = shell.read(cx).monitor.hooks.editors[0]
            .inputs
            .as_ref()
            .unwrap()[0]
            .clone();
        input.update(cx, |input, cx| {
            input.replace_all("/later-unsaved", window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    drop(lock);
    settle(cx, &shell, |this| !this.monitor.hooks.editors[0].writing);
    shell.read_with(cx, |this, _| {
        assert_eq!(this.monitor.hooks.editors[0].draft, "/later-unsaved");
        assert!(this.monitor.hooks.feedback.is_none());
    });
    assert!(
        std::fs::read_to_string(hook_directory.join("hooks.json"))
            .unwrap()
            .hook_command_text().contains("LokiMetris:tool=codex")
    );
}

/// 定位快照变化更新已有只读输入，同时保留目录草稿与输入实体身份。
#[gpui_kit::test]
fn refreshed_hook_file_projection_does_not_reset_directory_input(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let settings = loki_metris_service::monitor::settings::MonitorSettings::default();
    let (handle, shell, _writer) = crate::app::tests::open_shell(cx, 1440., 900.);
    let locations = loki_metris_service::monitor::store::list_hook_config_locations(&settings, root.path());
    shell.update(cx, |this, cx| {
        this.page = crate::app::Page::Monitor;
        this.monitor.route = Route::Hooks;
        this.monitor.hooks.accept_locations(locations.clone());
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let inputs = shell.read_with(cx, |this, _| {
        this.monitor.hooks.editors[0]
            .inputs
            .as_ref()
            .unwrap()
            .clone()
    });
    cx.update_window(handle, |_, window, cx| {
        inputs[0].update(cx, |input, cx| {
            input.replace_all("/still-unsaved", window, cx)
        })
    })
    .unwrap();
    cx.run_until_parked();
    let mut changed = locations;
    changed[0].config_path = "/new-projection/hooks.json".into();
    shell.update(cx, |this, cx| {
        this.monitor.hooks.accept_locations(changed);
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    assert_eq!(
        inputs[0].read_with(cx, |input, _| input.value()),
        "/still-unsaved"
    );
    assert_eq!(
        inputs[1].read_with(cx, |input, _| input.value()),
        "/new-projection/hooks.json"
    );
}
