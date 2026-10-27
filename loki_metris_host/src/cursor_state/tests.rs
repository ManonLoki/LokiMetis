//! 全部状态库和令牌为临时合成夹具，覆盖无外部 sqlite3 的实际只读查询。

use super::*;
use sea_orm::sqlx::sqlite::SqliteJournalMode;
use std::path::PathBuf;

/// 用显式文件路径建立测试库，不改变真实 Cursor 数据或进程环境。
async fn fixture(path: &Path) -> SqliteConnection {
    let options = SqliteConnectOptions::new().filename(path).create_if_missing(true)
        .journal_mode(SqliteJournalMode::Delete).disable_statement_logging();
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    sqlx::query("CREATE TABLE ItemTable(key TEXT PRIMARY KEY, value BLOB)").execute(&mut connection).await.unwrap();
    connection
}

/// 向隔离状态表放入一份合成字段，允许字节夹具验证坏 UTF-8。
async fn put(connection: &mut SqliteConnection, key: &str, value: &[u8]) {
    sqlx::query("INSERT OR REPLACE INTO ItemTable VALUES(?,?)").bind(key).bind(value).execute(connection).await.unwrap();
}

/// 用正常生产预算执行固定键查询，不依赖系统 PATH。
async fn read_fixture(path: &Path, query: CursorStateQuery) -> Result<String, QuotaFailure> {
    read(path, query, ScanCancellation::new(), Duration::from_secs(2)).await
}

/// 中文、空格与URL保留字符路径不被解释成连接 URL，且只返回允许的三个键、原库逐字节保留。
#[tokio::test]
async fn reads_fixed_keys_from_native_path_without_rewriting_database() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("账户 # space");
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("state.vscdb");
    let mut writer = fixture(&path).await;
    put(&mut writer, "cursorAuth/accessToken", b"user_fixture::synthetic").await;
    put(&mut writer, "cursorai/usageSummary", b"{\"weekly\":1}").await;
    put(&mut writer, "cursorAuth/usageSummary", b"{\"weekly\":2}").await;
    put(&mut writer, "unrelated-secret", b"must-not-be-returned").await;
    writer.close().await.unwrap();
    let before = std::fs::read(&path).unwrap();
    assert_eq!(read_fixture(&path, CursorStateQuery::AccessToken).await.unwrap(), "user_fixture::synthetic\n");
    let usage = read_fixture(&path, CursorStateQuery::UsageSummary).await.unwrap();
    assert!(usage.contains("{\"weekly\":1}") && usage.contains("{\"weekly\":2}"));
    assert!(!usage.contains("must-not-be-returned") && !usage.contains("synthetic"));
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

/// 缺失文件不被创建；损坏库和缺失表以脱敏失败返回，不执行初始化或迁移。
#[tokio::test]
async fn missing_and_malformed_databases_are_never_created_or_repaired() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("missing.vscdb");
    assert!(matches!(read_fixture(&missing, CursorStateQuery::AccessToken).await, Err(QuotaFailure::MissingCredentials)));
    assert!(!missing.exists());
    let corrupt = root.path().join("corrupt.vscdb");
    std::fs::write(&corrupt, b"not sqlite").unwrap();
    assert!(matches!(read_fixture(&corrupt, CursorStateQuery::AccessToken).await, Err(QuotaFailure::Transport)));
    assert_eq!(std::fs::read(&corrupt).unwrap(), b"not sqlite");
    let path = root.path().join("empty.vscdb");
    let writer = SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&path).create_if_missing(true)).await.unwrap();
    writer.close().await.unwrap();
    assert!(matches!(read_fixture(&path, CursorStateQuery::AccessToken).await, Err(QuotaFailure::Transport)));
}

/// 超大字段在查询边界截断后拒绝，非法UTF-8不成为令牌，两个摘要键共用总预算。
#[tokio::test]
async fn output_limits_and_utf8_are_enforced_before_returning_values() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("state.vscdb");
    let mut writer = fixture(&path).await;
    put(&mut writer, "cursorAuth/accessToken", &vec![b'x'; 8191]).await;
    assert_eq!(read_fixture(&path, CursorStateQuery::AccessToken).await.unwrap().len(), 8192);
    put(&mut writer, "cursorAuth/accessToken", &vec![b'x'; 1024 * 1024]).await;
    assert!(matches!(read_fixture(&path, CursorStateQuery::AccessToken).await, Err(QuotaFailure::Transport)));
    put(&mut writer, "cursorAuth/accessToken", &[0xff]).await;
    assert!(matches!(read_fixture(&path, CursorStateQuery::AccessToken).await, Err(QuotaFailure::Malformed)));
    for key in ["cursorai/usageSummary", "cursorAuth/usageSummary"] { put(&mut writer, key, &vec![b'x'; 40 * 1024]).await; }
    assert!(matches!(read_fixture(&path, CursorStateQuery::UsageSummary).await, Err(QuotaFailure::Transport)));
    writer.close().await.unwrap();
}

/// 预取消与零预算优先失败，不因重试创建缺失文件。
#[tokio::test]
async fn cancellation_and_zero_budget_do_not_open_a_database() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("missing.vscdb");
    let cancellation = ScanCancellation::new(); cancellation.cancel();
    assert!(matches!(read(&path, CursorStateQuery::AccessToken, cancellation, Duration::from_secs(2)).await, Err(QuotaFailure::Transport)));
    assert!(matches!(read(&path, CursorStateQuery::AccessToken, ScanCancellation::new(), Duration::ZERO).await, Err(QuotaFailure::Transport)));
    assert!(!path.exists());
}

/// 恶意递归视图也被查询预算与在途取消中断，等待关闭后可移除库文件，无后台查询遗留。
#[tokio::test]
async fn expensive_queries_stop_on_deadline_or_in_flight_cancellation() {
    for cancelled in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("state.vscdb");
        let mut writer = fixture(&path).await;
        sqlx::query("DROP TABLE ItemTable").execute(&mut writer).await.unwrap();
        sqlx::query("CREATE VIEW ItemTable AS WITH RECURSIVE n(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM n) SELECT 'cursorAuth/accessToken' AS key, CAST(sum(x) AS TEXT) AS value FROM n").execute(&mut writer).await.unwrap();
        writer.close().await.unwrap();
        let signal = ScanCancellation::new();
        let stop = signal.clone();
        let work = read(&path, CursorStateQuery::AccessToken, signal, if cancelled { Duration::from_secs(2) } else { Duration::from_millis(80) });
        let trigger = async { if cancelled { tokio::time::sleep(Duration::from_millis(20)).await; stop.cancel(); } };
        let began = Instant::now();
        let (result, ()) = tokio::join!(work, trigger);
        assert!(matches!(result, Err(QuotaFailure::Transport)));
        assert!(began.elapsed() < Duration::from_secs(1));
        std::fs::remove_file(&path).unwrap();
    }
}

/// 活动WAL中的最新令牌可被读取，查询不改写主库或WAL，不把活动库伪装成immutable快照。
#[tokio::test]
async fn reads_current_wal_without_changing_database_or_wal_bytes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("state.vscdb");
    let mut writer = fixture(&path).await;
    sqlx::query("PRAGMA journal_mode=WAL").execute(&mut writer).await.unwrap();
    sqlx::query("PRAGMA wal_autocheckpoint=0").execute(&mut writer).await.unwrap();
    put(&mut writer, "cursorAuth/accessToken", b"current-wal-token").await;
    let mut wal_name = path.as_os_str().to_owned(); wal_name.push("-wal");
    let wal = PathBuf::from(wal_name);
    let before = (std::fs::read(&path).unwrap(), std::fs::read(&wal).unwrap());
    assert_eq!(read_fixture(&path, CursorStateQuery::AccessToken).await.unwrap(), "current-wal-token\n");
    assert_eq!((std::fs::read(&path).unwrap(), std::fs::read(&wal).unwrap()), before);
    writer.close().await.unwrap();
}
