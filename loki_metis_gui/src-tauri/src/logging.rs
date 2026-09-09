use tauri::Manager;
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{Builder, Rotation},
};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

const RETAINED_LOG_FILES: usize = 7;

#[allow(dead_code)]
/// 保持非阻塞日志工作线程存活直至应用退出。
pub(crate) struct LoggingGuard(WorkerGuard);

/// 安装按日轮换且保留数量有界的文件日志订阅器。
pub(crate) fn install_logging(app: &mut tauri::App) -> tauri::Result<()> {
    let log_directory = app.path().app_log_dir()?;
    std::fs::create_dir_all(&log_directory)?;
    let file_appender = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix("loki-metis")
        .max_log_files(RETAINED_LOG_FILES)
        .build(log_directory)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let (writer, guard) = tracing_appender::non_blocking(file_appender);
    let subscriber = fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_writer(writer);
    let _ = tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(subscriber)
        .try_init();
    app.manage(LoggingGuard(guard));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 滚动日志保留数量必须维持固定上限。
    #[test]
    fn rolling_logs_have_a_bounded_retention() {
        assert_eq!(RETAINED_LOG_FILES, 7);
    }
}
