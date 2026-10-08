//! 设备偏好只保存有限展示字段；有界后台队列合并写入，窗口线程没有磁盘 I/O。

use gpui_kit::{App, Bounds, WindowBounds, point, px, size};
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
@@NOTIFICATION_FIELD@@
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
@@NOTIFICATION_DECODE@@
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
        let @@NOTIFICATION_MUT@@text = format!("theme={theme}\nlanguage={language}\ncollapsed={}\nmaximized={}\nbounds={bounds}\n", self.collapsed, self.maximized);
@@NOTIFICATION_ENCODE@@
        text
    }
}

/// 工作线程只收到合并信号，始终从内存读取最新快照，磁盘写入不持有 UI 状态锁。
#[derive(Clone)]
pub struct Writer {
    value: Arc<Mutex<Preferences>>,
    sender: mpsc::SyncSender<Option<()>>,
    thread: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
}

impl Writer {
    /// 极短内存更新后非阻塞通知后台；已有信号时自动合并为最新快照。
    pub fn save(&self, value: &Preferences) {
        *self.value.lock().expect("偏好内存锁损坏") = value.clone();
        let _ = self.sender.try_send(Some(()));
    }

    /// 原生应用进入不可取消退出阶段后，发送结束信号并等待最后快照完成写入。
    pub fn finish(&self) {
        let _ = self.sender.send(None);
        if let Some(thread) = self.thread.lock().expect("偏好线程锁损坏").take() {
            let _ = thread.join();
        }
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
    root.filter(|root| root.is_absolute()).map(|root| root.join("app-@@KEBAB_ID@@").join("preferences.txt"))
}

@@INSTANCE_DIRECTORY@@

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
    let value = Arc::new(Mutex::new(preferences.clone()));
    let worker_value = value.clone();
    let (sender, receiver) = mpsc::sync_channel::<Option<()>>(1);
    let thread = thread::spawn(move || {
        while let Ok(signal) = receiver.recv() {
            let latest = worker_value.lock().expect("偏好内存锁损坏").clone();
            if let Some(path) = &path {
                if let Err(error) = write(path, &latest.encode()) {
                    tracing::warn!(event = "preference_save_failed", error_kind = ?error.kind());
                }
            }
            if signal.is_none() { break }
        }
    });
    (preferences, Writer { value, sender, thread: Arc::new(Mutex::new(Some(thread))) })
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

/// 拒绝任何已存在的符号链接或非目录祖先，不跟随外部路径读写偏好。
fn safe_parent(path: &std::path::Path) -> std::io::Result<()> {
    let mut current = PathBuf::new();
    for component in path.parent().expect("偏好文件具有父目录").components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
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
