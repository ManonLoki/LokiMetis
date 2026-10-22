//! 受控子进程覆盖账户读取；所有登录材料是合成夹具，不访问用户或网络。

use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::json;

/// 合成账户 JWT，不具备任何真实认证能力。
fn token(user: &str) -> String {
    format!("e30.{}.signature", URL_SAFE_NO_PAD.encode(json!({"sub":format!("auth0|user_{user}")}).to_string()))
}

/// Unix 使用标准库 Python 受控程序，记录页码并返回精确夹具。
#[cfg(unix)]
fn fixture(root: &Path) -> (HomeLayout, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let layout = HomeLayout { home: root.join("home"), codex_home: None, grok_home: None, app_data: None, claude_record: None };
    let database = credentials::cursor_database(&layout);
    std::fs::create_dir_all(database.parent().unwrap()).unwrap();
    std::fs::write(&database,b"unchanged source database").unwrap();
    std::fs::write(root.join("token.txt"),token("first")).unwrap();
    let program = root.join("fixture");
    std::fs::write(&program,r#"#!/usr/bin/env python3
import pathlib,sys,json,time,os
root=pathlib.Path(__file__).resolve().parent
args=sys.argv[1:]
stdin=sys.stdin.read()
with (root/'argv.txt').open('a') as f:f.write(' '.join(args)+'\n')
if '-readonly' in args:
    sys.stdout.write((root/'token.txt').read_text());raise SystemExit
config={}
for line in stdin.splitlines():
    if ' = ' in line:
        key,value=line.split(' = ',1)
        try:config[key]=json.loads(value)
        except Exception:config[key]=value
body=json.loads(config['data-binary'])
with (root/'pages.txt').open('a') as f:f.write(str(body['page'])+'\n')
(root/'last-config.txt').write_text(stdin)
mode=(root/'mode.txt').read_text() if (root/'mode.txt').exists() else ''
if mode=='sleep':
    (root/'sleep-pid.txt').write_text(str(os.getpid()))
    time.sleep(30)
if mode=='unauthorized':sys.stdout.write('{}\n__LM_HTTP_401');raise SystemExit
payload=(root/('page-'+str(body['page'])+'.json')).read_text()
sys.stdout.write(payload+'\n__LM_HTTP_200')
"#).unwrap();
    std::fs::set_permissions(&program,std::fs::Permissions::from_mode(0o700)).unwrap();
    (layout,program)
}

/// 在冻结范围内构造一条非敏感事件。
fn response(id: usize, at: i64, input: u64) -> String {
    json!({"totalUsageEventsCount":1,"usageEventsDisplay":[{"usageEventId":format!("event-{id}"),
        "timestamp":at.to_string(),"model":"model-a","tokenUsage":{"inputTokens":input,"outputTokens":0}}]}).to_string()
}

/// 实际参数向量不带令牌、只读库不变；内存节流、显式刷新与换账户分别验证。
#[cfg(unix)]
#[test]
fn cursor_usage_driver_cache_refresh_and_account_isolation() {
    let runtime=tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let root=tempfile::tempdir().unwrap();let (layout,program)=fixture(root.path());
        let now="2026-10-22T12:00:00Z".parse::<jiff::Timestamp>().unwrap().as_millisecond();
        std::fs::write(root.path().join("page-1.json"),response(1,now,12)).unwrap();
        let reader=Reader::default();let (_sender,mut cancel)=watch::channel(false);
        let interval=ScanIntervalMinutes::default();
        let first=reader.read(&layout,Some(&program),Some(&program),LocalUsageWindow::Today,false,interval,now,&TimeZone::UTC,&mut cancel).await.unwrap();
        assert_eq!(first.counts.input.tokens,12);
        std::fs::write(root.path().join("page-1.json"),response(1,now,99)).unwrap();
        let cached=reader.read(&layout,Some(&program),Some(&program),LocalUsageWindow::Today,false,interval,now,&TimeZone::UTC,&mut cancel).await.unwrap();
        assert_eq!(cached.counts.input.tokens,12);
        assert_eq!(std::fs::read_to_string(root.path().join("pages.txt")).unwrap().lines().count(),1);
        let forced=reader.read(&layout,Some(&program),Some(&program),LocalUsageWindow::Today,true,interval,now,&TimeZone::UTC,&mut cancel).await.unwrap();
        assert_eq!(forced.counts.input.tokens,99);
        std::fs::write(root.path().join("token.txt"),token("second")).unwrap();
        std::fs::write(root.path().join("page-1.json"),response(2,now,7)).unwrap();
        let switched=reader.read(&layout,Some(&program),Some(&program),LocalUsageWindow::Today,false,interval,now,&TimeZone::UTC,&mut cancel).await.unwrap();
        assert_eq!(switched.counts.input.tokens,7);
        let args=std::fs::read_to_string(root.path().join("argv.txt")).unwrap();assert!(!args.contains(&token("first")));assert!(!args.contains(&token("second")));
        assert!(args.contains("-readonly"));
        assert_eq!(std::fs::read(credentials::cursor_database(&layout)).unwrap(),b"unchanged source database");
        let config=std::fs::read_to_string(root.path().join("last-config.txt")).unwrap();
        assert!(config.contains("WorkosCursorSessionToken=user_second%3A%3A"));
        assert!(config.contains("get-filtered-usage-events"));assert!(config.contains("startDate"));
        std::fs::write(root.path().join("token.txt"),token("third")).unwrap();
        std::fs::write(root.path().join("mode.txt"),"unauthorized").unwrap();
        assert_eq!(reader.read(&layout,Some(&program),Some(&program),LocalUsageWindow::Today,false,interval,now,&TimeZone::UTC,&mut cancel).await,Err(Error::Unauthorized));
        std::fs::write(root.path().join("mode.txt"),"").unwrap();
        assert_eq!(reader.read(&layout,Some(&program),Some(&program),LocalUsageWindow::Today,false,interval,now,&TimeZone::UTC,&mut cancel).await,Err(Error::Unauthorized),"failed account query must not expose another account cache");
        let recovered=reader.read(&layout,Some(&program),Some(&program),LocalUsageWindow::Today,true,interval,now,&TimeZone::UTC,&mut cancel).await.unwrap();
        assert_eq!(recovered.counts.input.tokens,7);
        std::fs::write(root.path().join("token.txt"),"").unwrap();
        assert_eq!(reader.read(&layout,Some(&program),Some(&program),LocalUsageWindow::Today,false,interval,now,&TimeZone::UTC,&mut cancel).await,Err(Error::Credentials));
    });
}

/// 空窗、成功和失败遵循配置分钟数；十秒页面重读不出站，到期后自动恢复。
#[cfg(unix)]
#[test]
fn cursor_usage_driver_empty_window_refreshes_and_recovers_automatically() {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        for minutes in [1, 7] {
            let root = tempfile::tempdir().unwrap();
            let (layout, program) = fixture(root.path());
            let now = "2026-10-22T12:00:00Z".parse::<jiff::Timestamp>().unwrap().as_millisecond();
            let reader = Reader::default();
            let interval = ScanIntervalMinutes::new(minutes).unwrap();
            let (_sender, mut cancel) = watch::channel(false);
            std::fs::write(root.path().join("page-1.json"), "{}").unwrap();
            let empty = reader.read(&layout, Some(&program), Some(&program), LocalUsageWindow::Today,
                false, interval, now, &TimeZone::UTC, &mut cancel).await.unwrap();
            assert_eq!(empty.counts.events, 0);
            std::fs::write(root.path().join("page-1.json"), response(1, now, 42)).unwrap();
            let cached = reader.read(&layout, Some(&program), Some(&program), LocalUsageWindow::Today,
                false, interval, now, &TimeZone::UTC, &mut cancel).await.unwrap();
            assert_eq!(cached.counts.events, 0);
            reader.cache.lock().unwrap().1[0].at = std::time::Instant::now() - Duration::from_secs(10);
            let displayed = reader.read(&layout, Some(&program), Some(&program), LocalUsageWindow::Today,
                false, interval, now, &TimeZone::UTC, &mut cancel).await.unwrap();
            assert_eq!(displayed.counts.events, 0);
            assert_eq!(std::fs::read_to_string(root.path().join("pages.txt")).unwrap().lines().count(), 1);
            // 只推进单调时刻，不进行真实等待；下一次普通页面查询才读取新的云端事实。
            reader.cache.lock().unwrap().1[0].at = std::time::Instant::now() - interval.duration();
            let fresh = reader.read(&layout, Some(&program), Some(&program), LocalUsageWindow::Today,
                false, interval, now, &TimeZone::UTC, &mut cancel).await.unwrap();
            assert_eq!(fresh.counts.input.tokens, 42);
            std::fs::write(root.path().join("mode.txt"), "unauthorized").unwrap();
            reader.cache.lock().unwrap().1[0].at = std::time::Instant::now() - interval.duration();
            assert_eq!(reader.read(&layout, Some(&program), Some(&program), LocalUsageWindow::Today,
                false, interval, now, &TimeZone::UTC, &mut cancel).await, Err(Error::Unauthorized));
            std::fs::write(root.path().join("mode.txt"), "").unwrap();
            let failed = reader.read(&layout, Some(&program), Some(&program), LocalUsageWindow::Today,
                false, interval, now, &TimeZone::UTC, &mut cancel).await;
            assert_eq!(failed, Err(Error::Unauthorized));
            reader.cache.lock().unwrap().1[0].at = std::time::Instant::now() - interval.duration();
            let recovered = reader.read(&layout, Some(&program), Some(&program), LocalUsageWindow::Today,
                false, interval, now, &TimeZone::UTC, &mut cancel).await.unwrap();
            assert_eq!(recovered.counts.input.tokens, 42);
            assert_eq!(std::fs::read_to_string(root.path().join("pages.txt")).unwrap().lines().count(), 4);
        }
    });
}

/// 缩短设置不提前打断旧等待；完成下一轮后才按新间隔获取后续事件。
#[cfg(unix)]
#[test]
fn cursor_usage_driver_interval_change_preserves_started_wait() {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let root = tempfile::tempdir().unwrap();
        let (layout, program) = fixture(root.path());
        let reader = Reader::default();
        let (_sender, mut cancel) = watch::channel(false);
        let now = "2026-10-22T12:00:00Z".parse::<jiff::Timestamp>().unwrap().as_millisecond();
        let old = ScanIntervalMinutes::new(7).unwrap();
        let new = ScanIntervalMinutes::new(1).unwrap();
        std::fs::write(root.path().join("page-1.json"), response(1, now, 12)).unwrap();
        reader.read(&layout, Some(&program), Some(&program), LocalUsageWindow::Today,
            false, old, now, &TimeZone::UTC, &mut cancel).await.unwrap();
        std::fs::write(root.path().join("page-1.json"), response(1, now, 99)).unwrap();
        reader.cache.lock().unwrap().1[0].at = std::time::Instant::now() - Duration::from_secs(120);
        let waiting = reader.read(&layout, Some(&program), Some(&program), LocalUsageWindow::Today,
            false, new, now, &TimeZone::UTC, &mut cancel).await.unwrap();
        assert_eq!(waiting.counts.input.tokens, 12);
        assert_eq!(std::fs::read_to_string(root.path().join("pages.txt")).unwrap().lines().count(), 1);
        reader.cache.lock().unwrap().1[0].at = std::time::Instant::now() - old.duration();
        let updated = reader.read(&layout, Some(&program), Some(&program), LocalUsageWindow::Today,
            false, new, now, &TimeZone::UTC, &mut cancel).await.unwrap();
        assert_eq!(updated.counts.input.tokens, 99);
        std::fs::write(root.path().join("page-1.json"), response(1, now, 7)).unwrap();
        reader.cache.lock().unwrap().1[0].at = std::time::Instant::now() - new.duration();
        let next = reader.read(&layout, Some(&program), Some(&program), LocalUsageWindow::Today,
            false, new, now, &TimeZone::UTC, &mut cancel).await.unwrap();
        assert_eq!(next.counts.input.tokens, 7);
    });
}

/// 损坏的权威配置使账户查询失败，不回退到写死间隔或触碰真实登录材料。
#[test]
fn cursor_usage_query_rejects_unreadable_interval_settings() {
    use crate::usage_service::{UsageService, UsageQuery};
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("data");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("usage-settings.json"), b"invalid json").unwrap();
    let service = UsageService::in_directory(directory).unwrap();
    let reply = service.query(UsageQuery::CursorAccount(LocalUsageWindow::Today, false)).unwrap();
    let result = service.runtime.lock().unwrap().as_ref().unwrap().block_on(reply).unwrap();
    assert!(matches!(result, Err("cursor_usage_transport")));
    service.finish();
}

/// 分页中断不能发布部分结果；认证错误、取消、超时均返回稳定失败。
#[cfg(unix)]
#[test]
fn cursor_usage_driver_rejects_partial_and_reaps_cancelled_process() {
    let runtime=tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let root=tempfile::tempdir().unwrap();let (_,program)=fixture(root.path());
        let now=1_792_627_200_000_i64;let range=CursorUsageRange{start_ms:now,end_ms:now+1};
        let secret=credentials::Secret::new(token("first")).unwrap();
        let (sender,mut cancel)=watch::channel(false);
        let limits=CurlLimits{timeout:Duration::from_secs(2),max_body:1024*1024};
        let rows:Vec<_>=(0..100).map(|id|json!({"usageEventId":format!("{id}"),"timestamp":now.to_string()})).collect();
        std::fs::write(root.path().join("page-1.json"),json!({"totalUsageEventsCount":101,"usageEventsDisplay":rows}).to_string()).unwrap();
        std::fs::write(root.path().join("page-2.json"),json!({"totalUsageEventsCount":101,"usageEventsDisplay":[]}).to_string()).unwrap();
        assert_eq!(read_pages(&secret,Some(&program),range,now,&TimeZone::UTC,limits,&mut cancel).await,Err(Error::Incomplete));
        std::fs::write(root.path().join("mode.txt"),"unauthorized").unwrap();
        assert_eq!(read_pages(&secret,Some(&program),range,now,&TimeZone::UTC,limits,&mut cancel).await,Err(Error::Unauthorized));
        std::fs::write(root.path().join("mode.txt"),"sleep").unwrap();
        let short=CurlLimits{timeout:Duration::from_millis(100),..limits};
        assert_eq!(read_pages(&secret,Some(&program),range,now,&TimeZone::UTC,short,&mut cancel).await,Err(Error::Transport));
        let pid=std::fs::read_to_string(root.path().join("sleep-pid.txt")).unwrap();
        assert!(!std::process::Command::new("/bin/kill").args(["-0",pid.trim()]).stderr(std::process::Stdio::null()).status().unwrap().success());
        std::fs::remove_file(root.path().join("sleep-pid.txt")).unwrap();
        let notify=async {
            tokio::time::timeout(Duration::from_secs(2),async {
                while !root.path().join("sleep-pid.txt").exists() {tokio::time::sleep(Duration::from_millis(5)).await;}
            }).await.unwrap();
            sender.send(true).unwrap();
        };
        let zone=TimeZone::UTC;
        let (result,())=tokio::join!(read_pages(&secret,Some(&program),range,now,&zone,limits,&mut cancel),notify);
        assert_eq!(result,Err(Error::Transport));
        let pid=std::fs::read_to_string(root.path().join("sleep-pid.txt")).unwrap();
        assert!(!std::process::Command::new("/bin/kill").args(["-0",pid.trim()]).stderr(std::process::Stdio::null()).status().unwrap().success());
        sender.send(true).unwrap();
        assert_eq!(read_pages(&secret,Some(&program),range,now,&TimeZone::UTC,limits,&mut cancel).await,Err(Error::Cancelled));
        assert_eq!(read_pages(&secret,None,range,now,&TimeZone::UTC,limits,&mut watch::channel(false).1).await,Err(Error::Transport));
    });
}
