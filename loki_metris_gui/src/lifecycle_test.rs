//! 通过真正 GPUI shutdown 钩子验证最终偏好和日志刷新，不依赖 run 返回。

use super::*;
use crate::preferences::{Preferences, ThemeChoice, load_from};
use gpui_kit::TestAppContext;
use std::{io::{self, Write}, sync::{Arc, Mutex, atomic::{AtomicU64, Ordering}}};

/// 可观察的日志输出保留后台 worker 写入的原始事件字节。
#[derive(Clone, Default)]
struct CapturedLog(Arc<Mutex<Vec<u8>>>);

impl Write for CapturedLog {
    /// 同一日志 worker 在内存中追加完整字节，供退出完成后的断言使用。
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().expect("测试日志锁正常").extend_from_slice(bytes);
        Ok(bytes.len())
    }

    /// 内存写入立即可见；真正队列刷新由 WorkerGuard 完成。
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

/// 原生退出钩子保存最后偏好并刷新 stopped 事件，run 返回回退不会重复执行。
#[gpui_kit::test]
fn native_shutdown_flushes_preferences_and_final_log_exactly_once(cx: &mut TestAppContext) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::fs::canonicalize(std::env::temp_dir()).expect("读取测试临时目录");
    let directory = root.join(format!("gpui-lifecycle-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir(&directory).expect("创建测试隔离目录");
    let file = directory.join("preferences.txt");
    let (_, writer) = load_from(Some(file.clone()));
    let captured = CapturedLog::default();
    let (log_writer, guard) = tracing_appender::non_blocking(captured.clone());
    let subscriber = tracing_subscriber::fmt().with_ansi(false).without_time().with_writer(log_writer).finish();
    let shutdown = Shutdown::new(writer.clone(), guard);
    writer.save(&Preferences { theme: ThemeChoice::Dark, collapsed: true, ..Preferences::default() });
    tracing::subscriber::with_default(subscriber, || {
        cx.update(|cx| install(cx, shutdown.clone()));
        // TestAppContext 调用的正是原生 App::shutdown，不返回 main 也必须收尾。
        cx.quit();
        assert!(std::fs::read_to_string(&file).expect("退出钩子已保存偏好").contains("theme=dark\n"));
        shutdown.borrow_mut().finish();
    });
    let text = String::from_utf8(captured.0.lock().expect("读取退出后的日志").clone()).expect("日志 UTF-8 正常");
    assert_eq!(text.matches("application_stopped").count(), 1, "退出事件必须实际刷新且只出现一次");
    assert!(shutdown.borrow().log_guard.is_none());
    std::fs::remove_dir_all(directory).expect("回收测试隔离目录");
}
