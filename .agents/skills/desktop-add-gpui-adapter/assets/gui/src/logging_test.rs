//! 真正落盘后检查格式、双输出、退出刷新、依赖过滤和受限滚动清理。

use super::*;
use std::sync::{Arc, Mutex, atomic::{AtomicU64, Ordering}};

/// 同一进程并发测试即使读到相同系统时钟，也必须拥有不同临时目录。
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

/// 测试标准错误替身只收集字节，不替换实际滚动文件输出。
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    /// 按原始字节收集完整输出，供文件和标准错误一致性比较。
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    /// 内存收集器没有额外缓冲，刷新无需修改状态。
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

/// 为每个测试建立真实且独立的目录，避免污染用户日志或依赖并发顺序。
fn temporary() -> PathBuf {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let directory = fs::canonicalize(std::env::temp_dir()).unwrap().join(format!("gpui-log-{}-{stamp}-{sequence}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    directory
}

/// Guard 退出后两边均包含结构化事件与 span，过滤外部依赖和 debug 事件且没有 ANSI。
#[test]
fn flushes_structured_events_and_spans_to_both_local_outputs() {
    let directory = temporary();
    let capture = Capture::default();
    let (subscriber, guard) = subscriber(&directory, capture.clone()).unwrap();
    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!(target: "@@PROJECT_ID@@_core", "use_case", operation = "status");
        let _entered = span.enter();
        tracing::info!(target: "@@PROJECT_ID@@_gui", event = "application_stopped", "正常退出");
        tracing::debug!(target: "@@PROJECT_ID@@_gui", "debug-excluded");
        tracing::info!(target: "external_dependency", "dependency-excluded");
    });
    drop(guard);
    let file = fs::read_dir(&directory).unwrap().next().unwrap().unwrap().path();
    let bytes = fs::read(file).unwrap();
    assert_eq!(bytes, *capture.0.lock().unwrap());
    let text = String::from_utf8(bytes).unwrap();
    for expected in ["INFO", "@@PROJECT_ID@@_gui", "application_stopped", "operation=\"status\"", "close", "正常退出"] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    assert!(text.as_bytes()[0].is_ascii_digit(), "timestamp missing");
    assert!(!text.contains('\u{1b}'));
    assert!(!text.contains("excluded"));
    fs::remove_dir_all(directory).unwrap();
}

/// 建立第八份匹配日志时只清理本应用日志，不删除同目录无关文件。
#[test]
fn rotation_retains_bounded_owned_files_only() {
    let directory = temporary();
    for day in 1..=9 { fs::write(directory.join(format!("application.2000-01-{day:02}.log")), b"old").unwrap(); }
    fs::write(directory.join("unrelated.txt"), b"keep").unwrap();
    let (_, guard) = subscriber(&directory, Capture::default()).unwrap();
    drop(guard);
    assert!(directory.join("unrelated.txt").exists());
    let logs = fs::read_dir(&directory).unwrap().filter(|entry| entry.as_ref().unwrap().file_name().to_string_lossy().ends_with(".log")).count();
    assert!(logs <= MAX_LOG_FILES);
    fs::remove_dir_all(directory).unwrap();
}

/// 不安全的日志目录必须显式失败，不能回退 stdout 或悄悄只写 stderr。
#[test]
fn rejects_relative_and_non_directory_log_locations() {
    assert!(matches!(subscriber(Path::new("relative"), Capture::default()), Err("log_directory_not_absolute")));
    let directory = temporary();
    fs::write(directory.join("file"), b"keep").unwrap();
    assert!(matches!(subscriber(&directory.join("file/logs"), Capture::default()), Err("log_directory_unsafe")));
    fs::remove_dir_all(directory).unwrap();
}

/// 链接日志目录不能触发外部目标的创建或滚动清理。
#[cfg(unix)]
#[test]
fn rejects_symlink_log_directory() {
    let directory = temporary();
    std::os::unix::fs::symlink(&directory, directory.join("linked")).unwrap();
    assert!(matches!(subscriber(&directory.join("linked/logs"), Capture::default()), Err("log_directory_unsafe")));
    assert!(!directory.join("logs").exists());
    fs::remove_dir_all(directory).unwrap();
}
