//! 使用真实宿主和 Entity 通知验证监控读取的刷新边界。

use super::*;
use gpui_kit::{Entity, TestAppContext};
use std::{cell::Cell, io::{Read, Write}, net::TcpStream, path::Path, rc::Rc, sync::Arc, time::{Duration, Instant}};

/// 先回收隔离宿主的监听和工作线程，再销毁临时目录。
struct Finish(Arc<crate::usage_service::UsageService>);
impl Drop for Finish {
    fn drop(&mut self) {self.0.finish();}
}

/// 有界等候真实宿主状态，不推进窗口轮询时钟或读取用户配置。
fn wait_status(service:&crate::usage_service::UsageService, predicate:impl Fn(&HookRelayStatus)->bool)->HookRelayStatus {
    let deadline=Instant::now()+Duration::from_secs(3);
    loop {
        let status=service.monitor_status().unwrap();
        if predicate(&status) {return status}
        assert!(Instant::now()<deadline,"relay state: {status:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// 每次生产读取后独立提交事件，避免同一更新内的通知合并掩盖重复刷新。
fn refresh(cx:&mut TestAppContext,shell:&Entity<Shell>,notifications:&Cell<usize>,expected:usize) {
    let before=Instant::now();
    shell.update(cx,|this,cx|this.refresh_monitor(true,cx));
    cx.run_until_parked();
    assert_eq!(notifications.get(),expected);
    shell.read_with(cx,|this,_|assert!(this.monitor.last_read.is_some_and(|read|read>=before)));
}

/// 真实 HTTP 拒绝只增加失败计数，不推进桌宠 revision。
fn rejected_request(path:&Path) {
    let endpoint:serde_json::Value=serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let port=endpoint["port"].as_u64().unwrap() as u16;
    let mut stream=TcpStream::connect_timeout(&([127,0,0,1],port).into(),Duration::from_secs(1)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
    stream.write_all(b"POST /invalid-fixture-route HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
    let mut response=String::new();stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 404"),"{response}");
}

/// 相同成功/失败不通知，计数不依赖 revision；失败保留指标，同快照恢复清除错误。
#[gpui_kit::test]
fn refresh_notifies_only_changed_status_or_error(cx:&mut TestAppContext) {
    let root=tempfile::tempdir().unwrap();
    let path=root.path().join("cache/relay.json");
    let service=crate::usage_service::UsageService::in_test_directory(root.path().join("data")).unwrap();
    let _finish=Finish(service.clone());
    let (_handle,shell,_writer)=crate::app::tests::open_shell(cx,1440.,900.);
    shell.update(cx,|this,_cx|this.page=crate::app::Page::Monitor);
    cx.run_until_parked();
    let notifications=Rc::new(Cell::new(0));
    let observed=notifications.clone();
    cx.update(|cx|cx.observe(&shell,move|_,_|observed.set(observed.get()+1)).detach());

    refresh(cx,&shell,&notifications,1);
    shell.read_with(cx,|this,_|assert_eq!(this.monitor.error,Some("monitor_service_unavailable")));
    refresh(cx,&shell,&notifications,1);
    shell.update(cx,|this,_cx|this.dashboard.service=Some(service.clone()));
    refresh(cx,&shell,&notifications,1);

    service.monitor_settings().unwrap().blocking_recv().unwrap().unwrap();
    service.start_test_monitoring_at(path.clone()).unwrap();
    let listening=wait_status(&service,|status|status.listening);
    refresh(cx,&shell,&notifications,2);
    shell.read_with(cx,|this,_|assert_eq!(this.monitor.status.as_ref(),Some(&listening)));
    shell.read_with(cx,|this,_|assert!(this.monitor.error.is_none()));
    refresh(cx,&shell,&notifications,2);

    rejected_request(&path);
    let rejected=wait_status(&service,|status|status.failed_count==1);
    assert_eq!(rejected.revision,listening.revision);
    refresh(cx,&shell,&notifications,3);
    shell.read_with(cx,|this,_|assert_eq!(this.monitor.status.as_ref(),Some(&rejected)));
    refresh(cx,&shell,&notifications,3);

    service.finish();
    let stopped=service.monitor_status().unwrap();
    assert!(!stopped.listening);
    refresh(cx,&shell,&notifications,4);
    shell.read_with(cx,|this,_|assert_eq!(this.monitor.status.as_ref(),Some(&stopped)));
    refresh(cx,&shell,&notifications,4);

    shell.update(cx,|this,_cx|this.dashboard.service=None);
    refresh(cx,&shell,&notifications,5);
    shell.read_with(cx,|this,_|assert_eq!(this.monitor.status.as_ref(),Some(&stopped)));
    shell.read_with(cx,|this,_|assert_eq!(this.monitor.error,Some("monitor_service_unavailable")));
    refresh(cx,&shell,&notifications,5);
    shell.update(cx,|this,_cx|this.dashboard.service=Some(service));
    refresh(cx,&shell,&notifications,6);
    shell.read_with(cx,|this,_|assert_eq!(this.monitor.status.as_ref(),Some(&stopped)));
    shell.read_with(cx,|this,_|assert!(this.monitor.error.is_none()));
    refresh(cx,&shell,&notifications,6);
}
