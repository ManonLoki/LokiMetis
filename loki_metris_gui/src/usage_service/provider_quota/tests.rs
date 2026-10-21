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

fn layout(home: PathBuf) -> HomeLayout {
    HomeLayout { home, codex_home: None, grok_home: None, app_data: None }
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, text).unwrap();
}

fn cancel() -> (watch::Sender<bool>, watch::Receiver<bool>) {
    watch::channel(false)
}

fn range() -> TimeRange {
    TimeRange { begin: "2026-04-02 00:00:00".to_owned(), end: "2027-04-02 00:00:00".to_owned() }
}

fn expect_failure<T>(result: Result<T, QuotaFailure>, expected: QuotaFailure) {
    match result {
        Err(actual) if actual == expected => {}
        Err(actual) => panic!("expected {expected:?}, got {actual:?}"),
        Ok(_) => panic!("expected {expected:?}"),
    }
}

#[test]
fn curl_config_keeps_the_token_out_of_argv_and_rejects_other_hosts() {
    let request = HttpRequest {
        method: "GET",
        url: "https://chatgpt.com/backend-api/wham/usage".to_owned(),
        headers: vec![("Authorization", format!("Bearer {TOKEN}"))],
        body: None,
    };
    let exchange = transport::build_curl_exchange(&request, &CurlLimits::production()).unwrap();
    assert_eq!(exchange.args, ["--config", "-"]);
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
        assert!(matches!(phase, super::QuotaCardPhase::NoWindow { plan: Some(ref plan) } if plan == "enterprise"));
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

fn remote_windows(five_hour: u16, weekly: u16) -> String {
    format!(r#"{{"rate_limit":{{"primary_window":{{"used_percent":{five_hour},"limit_window_seconds":18000}},"secondary_window":{{"used_percent":{weekly},"limit_window_seconds":604800}}}}}}"#) + "\n__LM_HTTP_200"
}

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
