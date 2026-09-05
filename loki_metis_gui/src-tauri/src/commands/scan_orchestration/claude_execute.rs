//! Claude 本机扫描的异步编排：薄封装，实际骨架见 [`super::run::run_scan`]。

use std::path::PathBuf;

use loki_metis_core::{
    LocalScanOutput, LocalScanProgress, ScanCancellation, ScanKind, ScanStartOrigin,
};

use super::client::ClaudeClient;
use super::run::run_scan;

/// 用 `ClaudeClient` 特化通用扫描骨架，执行一次 Claude 本机扫描。
pub(super) async fn execute_claude_scan(
    app_data_dir: PathBuf,
    settings_app_data_dir: PathBuf,
    kind: ScanKind,
    origin: ScanStartOrigin,
    cancellation: ScanCancellation,
    on_progress: Box<dyn FnMut(LocalScanProgress) + Send>,
) -> Result<LocalScanOutput, String> {
    run_scan::<ClaudeClient>(
        app_data_dir,
        settings_app_data_dir,
        None,
        kind,
        origin,
        cancellation,
        on_progress,
    )
    .await
}
