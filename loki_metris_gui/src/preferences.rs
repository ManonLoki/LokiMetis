//! 设备偏好只保存有限展示字段；有界后台队列合并写入，窗口线程没有磁盘 I/O。

use gpui_kit::{App, Bounds, WindowBounds, point, px, size};
use loki_metris_core::window_placement::WindowStateV2;
use std::{fs, io::{Read, Write}, path::PathBuf, sync::{Arc, Mutex, mpsc}, thread};

/// 设置页支持浅色、深色与随系统变化三种明确选择。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeChoice {
    #[default]
    System,
    Light,
    Dark,
}

/// 语言缺省跟随操作系统；未知系统语言回退到英文。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LanguageChoice {
    #[default]
    System,
    Chinese,
    English,
}

/// 展示偏好与可恢复窗口几何不进入业务核心或页面会话状态。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preferences {
    pub theme: ThemeChoice,
    pub language: LanguageChoice,
    pub collapsed: bool,
    pub bounds: Option<[f32; 4]>,
    pub maximized: bool,
    pub system_notification: bool,
    /// 有身份的屏内 DIP 状态；旧 bounds/maximized 继续保留供旧版本回退。
    pub window_state_v2: Option<WindowStateV2>,
}

impl Preferences {
    /// 语言只从已选择值或真实系统 locale 解析。
    pub fn locale(&self) -> &'static str {
        match self.language {
            LanguageChoice::Chinese => "zh-CN",
            LanguageChoice::English => "en-US",
            LanguageChoice::System => {
                if sys_locale::get_locale().is_some_and(|locale| locale.to_ascii_lowercase().starts_with("zh")) {
                    "zh-CN"
                } else {
                    "en-US"
                }
            }
        }
    }

    /// 损坏、过大、过小或离开所有显示器的窗口状态采用居中回退。
    pub fn window_bounds(&self, cx: &App) -> Option<WindowBounds> {
        let [x, y, width, height] = self.bounds?;
        if ![x, y, width, height].into_iter().all(f32::is_finite)
            || !(960. ..=16_384.).contains(&width)
            || !(640. ..=16_384.).contains(&height) {
            return None;
        }
        let bounds = Bounds::new(point(px(x), px(y)), size(px(width), px(height)));
        if !cx.displays().iter().any(|display| display.bounds().intersects(&bounds)) {
            return None;
        }
        Some(if self.maximized { WindowBounds::Maximized(bounds) } else { WindowBounds::Windowed(bounds) })
    }

    /// 白名单格式不反序列化可执行内容，未知或损坏字段使用默认值。
    fn decode(text: &str) -> Self {
        let mut value = Self::default();
        for line in text.lines() {
            let Some((key, field)) = line.split_once('=') else { continue };
            match key {
                "theme" => value.theme = match field { "light" => ThemeChoice::Light, "dark" => ThemeChoice::Dark, _ => ThemeChoice::System },
                "language" => value.language = match field { "zh-CN" => LanguageChoice::Chinese, "en-US" => LanguageChoice::English, _ => LanguageChoice::System },
                "collapsed" => value.collapsed = field == "true",
                "maximized" => value.maximized = field == "true",
                "system_notification" => value.system_notification = field == "true",
                "window_state_v2" => value.window_state_v2 = serde_json::from_str::<WindowStateV2>(field)
                    .ok().filter(WindowStateV2::is_valid),
                "bounds" => {
                    let fields: Vec<_> = field.split(',').map(str::parse::<f32>).collect();
                    if let [Ok(x), Ok(y), Ok(width), Ok(height)] = fields.as_slice() {
                        value.bounds = Some([*x, *y, *width, *height]);
                    }
                }
                _ => {}
            }
        }
        value
    }

    /// 序列化仅包含明确允许的展示偏好与窗口状态。
    fn encode(&self) -> String {
        let theme = match self.theme { ThemeChoice::System => "system", ThemeChoice::Light => "light", ThemeChoice::Dark => "dark" };
        let language = match self.language { LanguageChoice::System => "system", LanguageChoice::Chinese => "zh-CN", LanguageChoice::English => "en-US" };
        let bounds = self.bounds.map(|b| format!("{},{},{},{}", b[0], b[1], b[2], b[3])).unwrap_or_default();
        let mut text = format!("theme={theme}\nlanguage={language}\ncollapsed={}\nmaximized={}\nbounds={bounds}\nsystem_notification={}\n", self.collapsed, self.maximized, self.system_notification);
        if let Some(state) = self.window_state_v2.as_ref().filter(|state| state.is_valid())
            && let Ok(json) = serde_json::to_string(state) {
                text.push_str("window_state_v2="); text.push_str(&json); text.push('\n');
            }
        text
    }
}

/// 明确偏好逐字段提交，不能携带可能过时的窗口或其他偏好快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreferencePatch { Theme(ThemeChoice), Language(LanguageChoice), SystemNotification(bool) }

/// 几何属于可合并高频事件，不拥有主题与语言字段。
#[derive(Clone)]
struct WindowGeometry {
    bounds: Option<[f32; 4]>,
    maximized: bool,
    window_state_v2: Option<WindowStateV2>,
}

/// 几何只占一个待处理信号，不能挤满明确偏好命令的队列。
#[derive(Default)]
struct WindowUpdates { latest:Option<WindowGeometry>, queued:bool }

/// 有界命令队列保留真实回执；窗口事件仅发送合并信号。
enum Command {
    Window,
    Mutate(PreferencePatch,tokio::sync::oneshot::Sender<Result<PreferencePatch,&'static str>>),
    Stop,
}

/// 只有测试可使用显式内存存储，生产缺路径不能报告落盘成功。
enum Storage { File(PathBuf), Unavailable, #[cfg(test)] Memory }

/// 工作线程拥有唯一已提交快照，写入成功后才更新基线并通知 UI。
#[derive(Clone)]
pub struct Writer {
    geometry: Arc<Mutex<WindowUpdates>>,
    sender: mpsc::SyncSender<Command>,
    closing: Arc<Mutex<bool>>,
    thread: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
}

impl Writer {
    /// 明确提交要么获得独立回执，要么同步报告队列不可用，绝不静默丢弃。
    pub fn mutate(&self,patch:PreferencePatch)->Result<tokio::sync::oneshot::Receiver<Result<PreferencePatch,&'static str>>,&'static str> {
        let closing=self.closing.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if *closing {return Err("preference_writer_unavailable")}
        let (tx,rx)=tokio::sync::oneshot::channel();
        self.sender.try_send(Command::Mutate(patch,tx)).map_err(|_|"preference_writer_unavailable")?;Ok(rx)
    }

    /// 旧几何没有可信屏幕身份，提交时清除新记录，仍保留有效尺寸用于居中恢复。
    #[cfg(any(test, not(any(target_os="macos", windows))))]
    pub fn save_window(&self,bounds:Option<[f32;4]>,maximized:bool) {
        self.queue_window(bounds, maximized, None);
    }

    /// 最终原生读回同时提交新状态和兼容旧字段；同一合并槽不丢失屏幕身份。
    pub fn save_window_v2(&self, bounds: Option<[f32; 4]>, maximized: bool, state: WindowStateV2) {
        if state.is_valid() { self.queue_window(bounds, maximized, Some(state)); }
    }

    /// 高频事件合并完整几何；明确偏好命令不拥有任何窗口字段。
    fn queue_window(&self, bounds: Option<[f32; 4]>, maximized: bool, state: Option<WindowStateV2>) {
        let closing=self.closing.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if *closing {return}
        let mut updates=self.geometry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        updates.latest=Some(WindowGeometry{bounds,maximized,window_state_v2:state});
        if !updates.queued {updates.queued=true;if self.sender.try_send(Command::Window).is_err() {updates.queued=false;}}
    }

    /// 原生应用进入不可取消退出阶段后，发送结束信号并等待最后快照完成写入。
    pub fn finish(&self) {
        if let Some(thread) = self.thread.lock().expect("偏好线程锁损坏").take() {
            {
                let mut closing=self.closing.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                *closing=true;
                let _ = self.sender.send(Command::Stop);
            }
            let _ = thread.join();
        }
    }

    /// 测试明确注入初始偏好，与 Shell 的已确认基线保持一致。
    #[cfg(test)]
    pub(crate) fn in_memory(preferences:Preferences)->Self {start_writer(Storage::Memory,preferences)}

    /// 测试持有既有几何锁，确定性阻止 worker 写入及发送回执，释放后恢复原有流程。
    #[cfg(test)]
    pub(crate) fn pause_worker_for_test(&self) -> impl Drop + '_ {
        self.geometry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// 路径采用各平台常见用户目录，没有可用用户根时只保留进程内偏好。
fn path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let root = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let root = std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"));
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let root = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    root.filter(|root| root.is_absolute()).map(|root| root.join("app-loki-metris").join("preferences.txt"))
}

/// 单实例使用独立私有子目录；不改变已有偏好目录的权限。
pub(crate) fn instance_directory() -> std::io::Result<PathBuf> {
    path().and_then(|path| path.parent().map(|parent| parent.join("instance"))).ok_or(std::io::ErrorKind::NotFound.into())
}

/// 有界读取发生在事件循环前；后台线程在全部信号处理完毕后释放。
pub fn load() -> (Preferences, Writer) {
    load_from(path())
}

/// 可测试的受限加载器只接受已有应用拥有的偏好路径。
pub(crate) fn load_from(path: Option<PathBuf>) -> (Preferences, Writer) {
    let preferences = path.as_ref().and_then(|path| {
        safe_parent(path).ok()?;
        let metadata = fs::symlink_metadata(path).ok()?;
        if !metadata.is_file() || metadata.len() > 4096 { return None }
        let mut text = String::new();
        fs::File::open(path).ok()?.take(4097).read_to_string(&mut text).ok()?;
        (text.len() <= 4096).then(|| Preferences::decode(&text))
    }).unwrap_or_default();
    let writer=start_writer(path.map(Storage::File).unwrap_or(Storage::Unavailable),preferences.clone());
    (preferences,writer)
}

/// 单线程读改写字段与几何，失败不修改已提交偏好，不会让后续命令继承失败值。
fn start_writer(storage:Storage,mut committed:Preferences)->Writer {
    let geometry=Arc::new(Mutex::new(WindowUpdates::default()));let worker_geometry=geometry.clone();
    let (sender, receiver) = mpsc::sync_channel::<Command>(16);
    let thread = thread::spawn(move || {
        while let Ok(command) = receiver.recv() {
            let mut next=committed.clone();
            let geometry={let mut updates=worker_geometry.lock().unwrap_or_else(std::sync::PoisonError::into_inner);if matches!(&command,Command::Window) {updates.queued=false;}updates.latest.clone()};
            if let Some(geometry)=geometry {
                next.bounds=geometry.bounds;next.maximized=geometry.maximized;
                next.window_state_v2 = geometry.window_state_v2;
                if let Some(state) = next.window_state_v2.as_mut() { state.maximized = geometry.maximized; }
            }
            if let Command::Mutate(patch,_)=&command {match patch {
                PreferencePatch::Theme(theme)=>next.theme=*theme,PreferencePatch::Language(language)=>next.language=*language,PreferencePatch::SystemNotification(enabled)=>next.system_notification=*enabled,
            }}
            let result=match &storage {
                Storage::File(path)=>write(path,&next.encode()),
                Storage::Unavailable=>Err(std::io::ErrorKind::NotFound.into()),
                #[cfg(test)] Storage::Memory=>Ok(()),
            };
            if result.is_ok() {committed=next;}
            match command {
                Command::Mutate(patch,reply)=>{let _=reply.send(result.map(|_|patch).map_err(|_|"preference_save_failed"));},
                Command::Stop=>break,
                Command::Window=>{if let Err(error)=result&& matches!(storage,Storage::File(_)) {tracing::warn!(event="preference_save_failed",error_kind=?error.kind());}},
            }
        }
    });
    Writer { geometry, sender, closing:Arc::new(Mutex::new(false)), thread: Arc::new(Mutex::new(Some(thread))) }
}

/// 同目录临时文件原子替换，拒绝目标符号链接并清理失败临时文件。
fn write(path: &PathBuf, text: &str) -> std::io::Result<()> {
    let parent = path.parent().expect("偏好文件具有父目录");
    safe_parent(path)?;
    fs::create_dir_all(parent)?;
    safe_parent(path)?;
    if fs::symlink_metadata(path).is_ok_and(|metadata| !metadata.is_file()) {
        return Err(std::io::Error::other("preference target must be a regular file"));
    }
    static NEXT_FILE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = NEXT_FILE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = parent.join(format!(".preferences-{}-{sequence}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
    let result = (|| {
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() { let _ = fs::remove_file(&temporary); }
    result
}

/// 父路径必须解析为目录，允许 macOS /var 系统别名；目标文件仍单独拒绝链接。
fn safe_parent(path: &std::path::Path) -> std::io::Result<()> {
    let mut current = PathBuf::new();
    for component in path.parent().expect("偏好文件具有父目录").components() {
        if component == std::path::Component::ParentDir { return Err(std::io::ErrorKind::InvalidInput.into()); }
        current.push(component.as_os_str());
        // Windows verbatim 前缀单独的 \\?\C: 不是可查询目录，必须等完整根拼好。
        if matches!(component, std::path::Component::Prefix(_)) { continue; }
        match fs::metadata(&current) {
            Ok(metadata) if !metadata.is_dir() => return Err(std::io::Error::other("preference ancestors must be real directories")),
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "preferences_test.rs"]
mod tests;
