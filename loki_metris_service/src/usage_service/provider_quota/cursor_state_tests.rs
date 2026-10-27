//! Cursor 无 sqlite3.exe 时的服务接线回归；只操作合成数据库，不访问账号或网络。

use super::{credentials::{self, HomeLayout}, fetch, transport::CurlLimits, QuotaCardPhase};
use loki_metris_core::{QuotaFailure, QuotaProvider};
use sea_orm::sqlx::{self, Connection, ConnectOptions, SqliteConnection, sqlite::SqliteConnectOptions};
use tokio::sync::watch;

/// 在精确测试布局下建立 Cursor 状态库，只写入固定合成令牌和本地周额度。
async fn fixture(root: &std::path::Path) -> (HomeLayout, std::path::PathBuf) {
    let layout = HomeLayout { home: root.join("home"), app_data: Some(root.join("roaming")), codex_home: None, grok_home: None, claude_record: None };
    let database = credentials::cursor_database(&layout);
    std::fs::create_dir_all(database.parent().unwrap()).unwrap();
    let options = SqliteConnectOptions::new().filename(&database).create_if_missing(true).disable_statement_logging();
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    sqlx::query("CREATE TABLE ItemTable(key TEXT PRIMARY KEY,value TEXT)").execute(&mut connection).await.unwrap();
    let summary = r#"{"billingCycleStart":"2026-04-02T00:00:00Z","billingCycleEnd":"2026-04-09T00:00:00Z","individualUsage":{"plan":{"totalPercentUsed":11}}}"#;
    for (key, value) in [("cursorAuth/accessToken", "user_fixture::synthetic-token"), ("cursorAuth/usageSummary", summary)] {
        sqlx::query("INSERT INTO ItemTable VALUES(?,?)").bind(key).bind(value).execute(&mut connection).await.unwrap();
    }
    connection.close().await.unwrap();
    (layout, database)
}

/// 共享登录入口不再因 sqlite3.exe 缺失失败；读取成功后原库内容不变。
#[tokio::test]
async fn cursor_token_reads_without_external_sqlite() {
    let root = tempfile::tempdir().unwrap();
    let (layout, database) = fixture(root.path()).await;
    let before = std::fs::read(&database).unwrap();
    let (_sender, mut cancel) = watch::channel(false);
    let token = fetch::read_cursor_token(&layout, None, CurlLimits::production(), &mut cancel).await.unwrap();
    assert_eq!(token.expose(), "user_fixture::synthetic-token");
    assert_eq!(std::fs::read(database).unwrap(), before);
}

/// 本地周额度通过真实服务路径取得，无 curl 或 sqlite3 程序也不需要远程回退。
#[tokio::test]
async fn cursor_cached_quota_reads_without_any_external_program() {
    let root = tempfile::tempdir().unwrap();
    let (layout, database) = fixture(root.path()).await;
    let before = std::fs::read(&database).unwrap();
    let (_sender, mut cancel) = watch::channel(false);
    let range = fetch::TimeRange { begin: "2026-04-02 00:00:00".into(), end: "2026-04-09 00:00:00".into() };
    let phase = fetch::fetch_provider(QuotaProvider::Cursor, &layout, None, None, CurlLimits::production(), 0, &range, &mut cancel).await;
    let QuotaCardPhase::Ready(quota) = phase else { panic!("fixture weekly quota must be ready"); };
    assert_eq!(quota.weekly.unwrap().used_tenths, 110);
    assert_eq!(std::fs::read(database).unwrap(), before);
}

/// 已取消读取和缺失数据库保留原失败分类，不创建文件或回退读取其它登录位置。
#[tokio::test]
async fn embedded_cursor_read_respects_cancellation_and_missing_credentials() {
    let root = tempfile::tempdir().unwrap();
    let (layout, database) = fixture(root.path()).await;
    let (_sender, mut cancel) = watch::channel(true);
    assert!(matches!(fetch::read_cursor_token(&layout, None, CurlLimits::production(), &mut cancel).await, Err(QuotaFailure::Transport)));
    std::fs::remove_file(&database).unwrap();
    let (_sender, mut active) = watch::channel(false);
    assert!(matches!(fetch::read_cursor_token(&layout, None, CurlLimits::production(), &mut active).await, Err(QuotaFailure::MissingCredentials)));
    assert!(!database.exists());
}
