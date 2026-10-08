//! 端侧拥有唯一日志订阅器；结构化事件经有界后台队列同时写标准错误和本地滚动文件。

use std::{fs, io::{self, Write}, path::{Path, PathBuf}};
use tracing_appender::{non_blocking::{NonBlockingBuilder, WorkerGuard}, rolling::{RollingFileAppender, Rotation}};
use tracing_subscriber::{filter::Targets, fmt::format::FmtSpan, prelude::*};

/// 保留最多七份按日滚动文件；队列满时丢弃诊断，避免日志阻塞原生窗口线程。
const MAX_LOG_FILES: usize = 7;
/// 队列以事件行数设上限，日志调用方仍须限制单条字段大小且不得记录秘密或业务载荷。
const QUEUE_LINES: usize = 1024;

/// 两个本地输出共用同一后台 worker，文件失败时仍尝试标准错误且返回可观察错误。
struct LocalOutputs<W> {
    file: RollingFileAppender,
    stderr: W,
}

impl<W: Write> Write for LocalOutputs<W> {
    /// 完整写入同一行到两个输出，不允许短写使两边静默丢失字段。
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let file = self.file.write_all(bytes);
        let stderr = self.stderr.write_all(bytes);
        file.and(stderr).map(|()| bytes.len())
    }

    /// 正常退出时刷新两个输出；worker guard 由端侧退出状态持有至宿主工作收尾之后。
    fn flush(&mut self) -> io::Result<()> {
        let file = self.file.flush();
        let stderr = self.stderr.flush();
        file.and(stderr)
    }
}

/// 路径只取当前用户的平台标准日志目录；不回退项目 cwd 或临时共享目录。
fn directory() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let root = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let root = std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Logs"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let root = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")));
    root.filter(|root| root.is_absolute()).map(|root| root.join("app-@@KEBAB_ID@@").join("logs"))
}

/// 拒绝链接和非目录祖先，避免滚动清理穿过链接影响应用目录以外的文件。
fn prepare_directory(directory: &Path) -> Result<(), &'static str> {
    if !directory.is_absolute() { return Err("log_directory_not_absolute"); }
    for ancestor in directory.ancestors().collect::<Vec<_>>().into_iter().rev() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => return Err("log_directory_unsafe"),
            Ok(_) => {},
            Err(error) if error.kind() == io::ErrorKind::NotFound => {},
            Err(_) => return Err("log_directory_unavailable"),
        }
    }
    fs::create_dir_all(directory).map_err(|_| "log_directory_unavailable")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).map_err(|_| "log_directory_unavailable")?;
    }
    Ok(())
}

/// 建立可隔离测试的订阅器；只接收当前 GUI/core 的 INFO 以上事件，依赖诊断默认关闭。
fn subscriber<W: Write + Send + 'static>(directory: &Path, stderr: W)
    -> Result<(impl tracing::Subscriber + Send + Sync, WorkerGuard), &'static str>
{
    prepare_directory(directory)?;
    let file = RollingFileAppender::builder().rotation(Rotation::DAILY)
        .filename_prefix("application").filename_suffix("log").max_log_files(MAX_LOG_FILES)
        .build(directory).map_err(|_| "log_file_unavailable")?;
    let (writer, guard) = NonBlockingBuilder::default().buffered_lines_limit(QUEUE_LINES)
        .lossy(true).thread_name("application-log-writer").finish(LocalOutputs { file, stderr });
    let targets = Targets::new().with_target("@@PROJECT_ID@@_gui", tracing::Level::INFO)
        .with_target("@@PROJECT_ID@@_core", tracing::Level::INFO);
    let subscriber = tracing_subscriber::registry().with(targets).with(tracing_subscriber::fmt::layer()
        .with_ansi(false).with_target(true).with_level(true).with_span_events(FmtSpan::CLOSE).with_writer(writer));
    Ok((subscriber, guard))
}

/// 仅由主入口初始化一次；失败返回稳定类别，不把用户路径或底层错误写入日志。
pub fn init() -> Result<WorkerGuard, &'static str> {
    let directory = directory().ok_or("log_directory_unavailable")?;
    let (subscriber, guard) = subscriber(&directory, io::stderr())?;
    tracing::subscriber::set_global_default(subscriber).map_err(|_| "log_subscriber_conflict")?;
    Ok(guard)
}

#[cfg(test)]
#[path = "logging_test.rs"]
mod logging_test;
