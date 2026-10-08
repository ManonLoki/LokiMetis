//! 使用隔离私有目录执行真实本地 IPC，不接触用户已有实例或登录项。
use super::*;
use std::sync::{atomic::AtomicU64, mpsc};

/// 不依赖额外 tempfile crate 的唯一隔离目录，测试结束完整删除。
struct Directory(PathBuf);
impl Directory {
    /// 临时根先规范化，避免 macOS /var 和 /tmp 链接影响安全检查。
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(std::env::temp_dir().canonicalize().unwrap().join(format!("instance-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))))
    }
}
impl Drop for Directory {
    /// 生命周期测试先释放实例，再删除测试文件。
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
}
/// 确认当前测试取得锁，不把第二实例结果伪装为 owner。
fn primary(directory: &Path, sender: SyncSender<()>) -> Instance {
    match Instance::acquire(directory, sender).unwrap() { Launch::Primary(owner) => owner, Launch::Activated => panic!("应取得独立测试锁") }
}
/// 第二启动只恢复旧窗口；关闭后可以重新取得同一锁和 socket。
#[test]
fn second_launch_restores_and_shutdown_releases_lock() {
    let directory = Directory::new();
    let (sender, receiver) = mpsc::sync_channel(1);
    let owner = primary(&directory.0, sender.clone());
    assert!(matches!(Instance::acquire(&directory.0, sender.clone()).unwrap(), Launch::Activated));
    receiver.recv_timeout(Duration::from_secs(1)).unwrap();
    drop(owner);
    drop(primary(&directory.0, sender));
}
/// 慢客户与满 UI 队列都不阻塞另一启动或停止线程。
#[test]
fn slow_client_and_full_queue_do_not_block_shutdown() {
    let directory = Directory::new();
    let (sender, _receiver) = mpsc::sync_channel(1);
    sender.try_send(()).unwrap();
    let owner = primary(&directory.0, sender.clone());
    let mut slow = Socket(connect(&directory.0).unwrap());
    slow.0.write_all(b"s").unwrap();
    assert!(matches!(Instance::acquire(&directory.0, sender).unwrap(), Launch::Activated));
    let started = Instant::now();
    drop(owner);
    assert!(started.elapsed() < Duration::from_secs(1));
}
/// 非法数据不能触发恢复，重复停止不留下监听器。
#[test]
fn malformed_payload_is_rejected_and_listener_can_restart() {
    let directory = Directory::new();
    let (sender, receiver) = mpsc::sync_channel(1);
    let owner = primary(&directory.0, sender);
    let mut client = Socket(connect(&directory.0).unwrap());
    client.0.write_all(b"nope\n").unwrap();
    thread::sleep(POLL * 10);
    assert!(matches!(receiver.try_recv(), Err(mpsc::TryRecvError::Empty)));
    drop(owner);
    for _ in 0..8 { let (sender, _receiver) = mpsc::sync_channel(1); drop(primary(&directory.0, sender)); }
}
/// 相对路径与已有链接不能改变测试目录以外的内容。
#[test]
fn relative_directory_is_rejected() {
    let (sender, _receiver) = mpsc::sync_channel(1);
    assert!(Instance::acquire(Path::new("relative"), sender).is_err());
}

/// 同时首次建立私有目录不能因 mkdir 竞争把第二实例判为失败。
#[test]
fn concurrent_first_launch_has_one_primary_and_one_activation() {
    let directory = Directory::new();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let (sender, receiver) = mpsc::sync_channel(1);
    let tasks: Vec<_> = (0..2).map(|_| {
        let path = directory.0.clone(); let sender = sender.clone(); let barrier = barrier.clone();
        thread::spawn(move || { barrier.wait(); Instance::acquire(&path, sender).unwrap() })
    }).collect();
    let results: Vec<_> = tasks.into_iter().map(|task| task.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|launch| matches!(launch, Launch::Primary(_))).count(), 1);
    assert_eq!(results.iter().filter(|launch| matches!(launch, Launch::Activated)).count(), 1);
    receiver.recv_timeout(Duration::from_secs(1)).unwrap();
    drop(results);
}

/// 已有符号链接与普通 socket 占位不能被删除或穿透。
#[cfg(unix)]
#[test]
fn rejects_symlink_directory_lock_and_non_socket_placeholder() {
    let directory = Directory::new();
    private_directory(&directory.0).unwrap();
    let (sender, _receiver) = mpsc::sync_channel(1);
    let lock = directory.0.join("instance.lock");
    let outside = directory.0.join("unrelated");
    fs::write(&outside, b"keep").unwrap();
    std::os::unix::fs::symlink(&outside, &lock).unwrap();
    assert!(Instance::acquire(&directory.0, sender.clone()).is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"keep");
    fs::remove_file(lock).unwrap();
    fs::write(directory.0.join("instance.sock"), b"keep").unwrap();
    assert!(Instance::acquire(&directory.0, sender.clone()).is_err());
    assert_eq!(fs::read(directory.0.join("instance.sock")).unwrap(), b"keep");
    let link = directory.0.join("link");
    std::os::unix::fs::symlink(&directory.0, &link).unwrap();
    assert!(Instance::acquire(&link, sender).is_err());
}
