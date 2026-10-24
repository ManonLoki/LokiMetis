//! 隔离目录中验证真实 TCP 中继、统一启用集合和退出回收。

use super::*;
use std::{io::{Read,Write},net::TcpStream,time::{Duration,Instant}};
use loki_metris_core::{AiTool,HookBehavior,HOOK_RELAY_INSTANCE_HEADER,HOOK_EVENT_TYPE_HEADER};

/// 有界等待实际监听和工作线程状态，失败保留最后快照便于定位。
fn wait_status(service:&UsageService,predicate:impl Fn(&crate::monitor::listener::HookRelayStatus)->bool)->crate::monitor::listener::HookRelayStatus {
    let deadline=Instant::now()+Duration::from_secs(3);
    loop {let status=service.monitor_status().unwrap();if predicate(&status) {return status}assert!(Instant::now()<deadline,"relay state: {status:?}");std::thread::sleep(Duration::from_millis(5));}
}

/// 测试真实 HTTP 边界；会话只含固定 fixture 标识，不携带本机 Agent 内容。
fn post(path:&std::path::Path,event:&str,valid_instance:bool)->String {
    let endpoint:serde_json::Value=serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let instance=endpoint["instanceId"].as_str().unwrap();let port=endpoint["port"].as_u64().unwrap() as u16;
    let body=format!("{{\"hook_event_name\":\"{event}\",\"session_id\":\"fixture-session\",\"turn_id\":\"fixture-turn\"}}");
    let identity=if valid_instance {instance}else{"00000000-0000-4000-8000-000000000000"};
    let request=format!("POST /api/hooks/{instance}/codex HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\n{HOOK_RELAY_INSTANCE_HEADER}: {identity}\r\n{HOOK_EVENT_TYPE_HEADER}: {event}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
    let mut stream=TcpStream::connect_timeout(&([127,0,0,1],port).into(),Duration::from_secs(1)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
    stream.write_all(request.as_bytes()).unwrap();let mut response=String::new();stream.read_to_string(&mut response).unwrap();response
}

/// 晚启动消费已加载配置，事件推进、禁用释放、重新启用与关闭均走生产服务。
#[test]
fn monitor_tcp_uses_committed_selection_and_releases_endpoint_on_finish() {
    let root=tempfile::tempdir().unwrap();let path=root.path().join("cache/loki-metris-hook-relay.json");
    let service=UsageService::in_directory(root.path().join("data")).unwrap();
    service.monitor_settings().unwrap().blocking_recv().unwrap().unwrap();
    service.start_monitoring_at(path.clone()).unwrap();wait_status(&service,|status|status.listening);
    assert!(post(&path,"SessionStart",true).starts_with("HTTP/1.1 202"));
    let started=wait_status(&service,|status|status.received_count==1);assert!(started.pet_states.iter().any(|state|state.tool==AiTool::Codex&&state.behavior==Some(HookBehavior::Idle)));
    assert!(post(&path,"UserPromptSubmit",true).starts_with("HTTP/1.1 202"));
    let running=wait_status(&service,|status|status.received_count==2);assert!(running.pet_states.iter().any(|state|state.behavior==Some(HookBehavior::Running)));
    service.save_enabled_ai_tools(vec![AiTool::Cursor]).unwrap().blocking_recv().unwrap().unwrap();
    assert!(service.monitor_status().unwrap().pet_states.iter().all(|state|state.behavior.is_none()));
    assert!(post(&path,"SessionStart",true).starts_with("HTTP/1.1 202"));assert_eq!(service.monitor_status().unwrap().received_count,2);
    let rejected=post(&path,"SessionStart",false);assert!(rejected.starts_with("HTTP/1.1 404"),"{rejected}");wait_status(&service,|status|status.failed_count==1);
    service.save_enabled_ai_tools(vec![AiTool::Codex]).unwrap().blocking_recv().unwrap().unwrap();
    assert!(post(&path,"SessionStart",true).starts_with("HTTP/1.1 202"));wait_status(&service,|status|status.received_count==3);
    service.finish();assert!(!path.exists());assert!(path.with_extension("lock").exists());assert!(!service.monitor_status().unwrap().listening);
    assert!(service.save_enabled_ai_tools(vec![AiTool::Codex]).is_err());
}

/// 相同端点路径只允许一个发布者，失败实例关闭不能删除真实运行者的描述。
#[test]
fn monitor_endpoint_lease_prevents_overlapping_publishers() {
    let root=tempfile::tempdir().unwrap();let path=root.path().join("cache/endpoint.json");
    let first=UsageService::in_directory(root.path().join("one")).unwrap();first.start_monitoring_at(path.clone()).unwrap();wait_status(&first,|status|status.listening);
    let expected=std::fs::read(&path).unwrap();
    let second=UsageService::in_directory(root.path().join("two")).unwrap();second.start_monitoring_at(path.clone()).unwrap();
    wait_status(&second,|status|status.last_error.is_some());assert_eq!(std::fs::read(&path).unwrap(),expected);
    second.finish();assert_eq!(std::fs::read(&path).unwrap(),expected);assert!(first.monitor_status().unwrap().listening);
    first.finish();assert!(!path.exists());
}
