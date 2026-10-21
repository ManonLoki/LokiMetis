//! 凭据读取与 curl 边界：夹具进程代替真实 curl，不访问用户目录或上游。

use std::{io::Write, path::{Path, PathBuf}, time::Duration};

use loki_metris_core::{QuotaFailure, QuotaProvider};
use tokio::sync::watch;

use super::{
    credentials::{self, HomeLayout},
    fetch::{self, TimeRange},
    transport::{self, CurlLimits, HttpRequest},
};

const TOKEN: &str = "test-codex-access-token";
const GROK_OLD: &str = "expired-grok-session-key";
const GROK_NEW: &str = "current-grok-session-key";
const CURSOR_TOKEN: &str = "cursor-test-token";

/// 终端只接收成功额度；全部失败后以空列表明确清除先前数据，不保留秘密或失败正文。
#[test]
fn for_terminal_keeps_weekly_only_and_clears_failed_cards() {
    let mut snapshot = super::QuotaBoardSnapshot::default();
    assert!(snapshot.for_terminal().is_none());
    let quota = loki_metris_core::parse_provider_quota(QuotaProvider::Codex,
        r#"{"rate_limits":{"primary":{"used_percent":97,"window_minutes":10080}}}"#).unwrap();
    snapshot.cards[0].phase = super::QuotaCardPhase::Ready(quota);
    snapshot.cards[1].phase = super::QuotaCardPhase::Failed(QuotaFailure::MissingCredentials);
    snapshot.fetched_at_unix = Some(123);
    let wire = snapshot.for_terminal().unwrap();
    wire.validate().unwrap();
    assert_eq!(wire.providers.len(), 1);
    assert!(wire.providers[0].five_hour.is_none());
    assert_eq!(wire.providers[0].weekly.unwrap().remaining_tenths(), 30);
    snapshot.cards[0].phase = super::QuotaCardPhase::Failed(QuotaFailure::Transport);
    assert!(snapshot.for_terminal().unwrap().providers.is_empty());
}

/// 为夹具构造隔离的凭据目录布局。
fn layout(home: PathBuf) -> HomeLayout {
    HomeLayout { home, codex_home: None, grok_home: None, app_data: None, claude_record: None }
}

/// 在夹具目录内创建父目录并写入指定文本。
fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, text).unwrap();
}

/// 创建当前测试拥有的刷新取消通道。
fn cancel() -> (watch::Sender<bool>, watch::Receiver<bool>) {
    watch::channel(false)
}

/// 提供与真实时钟无关的固定账单查询范围。
fn range() -> TimeRange {
    TimeRange { begin: "2026-04-02 00:00:00".to_owned(), end: "2027-04-02 00:00:00".to_owned() }
}

/// 核对稳定领域失败类型，避免暴露凭据正文。
fn expect_failure<T>(result: Result<T, QuotaFailure>, expected: QuotaFailure) {
    match result {
        Err(actual) if actual == expected => {}
        Err(actual) => panic!("expected {expected:?}, got {actual:?}"),
        Ok(_) => panic!("expected {expected:?}"),
    }
}

/// 凭据仅进入标准输入配置，拒绝非法主机及请求头注入。
#[test]
fn curl_config_keeps_the_token_out_of_argv_and_rejects_other_hosts() {
    let request = HttpRequest {
        method: "GET",
        url: "https://chatgpt.com/backend-api/wham/usage".to_owned(),
        headers: vec![("Authorization", format!("Bearer {TOKEN}"))],
        body: None,
    };
    let exchange = transport::build_curl_exchange(&request, &CurlLimits::production()).unwrap();
    assert_eq!(exchange.args, ["--disable", "--config", "-"]);
    assert!(exchange.args.iter().all(|arg| !arg.contains(TOKEN)));
    assert!(exchange.config.contains(&format!("Authorization: Bearer {TOKEN}")));
    let rejected = HttpRequest { method: "GET", url: "https://evil.example/usage".to_owned(), headers: vec![("Accept", "application/json".to_owned())], body: None };
    expect_failure(transport::build_curl_exchange(&rejected, &CurlLimits::production()), QuotaFailure::Transport);
    let insecure = HttpRequest { method: "GET", url: "http://chatgpt.com/backend-api/wham/usage".to_owned(), headers: vec![("Accept", "application/json".to_owned())], body: None };
    expect_failure(transport::build_curl_exchange(&insecure, &CurlLimits::production()), QuotaFailure::Transport);
    let injected = HttpRequest { method: "GET", url: "https://chatgpt.com/usage".to_owned(), headers: vec![("Authorization", "Bearer bad\nX-Evil: 1".to_owned())], body: None };
    expect_failure(transport::build_curl_exchange(&injected, &CurlLimits::production()), QuotaFailure::Transport);
    assert_eq!(transport::curl_quote(r#"{"a":1}"#).unwrap(), r#""{\"a\":1}""#);
}

/// 真实 curl 忽略隔离目录中会改变请求行为的默认配置，测试只查询版本而不联网。
#[cfg(unix)]
#[test]
fn curl_skips_default_config_before_reading_stdin_config() {
    let curl = transport::find_program("curl").expect("quota transport tests require curl");
    let root = tempfile::tempdir().unwrap();
    write(&root.path().join(".curlrc"), "not-a-valid-curl-option = true\n");
    let request = HttpRequest { method: "GET", url: "https://chatgpt.com/usage".to_owned(), headers: vec![], body: None };
    let exchange = transport::build_curl_exchange(&request, &CurlLimits::production()).unwrap();
    let mut child = std::process::Command::new(&curl).args(&exchange.args).arg("--version")
        .env("CURL_HOME", root.path()).stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(exchange.config.as_bytes()).unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success());
    assert!(!String::from_utf8_lossy(&result.stderr).contains("not-a-valid-curl-option"));
    let unsafe_result = std::process::Command::new(&curl).arg("--version").env("CURL_HOME", root.path()).output().unwrap();
    assert!(String::from_utf8_lossy(&unsafe_result.stderr).contains("not-a-valid-curl-option"));
}

/// SQLite 通过显式空启动文件运行固定只读查询，路径不会触发用户初始化命令。
#[test]
fn sqlite_query_explicitly_replaces_user_startup_file() {
    let args = transport::cursor_query_args(Path::new("state.vscdb"), transport::CURSOR_USAGE_SQL).unwrap();
    assert_eq!(&args[..3], ["-init", if cfg!(windows) { "NUL" } else { "/dev/null" }, "-readonly"]);
    assert_eq!(args.last().unwrap(), transport::CURSOR_USAGE_SQL);
    assert!(!args.last().unwrap().contains("LIMIT 1"));
}

/// 子进程不读取大块标准输入时，超时仍覆盖写入阶段并回收本次进程。
#[cfg(unix)]
#[test]
fn command_timeout_also_bounds_stdin_writes() {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let root = tempfile::tempdir().unwrap();
        let program = install_fixture(root.path());
        write(&root.path().join("mode.txt"), "ignore-stdin");
        let (_shutdown, mut cancel) = cancel();
        let start = std::time::Instant::now();
        let result = transport::run_command(&program, &[], Some("x".repeat(1024 * 1024)),
            CurlLimits { timeout: Duration::from_millis(200), max_body: 1024 }, &mut cancel).await;
        expect_failure(result, QuotaFailure::Transport);
        assert!(start.elapsed() < Duration::from_secs(3));
    });
}

/// 凭据只读且拒绝跟随源文件链接。
#[test]
fn credential_files_are_not_rewritten_and_symlinks_are_not_followed() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let auth = home.join(".codex").join("auth.json");
    let body = format!(r#"{{"tokens":{{"access_token":"{TOKEN}","account_id":"account-1"}}}}"#);
    write(&auth, &body);
    let before = std::fs::read(&auth).unwrap();
    let login = credentials::read_codex(&layout(home.clone())).unwrap();
    assert_eq!(login.account_id, "account-1");
    assert_eq!(std::fs::read(&auth).unwrap(), before);

    let linked = home.join(".grok");
    std::fs::create_dir_all(&linked).unwrap();
    let real = root.path().join("real-auth.json");
    write(&real, format!(r#"{{"key":"{GROK_NEW}"}}"#).as_str());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&real, linked.join("auth.json")).unwrap();
        expect_failure(credentials::read_grok(&layout(home), 1_700_000_000), QuotaFailure::MissingCredentials);
        assert_eq!(std::fs::read(&real).unwrap(), format!(r#"{{"key":"{GROK_NEW}"}}"#).into_bytes());
    }
}

/// Grok 选择有效会话，WorkBuddy 加密材料不被解密或改写。
#[test]
fn grok_prefers_the_newest_unexpired_key_and_workbuddy_locked_stays_locked() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let auth = home.join(".grok").join("auth.json");
    write(&auth, &format!(r#"{{"old":{{"key":"{GROK_OLD}","expires_at":100}},"new":{{"key":"{GROK_NEW}","expires_at":"2099-01-01T00:00:00Z"}},"email":"hidden@example.com"}}"#));
    let login = credentials::read_grok(&layout(home.clone()), 1_700_000_000).unwrap();
    assert!(login.token.expose() == GROK_NEW);

    let info = credentials::workbuddy_auth_files(&layout(home.clone()))[0].clone();
    write(&info, r#"{"auth":{"accessToken":{"$wbEncrypted":1,"envelope":"abc"},"domain":""}}"#);
    let keyblob = home.join(".workbuddy").join("keyblob");
    write(&keyblob, "do-not-read");
    let before = std::fs::read(&keyblob).unwrap();
    expect_failure(credentials::read_workbuddy(&layout(home)), QuotaFailure::CredentialLocked);
    assert_eq!(std::fs::read(&info).unwrap(), br#"{"auth":{"accessToken":{"$wbEncrypted":1,"envelope":"abc"},"domain":""}}"#);
    assert_eq!(std::fs::read(&keyblob).unwrap(), before);
}

/// 超限登录材料与包含多行的令牌输出被拒绝。
#[test]
fn oversized_auth_is_malformed_and_cursor_output_rejects_extra_lines() {
    let root = tempfile::tempdir().unwrap();
    let auth = root.path().join("home").join(".codex").join("auth.json");
    write(&auth, "x");
    let mut file = std::fs::OpenOptions::new().append(true).open(&auth).unwrap();
    file.write_all(&vec![b'a'; 256 * 1024]).unwrap();
    expect_failure(credentials::read_codex(&layout(root.path().join("home"))), QuotaFailure::Malformed);
    expect_failure(credentials::cursor_token_from_output(""), QuotaFailure::MissingCredentials);
    expect_failure(credentials::cursor_token_from_output("one\ntwo"), QuotaFailure::Malformed);
}

/// 真实夹具进程覆盖成功、失败退出、认证失败、缺失、超时与输出超限。
#[cfg(unix)]
#[test]
fn fixture_process_covers_success_status_missing_program_timeout_and_output_cap() {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let (_shutdown, mut cancel) = cancel();
        let root = tempfile::tempdir().unwrap();
        let program = install_fixture(root.path());
        let home = root.path().join("home");
        write(&home.join(".codex").join("auth.json"), &format!(r#"{{"tokens":{{"access_token":"{TOKEN}","account_id":"account-1"}}}}"#));
        let before = std::fs::read(home.join(".codex").join("auth.json")).unwrap();
        std::fs::write(root.path().join("mode.txt"), "status").unwrap();
        std::fs::write(root.path().join("body.txt"), r#"{"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":27,"limit_window_seconds":18000},"secondary_window":{"used_percent":4,"limit_window_seconds":604800}}}"#.to_owned() + "\n__LM_HTTP_200").unwrap();
        let phase = fetch::fetch_provider(QuotaProvider::Codex, &layout(home.clone()), Some(&program), None, CurlLimits { timeout: Duration::from_secs(2), max_body: 64 * 1024 }, 0, &range(), &mut cancel).await;
        let super::QuotaCardPhase::Ready(quota) = phase else { panic!("codex fixture should parse both windows, got {phase:?}"); };
        assert_eq!(quota.five_hour.unwrap().used_tenths, 270);
        assert_eq!(quota.weekly.unwrap().used_tenths, 40);
        let argv = std::fs::read_to_string(root.path().join("argv.txt")).unwrap();
        let stdin = std::fs::read_to_string(root.path().join("stdin.txt")).unwrap();
        assert!(!argv.contains(TOKEN));
        assert!(stdin.contains(TOKEN));
        assert!(stdin.contains("codex_cli_rs/0.98.0"));
        assert!(stdin.contains("originator: codex_cli_rs"));
        assert_eq!(std::fs::read(home.join(".codex").join("auth.json")).unwrap(), before);

        std::fs::write(root.path().join("mode.txt"), "nonzero").unwrap();
        let phase = fetch::fetch_provider(QuotaProvider::Codex, &layout(home.clone()), Some(&program), None, CurlLimits::production(), 0, &range(), &mut cancel).await;
        assert_eq!(phase, super::QuotaCardPhase::Failed(QuotaFailure::Transport));
        std::fs::write(root.path().join("mode.txt"), "status").unwrap();

        std::fs::write(root.path().join("body.txt"), "{}\n__LM_HTTP_401").unwrap();
        let phase = fetch::fetch_provider(QuotaProvider::Codex, &layout(home.clone()), Some(&program), None, CurlLimits::production(), 0, &range(), &mut cancel).await;
        assert_eq!(phase, super::QuotaCardPhase::Failed(QuotaFailure::Unauthorized));

        let missing = root.path().join("missing-curl");
        let phase = fetch::fetch_provider(QuotaProvider::Codex, &layout(home.clone()), Some(&missing), None, CurlLimits::production(), 0, &range(), &mut cancel).await;
        assert_eq!(phase, super::QuotaCardPhase::Failed(QuotaFailure::Transport));

        std::fs::write(root.path().join("mode.txt"), "huge").unwrap();
        let started = std::time::Instant::now();
        let phase = fetch::fetch_provider(QuotaProvider::Codex, &layout(home.clone()), Some(&program), None, CurlLimits { timeout: Duration::from_secs(2), max_body: 1024 }, 0, &range(), &mut cancel).await;
        assert_eq!(phase, super::QuotaCardPhase::Failed(QuotaFailure::Transport));
        assert!(started.elapsed() < Duration::from_secs(5));

        std::fs::write(root.path().join("mode.txt"), "sleep").unwrap();
        let started = std::time::Instant::now();
        let phase = fetch::fetch_provider(QuotaProvider::Codex, &layout(home), Some(&program), None, CurlLimits { timeout: Duration::from_millis(200), max_body: 1024 }, 0, &range(), &mut cancel).await;
        assert_eq!(phase, super::QuotaCardPhase::Failed(QuotaFailure::Transport));
        assert!(started.elapsed() < Duration::from_secs(3));
    });
}

/// Cursor 与 WorkBuddy 的请求身份仅经夹具标准输入传输。
#[cfg(unix)]
#[test]
fn cursor_cookie_and_workbuddy_identity_use_the_fixture_without_touching_secrets() {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let (_shutdown, mut cancel) = cancel();
        let root = tempfile::tempdir().unwrap();
        let program = install_fixture(root.path());
        let home = root.path().join("home");
        let database = credentials::cursor_database(&layout(home.clone()));
        write(&database, "not-a-real-database");
        let before = std::fs::read(&database).unwrap();
        std::fs::write(root.path().join("mode.txt"), "status").unwrap();
        std::fs::write(root.path().join("sqlite-out.txt"), CURSOR_TOKEN).unwrap();
        std::fs::write(root.path().join("body.txt"), r#"{"billingCycleStart":"2026-04-02T00:00:00Z","billingCycleEnd":"2026-05-02T00:00:00Z","membershipType":"enterprise","individualUsage":{"plan":{"totalPercentUsed":100,"used":1,"limit":1}}}"#.to_owned() + "\n__LM_HTTP_200").unwrap();
        let phase = fetch::fetch_provider(QuotaProvider::Cursor, &layout(home.clone()), Some(&program), Some(&program), CurlLimits { timeout: Duration::from_secs(2), max_body: 64 * 1024 }, 0, &range(), &mut cancel).await;
        // 月度账单周期进入月窗口，不再被当作“没有目标窗口”。
        assert!(matches!(phase, super::QuotaCardPhase::Ready(ref quota)
            if quota.plan.as_deref() == Some("enterprise") && quota.monthly.is_some_and(|window| window.used_tenths == 1000) && quota.weekly.is_none()));
        let stdin = std::fs::read_to_string(root.path().join("stdin.txt")).unwrap();
        let argv = std::fs::read_to_string(root.path().join("argv.txt")).unwrap();
        assert!(stdin.contains("WorkosCursorSessionToken=cursor-test-token"));
        assert!(stdin.contains("Mozilla/5.0"));
        assert!(!argv.contains(CURSOR_TOKEN));
        assert!(argv.contains("cursorAuth/accessToken"));
        assert_eq!(std::fs::read(&database).unwrap(), before);

        let info = credentials::workbuddy_auth_files(&layout(home.clone()))[0].clone();
        write(&info, r#"{"auth":{"accessToken":"workbuddy-test-token","domain":"","uid":"user-1","enterpriseId":"ent-1"}}"#);
        std::fs::write(root.path().join("body.txt"), "{}\n__LM_HTTP_200").unwrap();
        let phase = fetch::fetch_provider(QuotaProvider::WorkBuddy, &layout(home), Some(&program), None, CurlLimits { timeout: Duration::from_secs(2), max_body: 64 * 1024 }, 0, &range(), &mut cancel).await;
        assert!(matches!(phase, super::QuotaCardPhase::NoWindow { .. }));
        let stdin = std::fs::read_to_string(root.path().join("stdin.txt")).unwrap();
        assert!(stdin.contains("https://www.codebuddy.cn/v2/billing/meter/get-enterprise-user-usage"));
        assert!(stdin.contains("User-Agent: WorkBuddy/5.8.4"));
        assert!(stdin.contains("X-IDE-Type: WorkBuddy"));
        assert!(stdin.contains("X-Enterprise-Id: ent-1"));
        assert!(!stdin.contains("X-Conversation-"));
        assert_eq!(std::fs::read(&info).unwrap(), br#"{"auth":{"accessToken":"workbuddy-test-token","domain":"","uid":"user-1","enterpriseId":"ent-1"}}"#);
    });
}

/// 存在任一真实本地窗口时不启动远程查询。
#[cfg(unix)]
#[test]
fn local_windows_skip_the_remote_program() {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let (_shutdown, mut cancel) = cancel();
        let root = tempfile::tempdir().unwrap();
        let program = install_fixture(root.path());
        let home = root.path().join("home");
        let limits = CurlLimits { timeout: Duration::from_secs(2), max_body: 64 * 1024 };
        let codex = home.join(".codex").join("sessions").join("2026").join("10").join("21").join("rollout.jsonl");
        let codex_body = r#"{"type":"event_msg","payload":{"type":"token_count","rate_limits":{"plan_type":"plus","primary":{"used_percent":27,"window_minutes":300,"resets_at":1700000000},"secondary":{"used_percent":4,"window_minutes":10080}}}}"#;
        write(&codex, codex_body);
        let grok = home.join(".grok").join("logs").join("unified.jsonl");
        write(&grok, r#"{"msg":"billing: fetched credits config","ctx":{"config":{"creditUsagePercent":12.5,"currentPeriod":{"start":"2026-04-02T00:00:00Z","end":"2026-04-09T00:00:00Z"}}}}"#);
        let workbuddy = home.join(".workbuddy").join("usage-data").join("quota.json");
        write(&workbuddy, r#"{"Accounts":[{"used_percent":33,"limit_window_seconds":18000},{"used_percent":6,"limit_window_seconds":604800}]}"#);
        let before = [std::fs::read(&codex).unwrap(), std::fs::read(&grok).unwrap(), std::fs::read(&workbuddy).unwrap()];
        std::fs::write(root.path().join("mode.txt"), "status").unwrap();
        std::fs::write(root.path().join("body.txt"), r#"{"rate_limit":{"primary_window":{"used_percent":90,"limit_window_seconds":18000},"secondary_window":{"used_percent":80,"limit_window_seconds":604800}}}"#.to_owned() + "\n__LM_HTTP_200").unwrap();
        let layout = layout(home.clone());
        for (provider, five_hour, weekly) in [
            (QuotaProvider::Codex, Some(270_u16), Some(40)),
            (QuotaProvider::Grok, None, Some(125)),
            (QuotaProvider::WorkBuddy, Some(330), Some(60)),
        ] {
            let phase = fetch::fetch_provider(provider, &layout, Some(&program), Some(&program), limits, 0, &range(), &mut cancel).await;
            let super::QuotaCardPhase::Ready(quota) = phase else { panic!("{provider:?} should use the local window"); };
            assert_eq!(quota.five_hour.map(|window| window.used_tenths), five_hour);
            assert_eq!(quota.weekly.map(|window| window.used_tenths), weekly);
        }
        assert!(!root.path().join("argv.txt").exists(), "local windows must not start the remote program");
        assert_eq!(std::fs::read(&codex).unwrap(), before[0]);
        assert_eq!(std::fs::read(&grok).unwrap(), before[1]);
        assert_eq!(std::fs::read(&workbuddy).unwrap(), before[2]);

        let database = credentials::cursor_database(&layout);
        write(&database, "not-a-real-database");
        let database_before = std::fs::read(&database).unwrap();
        std::fs::write(root.path().join("sqlite-out.txt"), r#"{"billingCycleStart":"2026-04-02T00:00:00Z","billingCycleEnd":"2026-04-09T00:00:00Z","individualUsage":{"plan":{"totalPercentUsed":11}}}"#).unwrap();
        let phase = fetch::fetch_provider(QuotaProvider::Cursor, &layout, Some(&program), Some(&program), limits, 0, &range(), &mut cancel).await;
        let super::QuotaCardPhase::Ready(quota) = phase else { panic!("cursor local summary should win"); };
        assert_eq!(quota.weekly.unwrap().used_tenths, 110);
        assert!(quota.five_hour.is_none());
        let argv = std::fs::read_to_string(root.path().join("argv.txt")).unwrap();
        assert!(argv.contains("cursorai/usageSummary"));
        assert!(!argv.contains("--config"), "cursor local summary must not start curl");
        assert!(!argv.contains("https://cursor.com"));
        assert_eq!(std::fs::read(&database).unwrap(), database_before);
    });
}

/// 本机无目标窗口时才允许使用远程返回的真实窗口。
#[cfg(unix)]
#[test]
fn missing_or_monthly_local_records_use_the_remote_window() {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let (_shutdown, mut cancel) = cancel();
        let root = tempfile::tempdir().unwrap();
        let program = install_fixture(root.path());
        let home = root.path().join("home");
        let limits = CurlLimits { timeout: Duration::from_secs(2), max_body: 64 * 1024 };
        std::fs::write(root.path().join("mode.txt"), "status").unwrap();
        std::fs::write(root.path().join("body.txt"), remote_windows(15, 25)).unwrap();
        write(&home.join(".codex").join("auth.json"), &format!(r#"{{"tokens":{{"access_token":"{TOKEN}","account_id":"account-1"}}}}"#));
        let phase = fetch::fetch_provider(QuotaProvider::Codex, &layout(home.clone()), Some(&program), None, limits, 0, &range(), &mut cancel).await;
        let super::QuotaCardPhase::Ready(quota) = phase else { panic!("missing local record should use the remote windows"); };
        assert_eq!(quota.five_hour.unwrap().used_tenths, 150);
        assert_eq!(quota.weekly.unwrap().used_tenths, 250);
        let stdin = std::fs::read_to_string(root.path().join("stdin.txt")).unwrap();
        assert!(stdin.contains("https://chatgpt.com/backend-api/wham/usage"));
        assert!(stdin.contains("codex_cli_rs/0.98.0"));

        let monthly = home.join(".codex").join("sessions").join("rollout.jsonl");
        let monthly_body = r#"{"rate_limits":{"primary":{"used_percent":50,"window_minutes":43200}}}"#;
        write(&monthly, monthly_body);
        let before = std::fs::read(&monthly).unwrap();
        std::fs::write(root.path().join("body.txt"), remote_windows(12, 22)).unwrap();
        let phase = fetch::fetch_provider(QuotaProvider::Codex, &layout(home.clone()), Some(&program), None, limits, 0, &range(), &mut cancel).await;
        let super::QuotaCardPhase::Ready(quota) = phase else { panic!("monthly local record should fall through to the remote windows"); };
        assert_eq!(quota.five_hour.unwrap().used_tenths, 120);
        assert_eq!(quota.weekly.unwrap().used_tenths, 220);
        assert_ne!(quota.five_hour.unwrap().used_tenths, 500);
        assert_eq!(std::fs::read(&monthly).unwrap(), before);

        std::fs::write(root.path().join("body.txt"), "{}\n__LM_HTTP_200").unwrap();
        let phase = fetch::fetch_provider(QuotaProvider::Codex, &layout(home.clone()), Some(&program), None, limits, 0, &range(), &mut cancel).await;
        assert!(matches!(phase, super::QuotaCardPhase::NoWindow { .. }), "remote without target windows stays empty, got {phase:?}");

        let info = credentials::workbuddy_auth_files(&layout(home.clone()))[0].clone();
        write(&info, r#"{"auth":{"accessToken":{"$wbEncrypted":1,"envelope":"abc"},"domain":""}}"#);
        let keyblob = home.join(".workbuddy").join("keyblob");
        write(&keyblob, "do-not-read");
        let locked = std::fs::read(&keyblob).unwrap();
        std::fs::remove_file(root.path().join("argv.txt")).ok();
        std::fs::write(root.path().join("body.txt"), remote_windows(1, 2)).unwrap();
        let phase = fetch::fetch_provider(QuotaProvider::WorkBuddy, &layout(home), Some(&program), None, limits, 0, &range(), &mut cancel).await;
        assert_eq!(phase, super::QuotaCardPhase::Failed(QuotaFailure::CredentialLocked));
        assert_eq!(std::fs::read(&keyblob).unwrap(), locked);
        assert!(!root.path().join("argv.txt").exists(), "encrypted login must not be sent to the remote program");
    });
}

/// JSONL 尾部截到多字节残行时仍能读取后续完整额度。
#[cfg(unix)]
#[test]
fn jsonl_tail_cut_on_a_continuation_byte_still_uses_the_local_window() {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let (_shutdown, mut cancel) = cancel();
        let root = tempfile::tempdir().unwrap();
        let program = install_fixture(root.path());
        let home = root.path().join("home");
        let path = home.join(".codex").join("sessions").join("rollout.jsonl");
        let window = r#"{"payload":{"rate_limits":{"primary":{"used_percent":17,"window_minutes":300,"resets_at":1700000000}}}}"#;
        let tail_bytes = super::local::TAIL_BYTES as usize;
        let mut tail = Vec::with_capacity(tail_bytes);
        tail.push(0xA9);
        tail.push(b'\n');
        tail.extend(window.as_bytes());
        tail.push(b'\n');
        tail.resize(tail_bytes, b'a');
        let mut bytes = vec![0xC2];
        bytes.extend_from_slice(&tail);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, &bytes).unwrap();
        let cut = bytes.len() - tail_bytes;
        assert_eq!(bytes[cut] & 0b1100_0000, 0b1000_0000);
        let before = std::fs::read(&path).unwrap();
        std::fs::write(root.path().join("mode.txt"), "status").unwrap();
        std::fs::write(root.path().join("body.txt"), remote_windows(90, 80)).unwrap();
        let phase = fetch::fetch_provider(QuotaProvider::Codex, &layout(home), Some(&program), Some(&program), CurlLimits { timeout: Duration::from_secs(2), max_body: 64 * 1024 }, 0, &range(), &mut cancel).await;
        let super::QuotaCardPhase::Ready(quota) = phase else { panic!("split utf-8 tail should keep the local window, got {phase:?}"); };
        assert_eq!(quota.five_hour.unwrap().used_tenths, 170);
        assert!(quota.weekly.is_none());
        assert!(!root.path().join("argv.txt").exists(), "local window must not start the remote program");
        assert_eq!(std::fs::read(&path).unwrap(), before);
    });
}

/// 较新空记录不阻止继续查找较旧的真实本地窗口。
#[cfg(unix)]
#[test]
fn older_local_window_wins_when_newer_files_have_no_target_window() {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let (_shutdown, mut cancel) = cancel();
        let root = tempfile::tempdir().unwrap();
        let program = install_fixture(root.path());
        let home = root.path().join("home");
        let sessions = home.join(".codex").join("sessions");
        let monthly = r#"{"rate_limits":{"primary":{"used_percent":50,"window_minutes":43200}}}"#;
        for index in 0..8 {
            let path = sessions.join(format!("newer-{index}.jsonl"));
            write(&path, monthly);
            touch(&path, 1_700_000_000 + index);
        }
        let older = sessions.join("older.jsonl");
        let older_body = r#"{"payload":{"rate_limits":{"primary":{"used_percent":21,"window_minutes":300,"resets_at":1700000000}}}}"#;
        write(&older, older_body);
        touch(&older, 1_600_000_000);
        let before = std::fs::read(&older).unwrap();
        std::fs::write(root.path().join("mode.txt"), "status").unwrap();
        std::fs::write(root.path().join("body.txt"), remote_windows(90, 80)).unwrap();
        let phase = fetch::fetch_provider(QuotaProvider::Codex, &layout(home), Some(&program), Some(&program), CurlLimits { timeout: Duration::from_secs(2), max_body: 64 * 1024 }, 0, &range(), &mut cancel).await;
        let super::QuotaCardPhase::Ready(quota) = phase else { panic!("ninth newest local window should win, got {phase:?}"); };
        assert_eq!(quota.five_hour.unwrap().used_tenths, 210);
        assert!(quota.weekly.is_none());
        assert!(!root.path().join("argv.txt").exists(), "older local window must not start the remote program");
        assert_eq!(std::fs::read(&older).unwrap(), before);
    });
}

/// Cursor 同时读取两个缓存键，第二条真实窗口阻止远程回退。
#[cfg(unix)]
#[test]
fn cursor_reads_both_cached_summary_rows_before_remote() {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let (_shutdown, mut cancel) = cancel();
        let root = tempfile::tempdir().unwrap();
        let program = install_fixture(root.path());
        let home = root.path().join("home");
        let layout = layout(home);
        let database = credentials::cursor_database(&layout);
        write(&database, "not-a-real-database");
        let before = std::fs::read(&database).unwrap();
        let monthly = r#"{"billingCycleStart":"2026-04-02T00:00:00Z","billingCycleEnd":"2026-05-02T00:00:00Z","individualUsage":{"plan":{"totalPercentUsed":44}}}"#;
        let weekly = r#"{"billingCycleStart":"2026-04-02T00:00:00Z","billingCycleEnd":"2026-04-09T00:00:00Z","individualUsage":{"plan":{"totalPercentUsed":13}}}"#;
        std::fs::write(root.path().join("sqlite-out.txt"), format!("{monthly}\n{weekly}\n")).unwrap();
        std::fs::write(root.path().join("mode.txt"), "status").unwrap();
        std::fs::write(root.path().join("body.txt"), "{\"billingCycleStart\":\"2026-04-02T00:00:00Z\",\"billingCycleEnd\":\"2026-04-09T00:00:00Z\",\"individualUsage\":{\"plan\":{\"totalPercentUsed\":90}}}\n__LM_HTTP_200").unwrap();
        let phase = fetch::fetch_provider(QuotaProvider::Cursor, &layout, Some(&program), Some(&program), CurlLimits { timeout: Duration::from_secs(2), max_body: 64 * 1024 }, 0, &range(), &mut cancel).await;
        let super::QuotaCardPhase::Ready(quota) = phase else { panic!("second cached cursor row should win, got {phase:?}"); };
        assert_eq!(quota.weekly.unwrap().used_tenths, 130);
        assert!(quota.five_hour.is_none());
        let argv = std::fs::read_to_string(root.path().join("argv.txt")).unwrap();
        assert!(argv.contains("cursorai/usageSummary"));
        assert!(argv.contains("cursorAuth/usageSummary"));
        assert!(!argv.contains("LIMIT 1"));
        assert!(!argv.contains("--config"), "a local weekly row must not start curl");
        assert_eq!(std::fs::read(&database).unwrap(), before);
    });
}

/// 为用量测试写入夹具文件并设置可比较的修改时间。
#[cfg(unix)]
fn touch(path: &Path, secs: u64) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(std::time::UNIX_EPOCH + Duration::from_secs(secs)).unwrap();
}

/// 构造仅供进程夹具返回的固定远程额度正文。
fn remote_windows(five_hour: u16, weekly: u16) -> String {
    format!(r#"{{"rate_limit":{{"primary_window":{{"used_percent":{five_hour},"limit_window_seconds":18000}},"secondary_window":{{"used_percent":{weekly},"limit_window_seconds":604800}}}}}}"#) + "\n__LM_HTTP_200"
}

/// 在隔离目录安装测试脚本并授予当前宿主执行权限。
#[cfg(unix)]
fn install_fixture(dir: &Path) -> PathBuf {
    let path = dir.join("fixture.py");
    std::fs::write(&path, include_str!("fixture.py")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).unwrap();
    path
}

/// Claude 只读 statusLine 记录：过期窗口丢弃，全部过期或没有记录都不算有额度。
#[test]
fn claude_local_record_drops_expired_windows() {
    let root = tempfile::tempdir().unwrap();
    let record = root.path().join("claude-rate-limits.json");
    let mut home = layout(root.path().to_path_buf());
    assert!(super::local::claude_window(&home, 100).is_none());
    home.claude_record = Some(record.clone());
    assert!(super::local::claude_window(&home, 100).is_none());
    write(&record, r#"{"rate_limits":{"five_hour":{"used_percentage":40,"resets_at":150},"seven_day":{"used_percentage":12.5,"resets_at":900}}}"#);
    let quota = super::local::claude_window(&home, 100).unwrap();
    assert_eq!(quota.provider, QuotaProvider::Claude);
    assert_eq!(quota.five_hour.unwrap().used_tenths, 400);
    assert_eq!(quota.weekly.unwrap().used_tenths, 125);
    let later = super::local::claude_window(&home, 200).unwrap();
    assert!(later.five_hour.is_none() && later.weekly.is_some());
    assert!(super::local::claude_window(&home, 900).is_none());
}
