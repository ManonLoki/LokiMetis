//! 当前用户登录项以 OS 为权威；串行后台线程读写，窗口线程只访问内存快照。

use auto_launch::{AutoLaunch, AutoLaunchBuilder, MacOSLaunchMode};
use std::{sync::{Arc, Mutex, mpsc}, thread};
#[cfg(windows)]
#[path = "autostart_windows.rs"]
mod windows;

/// 未知和关闭必须区分；失败后也以重新读取的实际系统状态展示。
#[derive(Clone, Debug, Default)]
pub struct State {
    pub enabled: Option<bool>,
    pub busy: bool,
    pub error: bool,
    pub revision: u64,
}

/// 唯一 worker 接收读、写和退出命令，不在初始化时注册登录项。
enum Command { Read, Set(bool), Stop }

/// 应用生命周期持有线程与发送端，退出时等待本模块工作完成。
pub struct Service {
    state: Arc<Mutex<State>>,
    sender: mpsc::SyncSender<Command>,
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

    /// 读取失败后可重试，不把 unknown 伪装成 false。
    pub fn reload(&self) { self.enqueue(Command::Read); }

    /// 有界发送不会阻塞 UI，关闭的 worker 立即表现为可观察错误。
    fn enqueue(&self, command: Command) {
        let mut state = self.state.lock().expect("自启状态锁损坏");
        if state.busy { return }
        state.busy = true;
        if self.sender.try_send(command).is_err() {
            state.busy = false;
            state.error = true;
        }
        state.revision += 1;
    }
}

/// 登录项只指向当前可执行文件；无隐藏或最小化参数，下次登录正常显示主窗口。
fn handle() -> Result<AutoLaunch, String> {
    let executable = std::env::current_exe().map_err(|_| "executable_unavailable")?;
    let executable = executable.to_str().ok_or("executable_not_utf8")?;
    let executable = encoded_path(executable)?;
    let mut builder = AutoLaunchBuilder::new();
    builder.set_app_name("@@APP_IDENTIFIER@@").set_app_path(&executable)
        .set_macos_launch_mode(MacOSLaunchMode::LaunchAgent).set_args(&[] as &[&str]);
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
    { let _ = handle; windows::is_enabled("@@APP_IDENTIFIER@@").map_err(|_| "autostart_read_failed".into()) }
    #[cfg(not(windows))]
    { handle.is_enabled().map_err(|_| "autostart_read_failed".into()) }
}
/// Windows 开启明确 CurrentUser，关闭只删除当前用户同名项，不触达 HKLM。
fn write(handle: &AutoLaunch, enabled: bool) -> Result<(), String> {
    if enabled { return handle.enable().map_err(|_| "autostart_write_failed".into()) }
    #[cfg(windows)]
    { windows::disable("@@APP_IDENTIFIER@@").map_err(|_| "autostart_write_failed".into()) }
    #[cfg(not(windows))]
    { handle.disable().map_err(|_| "autostart_write_failed".into()) }
}
/// 写入后复读必须匹配请求，OS 拒绝生效和读取失败都保留实际状态并显示错误。
fn apply<E>(desired: Option<bool>, mut read: impl FnMut() -> Result<bool, E>, mut write: impl FnMut(bool) -> Result<(), E>) -> (Option<bool>, bool) {
    let result = match desired {
        Some(desired) => read().and_then(|actual| if actual == desired { Ok(()) } else { write(desired) }),
        None => Ok(()),
    };
    let actual = read();
    let failed = result.is_err() || actual.is_err()
        || desired.is_some_and(|desired| actual.as_ref().is_ok_and(|actual| *actual != desired));
    (actual.ok(), failed)
}

/// REG_SZ 必须是有终止符的非空有效命令，损坏值不能冒充开启。
#[cfg(any(windows, test))]
fn valid_windows_command(bytes: &[u8]) -> bool {
    if bytes.len() < 4 || bytes.len() % 2 != 0 { return false }
    let mut words: Vec<u16> = bytes.chunks_exact(2).map(|part| u16::from_le_bytes([part[0], part[1]])).collect();
    if words.pop() != Some(0) || words.contains(&0) { return false }
    String::from_utf16(&words).is_ok_and(|value| !value.trim().is_empty() && !value.chars().any(char::is_control))
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
    use super::{State, apply, encoded_path, valid_windows_command, windows_override};
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
        assert_eq!(windows_override(&[2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap(), true);
        assert_eq!(windows_override(&[3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap(), false);
        #[cfg(windows)]
        assert!(encoded_path(&format!("C:\\{}", "x".repeat(260))).is_err());
    }
}
