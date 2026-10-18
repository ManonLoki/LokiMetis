//! 执行实际保存的 POSIX Hook 命令，并跨子进程传入真实 listener；不改用户 HOME 或原配置。

use super::*;
use crate::usage_service::UsageService;
use loki_metris_core::{AiTool, HookBehavior, PetOverlayPosition, ai_tool_name, hook_config_filename};
use std::{os::unix::fs::PermissionsExt, process::{Command, Stdio}};

/// 仅由下方隔离子进程调用；普通全量测试运行此项时没有端点，立即返回。
#[test]
fn relay_command_child() {
    let Some(endpoint) = std::env::var_os("LOKI_METRIS_TEST_RELAY_ENDPOINT") else { return; };
    let arguments = std::iter::once(OsString::from("LokiMetris")).chain(std::env::args_os().skip_while(|argument| argument != "--").skip(1));
    let arguments = parse_relay_arguments(arguments).expect("relay mode").expect("valid command arguments");
    let tool = tool_from_slug(&arguments.tool_slug).unwrap();
    assert_eq!(arguments.managed_by, managed_hook_marker(tool));
    relay_native_input(tool, &arguments.tool_slug, &arguments.event, &mut std::io::stdin(), &mut std::io::stdout(), |slug, event, body| {
        post_local_hook_at(Path::new(&endpoint), slug, event, body, 0, Duration::ZERO)
    }).expect("actual relay delivers to isolated listener");
}

/// 真实保存的五种公开命令经 shell、CLI 参数解析与 relay 归约，推进启用 Agent 的展示态。
#[test]
fn saved_public_hook_commands_deliver_from_child_process_to_listener() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    let monitor = data.join("monitor");
    let endpoint = root.path().join("cache/endpoint.json");
    let executable = root.path().join("诡秘 神谕's relay");
    std::fs::write(&executable, "#!/bin/sh\nexec \"$LOKI_METRIS_TEST_RELAY_EXECUTABLE\" --exact monitor::relay::command_tests::relay_command_child --nocapture -- \"$@\"\n").unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let tools: Vec<_> = loki_metris_core::public_monitor_ai_tools().collect();
    super::super::settings::update_monitor_settings(&monitor, |settings| {
        settings.enabled_ai_tools = tools.clone();
        for &tool in &tools {
            settings.hook_directories.set(tool, root.path().join(ai_tool_name(tool)).to_string_lossy().into_owned());
        }
    }).unwrap();
    let service = UsageService::in_test_directory(data).unwrap();
    service.start_test_hook_writer(executable, root.path().join("home")).unwrap();
    service.start_test_monitoring_at(endpoint.clone()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !service.monitor_status().unwrap().listening {
        assert!(Instant::now() < deadline, "listener must bind");
        thread::sleep(Duration::from_millis(5));
    }
    // 重现真实启动竞态：桌宠先写位置，随后所有查询都命中相同缓存。
    service.save_pet_position(PetOverlayPosition { x: 24, y: 36 }).unwrap().blocking_recv().unwrap().unwrap();
    for (index, &tool) in tools.iter().enumerate() {
        service.write_monitor_hook_config(tool).unwrap().blocking_recv().unwrap().unwrap();
        let file = root.path().join(ai_tool_name(tool)).join(hook_config_filename(tool));
        let config: serde_json::Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        let event = if tool == AiTool::Cursor { "sessionStart" } else { "SessionStart" };
        let group = &config["hooks"][event][0];
        let command = group.get("command").or_else(|| group["hooks"][0].get("command")).and_then(serde_json::Value::as_str).expect("native command");
        let mut child = Command::new("sh").args(["-c", command])
            .env("LOKI_METRIS_TEST_RELAY_ENDPOINT", &endpoint)
            .env("LOKI_METRIS_TEST_RELAY_EXECUTABLE", std::env::current_exe().unwrap())
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(br#"{"session_id":"isolated-shell-session"}"#).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{tool:?}: stdout={} stderr={}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let status = service.monitor_status().unwrap();
            if status.received_count == index as u64 + 1 {
                assert_eq!(status.failed_count, 0);
                assert!(status.pet_states.iter().any(|state| state.tool == tool && state.behavior == Some(HookBehavior::Idle)), "{tool:?}: {status:?}");
                break;
            }
            assert!(Instant::now() < deadline, "{tool:?}: {status:?}");
            thread::sleep(Duration::from_millis(5));
        }
    }
    service.finish();
    assert!(!endpoint.exists());
}
