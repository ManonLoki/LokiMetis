//! Windows 后台进程使用真实隔离子进程验证控制台与失败边界，不访问账户或网络。

use super::*;

/// 仅由父测试显式启动，检查控制台子系统程序确实没有窗口或附着控制台。
#[test]
#[ignore = "由后台进程回归作为受控子进程执行"]
fn quota_console_probe() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        /// 返回当前进程控制台的窗口句柄，无控制台时为空。
        fn GetConsoleWindow() -> *mut std::ffi::c_void;
    }
    // SAFETY: 该系统查询不接收指针、不保留状态，也不要求初始化。
    assert!(unsafe { GetConsoleWindow() }.is_null());
    println!("quota_probe_without_console");
}

/// 使用同一控制台测试二进制经生产驱动启动，确认隐藏窗口标志没有破坏输出捕获。
#[tokio::test]
async fn quota_child_has_no_console_and_keeps_captured_output() {
    let program = std::env::current_exe().unwrap();
    let args = ["--exact", "usage_service::provider_quota::transport::windows_tests::quota_console_probe", "--ignored", "--nocapture"].map(str::to_owned);
    let (_sender, mut cancel) = watch::channel(false);
    let output = run_command(&program, &args, None, CurlLimits { timeout: Duration::from_secs(5), max_body: 16 * 1024 }, &mut cancel).await.unwrap();
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("quota_probe_without_console"));
    assert!(text.contains("1 passed"));
}

/// 缺失程序、取消和过量输出仍返回同一脱敏传输失败，隐藏窗口不会跳过边界。
#[tokio::test]
async fn quota_child_preserves_failure_and_output_limits() {
    let root = tempfile::tempdir().unwrap();
    let (_sender, mut cancel) = watch::channel(false);
    assert!(matches!(run_command(&root.path().join("missing.exe"), &[], None, CurlLimits::production(), &mut cancel).await, Err(QuotaFailure::Transport)));
    let program = std::env::current_exe().unwrap();
    let args = ["--exact", "usage_service::provider_quota::transport::windows_tests::quota_console_probe", "--ignored", "--nocapture"].map(str::to_owned);
    assert!(matches!(run_command(&program, &args, None, CurlLimits { timeout: Duration::from_secs(5), max_body: 1 }, &mut cancel).await, Err(QuotaFailure::Transport)));
    let (_sender, mut cancelled) = watch::channel(true);
    assert!(matches!(run_command(&program, &args, None, CurlLimits::production(), &mut cancelled).await, Err(QuotaFailure::Transport)));
}
