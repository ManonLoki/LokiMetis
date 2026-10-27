//! 无 sqlite3 可执行文件时使用项目已有数据库驱动；取消仍归同一后台刷新 owner。

use std::path::Path;
use loki_metris_core::{QuotaFailure, ScanCancellation};
use tokio::sync::watch;
pub(super) use loki_metris_host::cursor_state::CursorStateQuery;
use super::transport::CurlLimits;

/// 将后台取消信号传到只读 SQLite，并等待当前查询关闭，不读取其它键或发起网络请求。
pub(super) async fn read(database: &Path, query: CursorStateQuery, limits: CurlLimits, cancel: &mut watch::Receiver<bool>) -> Result<String, QuotaFailure> {
    if *cancel.borrow() { return Err(QuotaFailure::Transport); }
    let cancellation = ScanCancellation::new();
    let work = loki_metris_host::cursor_state::read(database, query, cancellation.clone(), limits.timeout);
    tokio::pin!(work);
    tokio::select! {
        biased;
        _ = cancelled(cancel) => { cancellation.cancel(); let _ = work.await; Err(QuotaFailure::Transport) },
        result = &mut work => result,
    }
}

/// 发送端关闭与显式取消同样结束当前只读操作。
async fn cancelled(cancel: &mut watch::Receiver<bool>) {
    loop {
        if *cancel.borrow_and_update() || cancel.changed().await.is_err() { return; }
    }
}
