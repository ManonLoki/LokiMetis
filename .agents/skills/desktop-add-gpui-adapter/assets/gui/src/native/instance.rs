//! 排他文件锁与本地 IPC；单个有界轮询线程只接受恢复窗口信号，退出时完整回收。

use interprocess::{ConnectWaitMode, local_socket::{Listener, ListenerNonblockingMode, ListenerOptions, Name, Stream, prelude::*}};
#[cfg(unix)]
use interprocess::local_socket::{ConnectOptions, GenericFilePath};
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;
use std::{fs::{self, File, OpenOptions, TryLockError}, io::{self, Read, Write}, path::{Component, Path, PathBuf},
    sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc::{SyncSender, TrySendError}}, thread::{self, JoinHandle}, time::{Duration, Instant}};

/// 单轮等待有界，停止信号不依赖额外唤醒连接。
const POLL: Duration = Duration::from_millis(10);
/// 慢客户在此期限后被关闭，不占用永久线程或句柄。
const TIMEOUT: Duration = Duration::from_millis(800);
/// 最多八个在途连接，输入风暴不阻塞退出。
const MAX_CLIENTS: usize = 8;

/// 第二次启动通知旧实例后直接退出，不创建第二个窗口。
pub enum Launch { Primary(Instance), Activated }

/// 持有锁文件直到 listener 停止；不能删除锁文件造成 Unix inode 绕过。
pub struct Instance { _lock: File, stop: Arc<AtomicBool>, worker: Option<JoinHandle<()>> }

impl Instance {
    /// 仅应用私有目录可建立实例锁；锁竞争期间有限等待旧监听器就绪。
    pub fn acquire(directory: &Path, sender: SyncSender<()>) -> io::Result<Launch> {
        let directory = private_directory(directory)?;
        let directory = directory.as_path();
        let lock_path = directory.join("instance.lock");
        if fs::symlink_metadata(&lock_path).is_ok_and(|metadata| linked(&metadata) || !metadata.is_file()) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
        #[cfg(windows)]
        { use std::os::windows::fs::OpenOptionsExt; options.custom_flags(0x0020_0000); }
        let lock = options.open(&lock_path)?;
        if linked(&lock.metadata()?) || !lock.metadata()?.is_file() { return Err(io::ErrorKind::InvalidInput.into()) }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let opened = lock.metadata()?;
            let named = fs::symlink_metadata(&lock_path)?;
            if opened.dev() != named.dev() || opened.ino() != named.ino() { return Err(io::ErrorKind::InvalidInput.into()) }
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) => {
                    if notify(directory).is_ok() { return Ok(Launch::Activated) }
                    if Instant::now() >= deadline { return Err(io::ErrorKind::TimedOut.into()) }
                    thread::sleep(POLL);
                }
                Err(TryLockError::Error(error)) => return Err(error),
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileTypeExt;
            let socket = directory.join("instance.sock");
            match fs::symlink_metadata(&socket) {
                Ok(metadata) if metadata.file_type().is_socket() => fs::remove_file(socket)?,
                Ok(_) => return Err(io::ErrorKind::InvalidInput.into()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {},
                Err(error) => return Err(error),
            }
        }
        let listener = ListenerOptions::new().name(socket_name(directory)?).nonblocking(ListenerNonblockingMode::Both).create_sync()?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = thread::Builder::new().name("single-instance".into()).spawn(move || serve(listener, sender, worker_stop))?;
        Ok(Launch::Primary(Self { _lock: lock, stop, worker: Some(worker) }))
    }
}

impl Drop for Instance {
    /// 非阻塞 listener 允许直接唤醒并 join，不遗留客户端或通知线程。
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() { worker.thread().unpark(); let _ = worker.join(); }
    }
}

/// Windows 不把未读输出转交 interprocess 的 detached limbo flush 线程。
struct Socket(Stream);
impl Drop for Socket {
    /// 每条连接的缓冲区由本模块有界协议管理。
    fn drop(&mut self) {
        #[cfg(windows)]
        { let Stream::NamedPipe(pipe) = &self.0; pipe.inner().assume_flushed(); }
    }
}

/// 客户端只持有定长输入、应答偏移与绝对超时，不分配独立线程。
struct Client { socket: Socket, input: [u8; 6], received: usize, sent: Option<usize>, deadline: Instant }
impl Client {
    /// 每次最多一次读与写，固定 show 协议以外的数据不能触发动作。
    fn poll(&mut self, sender: &SyncSender<()>) -> bool {
        if Instant::now() >= self.deadline { return false }
        if self.sent.is_none() {
            match self.socket.0.read(&mut self.input[self.received..]) {
                Ok(0) => return false,
                Ok(count) => self.received += count,
                Err(error) if retryable(&error) => return true,
                Err(_) => return false,
            }
            if self.received < 5 { return b"show\n".starts_with(&self.input[..self.received]) }
            if self.received != 5 || &self.input[..5] != b"show\n" { return false }
            match sender.try_send(()) {
                Ok(()) | Err(TrySendError::Full(())) => self.sent = Some(0),
                Err(TrySendError::Disconnected(())) => return false,
            }
        }
        let sent = self.sent.as_mut().expect("恢复请求已接收");
        if *sent < 3 {
            match self.socket.0.write(&b"ok\n"[*sent..]) {
                Ok(0) => return false,
                Ok(count) => *sent += count,
                Err(error) if retryable(&error) => return true,
                Err(_) => return false,
            }
        }
        if *sent == 3 {
            let mut extra = [0; 1];
            match self.socket.0.read(&mut extra) { Err(error) if retryable(&error) => {}, _ => return false }
        }
        true
    }
}

/// 监听与全部慢客户端共享一个可停线程；满队列的恢复信号合并。
fn serve(listener: Listener, sender: SyncSender<()>, stop: Arc<AtomicBool>) {
    let mut clients: Vec<Client> = Vec::with_capacity(MAX_CLIENTS);
    while !stop.load(Ordering::Acquire) {
        for _ in 0..MAX_CLIENTS {
            match listener.accept() {
                Ok(stream) if clients.len() < MAX_CLIENTS => clients.push(Client { socket: Socket(stream), input: [0; 6], received: 0, sent: None, deadline: Instant::now() + TIMEOUT }),
                Ok(stream) => drop(Socket(stream)),
                Err(error) if retryable(&error) => break,
                Err(_) => return,
            }
        }
        clients.retain_mut(|client| client.poll(&sender));
        thread::park_timeout(POLL);
    }
}

/// 有界客户端交换不创建后台线程，只有收到确认才判断旧实例已唤醒。
fn notify(directory: &Path) -> io::Result<()> {
    let mut socket = Socket(connect(directory)?);
    let deadline = Instant::now() + TIMEOUT;
    let (mut sent, mut received) = (0, 0);
    let mut ack = [0; 3];
    while Instant::now() < deadline {
        let result = if sent < 5 { socket.0.write(&b"show\n"[sent..]).map(|count| { sent += count; count }) }
            else { socket.0.read(&mut ack[received..]).map(|count| { received += count; count }) };
        match result { Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()), Ok(_) => {}, Err(error) if retryable(&error) => {}, Err(error) => return Err(error) }
        if received == 3 { return if ack == *b"ok\n" { Ok(()) } else { Err(io::ErrorKind::InvalidData.into()) } }
        thread::sleep(POLL);
    }
    Err(io::ErrorKind::TimedOut.into())
}

/// Unix socket 使用应用私有目录，不监听 TCP 端口。
#[cfg(unix)]
fn socket_name(directory: &Path) -> io::Result<Name<'static>> { directory.join("instance.sock").to_fs_name::<GenericFilePath>() }
/// Windows 命名管道由同一规范化应用目录稳定派生。
#[cfg(windows)]
fn socket_name(directory: &Path) -> io::Result<Name<'static>> { pipe_name(directory).to_ns_name::<GenericNamespaced>() }
/// Unicode 路径按 UTF-16 字节散列，避免有损转换造成身份碰撞。
#[cfg(windows)]
fn pipe_name(directory: &Path) -> String {
    use std::os::windows::ffi::OsStrExt;
    let hash = directory.as_os_str().encode_wide().flat_map(u16::to_le_bytes).fold(0xcbf29ce484222325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3));
    format!("app-@@KEBAB_ID@@-{hash:016x}")
}
/// 连接后所有读写保持非阻塞；期限由调用方控制。
#[cfg(unix)]
fn connect(directory: &Path) -> io::Result<Stream> {
    ConnectOptions::new().name(socket_name(directory)?).wait_mode(ConnectWaitMode::Timeout(POLL)).nonblocking_stream(true).connect_sync()
}
/// 显式传 timeout 绕过当前 interprocess local_socket Windows wrapper 的无界等待。
#[cfg(windows)]
fn connect(directory: &Path) -> io::Result<Stream> {
    use interprocess::os::windows::named_pipe::{DuplexPipeStream, local_socket, pipe_mode::Bytes};
    let pipe = DuplexPipeStream::<Bytes>::connect_by_path_with_wait_mode(format!(r"\\.\pipe\{}", pipe_name(directory)), ConnectWaitMode::Timeout(POLL))?;
    pipe.set_nonblocking(true)?;
    Ok(local_socket::Stream::from(pipe).into())
}

/// 逐级拒绝链接/重解析点；新建私有目录使用 0700，不修改已有用户目录权限。
fn private_directory(directory: &Path) -> io::Result<PathBuf> {
    if !directory.is_absolute() { return Err(io::ErrorKind::InvalidInput.into()) }
    let mut current = PathBuf::new();
    for part in directory.components() {
        if part == Component::ParentDir { return Err(io::ErrorKind::InvalidInput.into()) }
        current.push(part);
        if !matches!(part, Component::Normal(_)) { continue }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if !linked(&metadata) && metadata.is_dir() => {},
            Ok(_) => return Err(io::ErrorKind::InvalidInput.into()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let mut builder = fs::DirBuilder::new();
                #[cfg(unix)]
                { use std::os::unix::fs::DirBuilderExt; builder.mode(0o700); }
                if let Err(error) = builder.create(&current) {
                    if error.kind() != io::ErrorKind::AlreadyExists { return Err(error) }
                }
                let metadata = fs::symlink_metadata(&current)?;
                if linked(&metadata) || !metadata.is_dir() { return Err(io::ErrorKind::InvalidInput.into()) }
            }
            Err(error) => return Err(error),
        }
    }
    #[cfg(unix)]
    { use std::os::unix::fs::PermissionsExt; if fs::metadata(directory)?.permissions().mode() & 0o077 != 0 { return Err(io::ErrorKind::PermissionDenied.into()) } }
    directory.canonicalize()
}
/// 同时拒绝 Unix symlink 与 Windows reparse point。
fn linked(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    { use std::os::windows::fs::MetadataExt; metadata.file_attributes() & 0x400 != 0 }
    #[cfg(not(windows))]
    { metadata.file_type().is_symlink() }
}
/// 暂无数据与可中断 syscall 允许在有界轮询中重试。
fn retryable(error: &io::Error) -> bool { matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) }

#[cfg(test)]
#[path = "instance_test.rs"]
mod tests;
