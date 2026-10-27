//! 只读 Cursor 状态库的固定键，不依赖系统安装 sqlite3，不执行迁移或返回任意表内容。

use std::{path::Path, time::{Duration, Instant}};
use loki_metris_core::{QuotaFailure, ScanCancellation};
use sea_orm::sqlx::{self, Connection, ConnectOptions, SqliteConnection, sqlite::SqliteConnectOptions};

/// 已批准读取的 Cursor 状态键集合；调用方不能提供任意 SQL 或键名。
#[derive(Clone, Copy)]
pub enum CursorStateQuery {
    /// 当前访问令牌，最多一条、8 KiB。
    AccessToken,
    /// 两个兼容用量摘要键，合计最多64 KiB。
    UsageSummary,
}

impl CursorStateQuery {
    /// 固定 SQL 在跨出 SQLite 前截取超限哨兵字节，避免返回无界字段或行数。
    fn sql(self) -> &'static str {
        match self {
            Self::AccessToken => "SELECT substr(CAST(value AS BLOB),1,8193) FROM ItemTable WHERE key='cursorAuth/accessToken' LIMIT 1",
            Self::UsageSummary => "SELECT substr(CAST(value AS BLOB),1,65537) FROM ItemTable WHERE key IN ('cursorai/usageSummary','cursorAuth/usageSummary') LIMIT 2",
        }
    }

    /// 各查询沿用原外部读取的输出预算。
    fn max_bytes(self) -> usize { match self { Self::AccessToken => 8 * 1024, Self::UsageSummary => 64 * 1024 } }
}

/// 读取既存普通库文件；取消或预算耗尽时中断 SQLite，关闭连接后返回稳定错误。
/// 活动 WAL 使用正常只读锁语义，不使用会忽略变化的 immutable：https://sqlite.org/wal.html#read_only_databases
pub async fn read(database: &Path, query: CursorStateQuery, cancellation: ScanCancellation, timeout: Duration) -> Result<String, QuotaFailure> {
    let timeout = timeout.min(Duration::from_secs(8));
    if cancellation.is_cancelled() || timeout.is_zero() { return Err(QuotaFailure::Transport); }
    let deadline = Instant::now() + timeout;
    let open = async {
        let metadata = tokio::fs::symlink_metadata(database).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound { QuotaFailure::MissingCredentials } else { QuotaFailure::Transport }
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() { return Err(QuotaFailure::MissingCredentials); }
        let options = SqliteConnectOptions::new().filename(database).create_if_missing(false).read_only(true)
            .busy_timeout(timeout.min(Duration::from_millis(250)))
            .pragma("trusted_schema", "OFF").pragma("query_only", "ON").disable_statement_logging();
        SqliteConnection::connect_with(&options).await.map_err(|_| QuotaFailure::Transport)
    };
    let mut connection = tokio::time::timeout(timeout, open).await.map_err(|_| QuotaFailure::Transport)??;
    let stopped = cancellation.clone();
    let operation = async {
        connection.lock_handle().await.map_err(|_| QuotaFailure::Transport)?
            .set_progress_handler(1000, move || !stopped.is_cancelled() && Instant::now() < deadline);
        read_rows(&mut connection, query, &cancellation).await
    };
    let result = tokio::time::timeout(deadline.saturating_duration_since(Instant::now()), operation)
        .await.unwrap_or(Err(QuotaFailure::Transport));
    // 取消查询 future 后仍显式等待 worker 停止；进度回调与250ms锁预算约束收尾，不能再次超时丢弃关闭。
    connection.close().await.map_err(|_| QuotaFailure::Transport)?;
    if cancellation.is_cancelled() || Instant::now() >= deadline { return Err(QuotaFailure::Transport); }
    result
}

/// 只组装固定查询的有界 UTF-8 结果，保持 sqlite3 无表头输出的空值与换行语义。
async fn read_rows(connection: &mut SqliteConnection, query: CursorStateQuery, cancellation: &ScanCancellation) -> Result<String, QuotaFailure> {
    if cancellation.is_cancelled() { return Err(QuotaFailure::Transport); }
    let rows: Vec<Option<Vec<u8>>> = sqlx::query_scalar(query.sql()).fetch_all(connection).await.map_err(|_| QuotaFailure::Transport)?;
    let mut result = String::new();
    for row in rows {
        let bytes = row.unwrap_or_default();
        if result.len().saturating_add(bytes.len()).saturating_add(1) > query.max_bytes() { return Err(QuotaFailure::Transport); }
        result.push_str(std::str::from_utf8(&bytes).map_err(|_| QuotaFailure::Malformed)?);
        result.push('\n');
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
