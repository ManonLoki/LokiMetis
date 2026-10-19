//! 当前用户登录项以 OS 为权威；串行后台线程读写，窗口线程只访问内存快照。

use auto_launch::{AutoLaunch, AutoLaunchBuilder, MacOSLaunchMode};
use std::{sync::{Arc, Mutex, mpsc}, thread};
#[cfg(windows)]
#[path = "autostart_windows.rs"]
mod windows;

/// 未知和关闭必须区分；失败后也以重新读取的实际系统状态展示。
#[derive(Clone, Debug, Default)]
pub struct State {
    /// 最近读取的系统自启状态；None 表示尚未读取或读取失败。
    pub enabled: Option<bool>,
    /// 读写命令进行中，期间不接受新命令。
    pub busy: bool,
    /// 进行中的写入目标；纯读取或空闲时为 None。
    pub desired:Option<bool>,
    /// 最近一次读写失败。
    pub error: bool,
    /// 每次命令完成递增，托盘与菜单据此刷新。
    pub revision: u64,
}

/// 唯一 worker 接收读、写和退出命令，不在初始化时注册登录项。
enum Command { Read, Set(bool), Stop }

/// 应用生命周期持有线程与发送端，退出时等待本模块工作完成。
pub struct Service {
    /// 与 worker 共享的展示状态。
    state: Arc<Mutex<State>>,
    /// 向 worker 投递读写命令的有界通道。
    sender: mpsc::SyncSender<Command>,
    /// 执行系统读写的专用线程，退出时 join。
    worker: Option<thread::JoinHandle<()>>,
}

impl Service {
    /// 首次读取真实 OS 状态；构建登录项句柄不表示已启用。
    pub fn new() -> Self {
        let state = Arc::new(Mutex::new(State { busy: true, ..State::default() }));
        let shared = state.clone();
        let (sender, receiver) = mpsc::sync_channel(2);
        let worker = thread::spawn(move || {
            let handle = handle();
            while let Ok(command) = receiver.recv() {
                if matches!(command, Command::Stop) { break }
                let (actual, failed) = match &handle {
                    Ok(handle) => {
                        let desired = match command { Command::Set(value) => Some(value), _ => None };
                        apply(desired, || read(handle), |enabled| write(handle, enabled))
                    }
                    Err(_) => (None, true),
                };
                let mut state = shared.lock().expect("自启状态锁损坏");
                state.enabled = actual;
                state.busy = false;
                state.desired = None;
                state.error = failed;
                state.revision += 1;
            }
        });
        sender.send(Command::Read).expect("自启工作线程已创建");
        Self { state, sender, worker: Some(worker) }
    }

    /// 渲染只复制内存；不会读取注册表、启动进程或访问偏好文件。
    pub fn state(&self) -> State { self.state.lock().expect("自启状态锁损坏").clone() }

    /// 实际开关拥有 mutation；执行期间不接受第二次修改。
    pub fn set(&self, enabled: bool) { self.enqueue(Command::Set(enabled)); }

    /// 进入设置、激活窗口或失败重试时重新读取 OS；不写入注册表或把 unknown 伪装成 false。
    pub fn reload(&self) { self.enqueue(Command::Read); }

    /// 有界发送不会阻塞 UI，关闭的 worker 立即表现为可观察错误。
    fn enqueue(&self, command: Command) {
        let mut state = self.state.lock().expect("自启状态锁损坏");
        if state.busy { return }
        state.busy = true;
        state.desired = match command {Command::Set(enabled)=>Some(enabled),_=>None};
        if self.sender.try_send(command).is_err() {
            state.busy = false;
            state.desired = None;
            state.error = true;
        }
        state.revision += 1;
    }
}

/// 登录项指向当前可执行文件；macOS 显式传入登录标记并关联本产品 bundle。
fn handle() -> Result<AutoLaunch, String> {
    let executable = std::env::current_exe().map_err(|_| "executable_unavailable")?;
    let executable = executable.to_str().ok_or("executable_not_utf8")?;
    let executable = encoded_path(executable)?;
    let mut builder = AutoLaunchBuilder::new();
    builder.set_app_name("com.loki-metris.app").set_app_path(&executable)
        .set_macos_launch_mode(MacOSLaunchMode::LaunchAgent).set_args(&[] as &[&str]);
    #[cfg(target_os = "macos")]
    builder.set_args(&[super::startup::AUTOSTART_ARGUMENT]).set_bundle_identifiers(&["com.loki-metris.app"]);
    #[cfg(windows)]
    builder.set_windows_enable_mode(auto_launch::WindowsEnableMode::CurrentUser);
    builder.build().map_err(|_| "autostart_unavailable".into())
}

/// 上游不转义 XML/XDG 字段；含特殊字符的路径在任何写入前失败关闭。
fn encoded_path(path: &str) -> Result<String, String> {
    if path.is_empty() || path.chars().any(|value| value.is_control()) { return Err("autostart_unsafe_path".into()) }
    #[cfg(windows)]
    { if path.contains('"') { return Err("autostart_unsafe_path".into()) }
      let command = format!("\"{path}\"");
      if command.encode_utf16().count() + 1 > 260 { return Err("autostart_unsafe_path".into()) }
      Ok(command) }
    #[cfg(target_os = "macos")]
    { if path.contains(['&', '<', '>']) { return Err("autostart_unsafe_path".into()) } Ok(path.into()) }
    #[cfg(not(any(windows, target_os = "macos")))]
    { if !path.chars().all(|value| value.is_alphanumeric() || "/._-".contains(value)) { return Err("autostart_unsafe_path".into()) } Ok(path.into()) }
}

/// Windows 只查询当前用户；上游跨用户查询和删除不能作为本模块的系统事实。
fn read(handle: &AutoLaunch) -> Result<bool, String> {
    #[cfg(windows)]
    { windows::is_enabled("com.loki-metris.app", handle.get_app_path()).map_err(|_| "autostart_read_failed".into()) }
    #[cfg(not(windows))]
    { handle.is_enabled().map_err(|_| "autostart_read_failed".into()) }
}
/// Windows 开启明确 CurrentUser，关闭只删除当前用户同名项，不触达 HKLM。
fn write(handle: &AutoLaunch, enabled: bool) -> Result<(), String> {
    if enabled { return handle.enable().map_err(|_| "autostart_write_failed".into()) }
    #[cfg(windows)]
    { windows::disable("com.loki-metris.app").map_err(|_| "autostart_write_failed".into()) }
    #[cfg(not(windows))]
    { handle.disable().map_err(|_| "autostart_write_failed".into()) }
}
/// 写入后复读必须匹配请求，OS 拒绝生效和读取失败都保留实际状态并显示错误。
fn apply<E>(desired: Option<bool>, mut read: impl FnMut() -> Result<bool, E>, mut write: impl FnMut(bool) -> Result<(), E>) -> (Option<bool>, bool) {
    let result = match desired {
        // 显式关闭也清理同名旧路径；false 只表示当前命令未启用，不能证明 Run 值不存在。
        Some(false) if cfg!(windows) => write(false),
        Some(desired) => read().and_then(|actual| if actual == desired { Ok(()) } else { write(desired) }),
        None => Ok(()),
    };
    let actual = read();
    let failed = result.is_err() || actual.is_err()
        || desired.is_some_and(|desired| actual.as_ref().is_ok_and(|actual| *actual != desired));
    (actual.ok(), failed)
}

/// REG_SZ 必须是有终止符的非空有效命令，损坏值不能冒充开启。
#[cfg(test)]
fn valid_windows_command(bytes: &[u8]) -> bool {
    windows_command(bytes).is_ok()
}
/// 解码当前用户 Run 命令；字节损坏与已不存在的旧路径不是同一种状态。
#[cfg(any(windows, test))]
fn windows_command(bytes: &[u8]) -> std::io::Result<String> {
    if bytes.len() < 4 || !bytes.len().is_multiple_of(2) { return Err(std::io::ErrorKind::InvalidData.into()) }
    let mut words: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|part| u16::from_le_bytes([part[0], part[1]])).collect();
    if words.pop() != Some(0) || words.contains(&0) { return Err(std::io::ErrorKind::InvalidData.into()) }
    let value = String::from_utf16(&words).map_err(|_| std::io::ErrorKind::InvalidData)?;
    if value.trim().is_empty() || value.chars().any(char::is_control) { return Err(std::io::ErrorKind::InvalidData.into()) }
    Ok(value)
}
/// 只认可当前无参数的受引号保护命令；旧副本和额外参数均要求显式重新开启。
#[cfg(any(windows, test))]
fn windows_command_matches(bytes: &[u8], expected: &str) -> std::io::Result<bool> {
    Ok(windows_command(bytes)?.trim_end_matches(' ').eq_ignore_ascii_case(expected))
}
/// 按完整 DWORD 识别已知开关；未知/损坏值返回失败，不猜测 OS 状态。
#[cfg(any(windows, test))]
fn windows_override(bytes: &[u8]) -> std::io::Result<bool> {
    if bytes.len() < 12 { return Err(std::io::ErrorKind::InvalidData.into()) }
    match u32::from_le_bytes(bytes[..4].try_into().expect("已有四字节")) {
        2 | 6 => Ok(true), 3 | 7 => Ok(false), _ => Err(std::io::ErrorKind::InvalidData.into()),
    }
}

impl Drop for Service {
    /// 退出关闭队列并回收自己的工作线程，不遗留 detached task。
    fn drop(&mut self) {
        let _ = self.sender.send(Command::Stop);
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}

#[cfg(test)]
mod tests {
    use super::{State, apply, encoded_path, valid_windows_command, windows_command_matches, windows_override};
    /// 构造macOS登录项只声明后台标记，不注册或改写真实登录项。
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_autostart_configuration_launches_in_background() {
        assert_eq!(super::handle().unwrap().get_args(), &[super::super::startup::AUTOSTART_ARGUMENT]);
    }
    /// 实际 HKCU 读取与后台线程只读初始化；不创建、修改或删除任何系统值。
    #[cfg(windows)]
    #[test]
    fn native_autostart_bridge_reads_and_joins_without_registration() {
        let id = format!("com.loki-metris.audit-missing-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        assert!(!super::windows::is_enabled(&id, r#""C:\unused.exe""#).unwrap());
        let service = super::Service::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while service.state().busy && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let state = service.state();
        assert!(!state.busy);
        assert!(!state.error);
        assert!(state.enabled.is_some());
        service.reload();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while service.state().busy && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let refreshed = service.state();
        assert!(!refreshed.busy);
        assert!(!refreshed.error);
        assert_eq!(refreshed.enabled, state.enabled);
        assert!(refreshed.revision > state.revision);
        drop(service);
    }
    /// 安装位置迁移后旧 Run 项不冒充当前自启；显式开启必须改写并验证当前命令。
    #[test]
    fn stale_windows_run_command_is_repaired_only_on_explicit_enable() {
        let bytes = |value: &str| value.encode_utf16().chain(Some(0)).flat_map(u16::to_le_bytes).collect::<Vec<_>>();
        let expected = r#""C:\Program Files\LokiMetris\app.exe""#;
        let registered = std::cell::RefCell::new(Some(bytes(r#""C:\Old LokiMetris\app.exe" "#)));
        let writes = std::cell::Cell::new(0);
        let read = || registered.borrow().as_ref().map(|bytes| windows_command_matches(bytes, expected)).unwrap_or(Ok(false));
        assert_eq!(apply(None, read, |_| -> std::io::Result<()> { panic!("查询不能修复旧路径") }), (Some(false), false));
        #[cfg(windows)]
        {
            assert_eq!(apply(Some(false), read, |enabled| {
                assert!(!enabled);
                *registered.borrow_mut() = None;
                Ok(())
            }), (Some(false), false));
            assert!(registered.borrow().is_none());
            *registered.borrow_mut() = Some(bytes(r#""C:\Old LokiMetris\app.exe" "#));
        }
        assert_eq!(apply(Some(true), read, |enabled| {
            assert!(enabled);
            writes.set(writes.get() + 1);
            *registered.borrow_mut() = Some(bytes(&format!("{expected} ")));
            Ok(())
        }), (Some(true), false));
        assert_eq!(writes.get(), 1);
        assert_eq!(apply(Some(true), read, |_| -> std::io::Result<()> { panic!("已匹配命令不能重复写入") }), (Some(true), false));
        assert!(!windows_command_matches(&bytes(&format!("{expected} --other")), expected).unwrap());
        assert!(windows_command_matches(&bytes(&format!("{} ", expected.to_ascii_lowercase())), expected).unwrap());
        assert!(windows_command_matches(&[65, 0], expected).is_err());
    }
    /// 未读取的系统状态不能冒充已开启或已关闭，也不授权注册。
    #[test]
    fn initial_autostart_state_is_unknown_and_has_no_registration() {
        let state = State::default();
        assert_eq!(state.enabled, None);
        assert!(!state.busy);
        assert!(!state.error);
    }
    /// 初始化与等值修改不注册，失败修改显示复读的 OS 事实。
    #[test]
    fn read_only_idempotent_and_failed_write_keep_actual_state() {
        assert_eq!(apply::<()>(None, || Ok(false), |_| panic!("初始化不能注册")), (Some(false), false));
        assert_eq!(apply::<()>(Some(true), || Ok(true), |_| panic!("等值不能重复注册")), (Some(true), false));
        assert_eq!(apply(Some(true), || Ok(false), |_| Err(())), (Some(false), true));
        assert_eq!(apply(Some(true), || Err(()), |_| panic!("读取失败不能注册")), (None, true));
        assert!(encoded_path("bad\npath").is_err());
        #[cfg(target_os = "macos")]
        assert!(encoded_path("/Applications/A&B.app/Contents/MacOS/app").is_err());
        #[cfg(windows)]
        assert_eq!(encoded_path(r"C:\Program Files\Tool.exe").unwrap(), r#""C:\Program Files\Tool.exe""#);
    }
    /// OS 接受写入却保留相反状态时必须显示失败；真实状态变更才算成功。
    #[test]
    fn successful_write_requires_readback_to_match_requested_state() {
        for desired in [false, true] {
            assert_eq!(apply::<()>(Some(desired), || Ok(!desired), |_| Ok(())), (Some(!desired), true));
        }
        let actual = std::cell::Cell::new(false);
        assert_eq!(apply::<()>(Some(true), || Ok(actual.get()), |desired| { actual.set(desired); Ok(()) }), (Some(true), false));
    }
    /// 纯数据回归不打开注册表；空命令和损坏 DWORD 均不能显示为开启。
    #[test]
    fn windows_registry_values_reject_empty_or_malformed_state() {
        assert!(!valid_windows_command(&[0, 0, 0, 0]));
        assert!(!valid_windows_command(&[65, 0, 66, 0]));
        assert!(valid_windows_command(&[65, 0, 0, 0]));
        assert!(windows_override(&[2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).is_err());
        assert!(windows_override(&[2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap());
        assert!(!windows_override(&[3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap());
        #[cfg(windows)]
        assert!(encoded_path(&format!("C:\\{}", "x".repeat(260))).is_err());
    }
}
