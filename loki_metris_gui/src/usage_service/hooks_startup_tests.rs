//! 桌宠先提交配置时，真实 TCP listener 仍必须消费同一权威启用集合。

use super::super::UsageService;
use crate::monitor::hook_test_support::HookCommandText as _;
use crate::monitor::listener::HookRelayStatus;
use loki_metris_core::{
    AiTool, HookBehavior, PetLayout, PetOverlayPosition, PetPreferenceChange, PetSizeRange,
    HOOK_EVENT_TYPE_HEADER, HOOK_RELAY_INSTANCE_HEADER,
};
use std::{io::{Read, Write}, net::TcpStream, path::Path, time::{Duration, Instant}};

/// 使用真实环回边界，测试标识只存在于临时目录与内存。
fn post(path: &Path, tool: &str, event: &str) {
    let endpoint: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let instance = endpoint["instanceId"].as_str().unwrap();
    let port = endpoint["port"].as_u64().unwrap() as u16;
    let body = serde_json::json!({"hook_event_name":event,"session_id":"pet-first-session"}).to_string();
    let request = format!("POST /api/hooks/{instance}/{tool} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{HOOK_RELAY_INSTANCE_HEADER}: {instance}\r\n{HOOK_EVENT_TYPE_HEADER}: {event}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    let mut stream = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(1)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 202"), "{response}");
}

/// 只等待可观察状态，失败保留最终快照；不把 HTTP 接受误算为状态机处理。
fn wait_status(service: &UsageService, predicate: impl Fn(&HookRelayStatus) -> bool) -> HookRelayStatus {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let status = service.monitor_status().unwrap();
        if predicate(&status) { return status; }
        assert!(Instant::now() < deadline, "relay state: {status:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// 偏好、位置与默认偏好无写入首次提交都不能把空 listener 门禁永久缓存下来。
#[test]
fn pet_first_commit_publishes_enabled_tools_to_the_running_listener() {
    for first in 0..3 {
        let root = tempfile::tempdir().unwrap();
        let endpoint = root.path().join("cache/endpoint.json");
        let service = UsageService::in_test_directory(root.path().join("data")).unwrap();
        service.start_test_monitoring_at(endpoint.clone()).unwrap();
        wait_status(&service, |status| status.listening);
        let receipt = if first == 1 {
            service.save_pet_position(PetOverlayPosition { x: 24, y: 36 }).unwrap()
        } else {
            service.change_pet_preferences(PetPreferenceChange::Locked(first == 0), PetSizeRange::for_work_area(1080., PetLayout::Grid)).unwrap()
        };
        let saved = receipt.blocking_recv().unwrap().unwrap();
        let cached = service.monitor_settings().unwrap().blocking_recv().unwrap().unwrap();
        assert_eq!(cached.enabled_ai_tools, saved.enabled_ai_tools);
        post(&endpoint, "codex", "SessionStart");
        let status = wait_status(&service, |status| status.received_count == 1);
        assert!(status.pet_states.iter().any(|state| state.tool == AiTool::Codex && state.behavior == Some(HookBehavior::Idle)));
        assert_eq!(status.failed_count, 0);
        service.finish();
    }
}

/// 同位置无写入也会读取新磁盘事实，必须同步禁用旧工具并接收新工具事件。
#[test]
fn pet_position_noop_refreshes_listener_selection_from_disk() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    let endpoint = root.path().join("cache/endpoint.json");
    let service = UsageService::in_test_directory(data.clone()).unwrap();
    service.start_test_monitoring_at(endpoint.clone()).unwrap();
    wait_status(&service, |status| status.listening);
    service.monitor_settings().unwrap().blocking_recv().unwrap().unwrap();
    let position = PetOverlayPosition { x: 24, y: 36 };
    service.save_pet_position(position).unwrap().blocking_recv().unwrap().unwrap();
    post(&endpoint, "codex", "SessionStart");
    wait_status(&service, |status| status.received_count == 1);
    crate::monitor::settings::update_monitor_settings(&data.join("monitor"), |settings| {
        settings.enabled_ai_tools = vec![AiTool::Grok];
    }).unwrap();
    let saved = service.save_pet_position(position).unwrap().blocking_recv().unwrap().unwrap();
    assert_eq!(saved.enabled_ai_tools, vec![AiTool::Grok]);
    assert_eq!(service.pet_settings_revision(), 1);
    assert!(service.monitor_status().unwrap().pet_states.iter().all(|state| state.behavior.is_none()));
    post(&endpoint, "codex", "UserPromptSubmit");
    post(&endpoint, "grok", "SessionStart");
    let status = wait_status(&service, |status| status.received_count == 2);
    assert_eq!(status.last_event.unwrap().tool, AiTool::Grok);
    service.finish();
}

/// 同位置提交吸收新的 Hooks 目录时，自动维护必须迁到最新的已注册配置根。
#[test]
fn pet_position_noop_refreshes_owned_hook_repair_directory() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    let config = data.join("monitor");
    let old = root.path().join("old-codex");
    let next = root.path().join("next-codex");
    for directory in [&old, &next] {
        std::fs::create_dir_all(directory).unwrap();
        let generated = loki_metris_core::generate_hook_config(AiTool::Codex, Path::new("/previous/LokiMetris")).unwrap();
        std::fs::write(directory.join("hooks.json"), generated.content).unwrap();
    }
    crate::monitor::settings::update_monitor_settings(&config, |settings| {
        settings.enabled_ai_tools = vec![AiTool::Codex];
        settings.hook_directories.codex = old.to_string_lossy().into_owned();
    }).unwrap();
    let service = UsageService::in_test_directory(data).unwrap();
    service.start_test_hook_writer("/new/LokiMetris".into(), root.path().join("home")).unwrap();
    service.monitor_settings().unwrap().blocking_recv().unwrap().unwrap();
    wait_command(&old.join("hooks.json"), "/new/LokiMetris");
    let position = PetOverlayPosition { x: 24, y: 36 };
    service.save_pet_position(position).unwrap().blocking_recv().unwrap().unwrap();
    crate::monitor::settings::update_monitor_settings(&config, |settings| {
        settings.hook_directories.codex = next.to_string_lossy().into_owned();
    }).unwrap();
    service.save_pet_position(position).unwrap().blocking_recv().unwrap().unwrap();
    wait_command(&next.join("hooks.json"), "/new/LokiMetris");
    assert_eq!(service.pet_settings_revision(), 1);
    service.finish();
}

/// 确认真实后台文件已替换，等待上限覆盖一次完整自动修复周期。
fn wait_command(path: &Path, executable: &str) {
    let deadline = Instant::now() + Duration::from_secs(7);
    loop {
        if std::fs::read_to_string(path).is_ok_and(|text| text.hook_command_text().contains(executable)) { return; }
        assert!(Instant::now() < deadline, "Hook command was not repaired: {}", path.display());
        std::thread::sleep(Duration::from_millis(5));
    }
}
