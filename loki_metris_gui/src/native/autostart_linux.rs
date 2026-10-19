//! XDG 登录项必须实际指向当前带标记命令；旧无参数项只在用户重新开启时替换。

use std::{fs::File, io::{self, Read}, path::Path};

/// auto-launch 0.6 的 XDG 路径固定在 HOME/.config；此处只读相同的本应用条目。
pub(super) fn is_enabled(handle: &auto_launch::AutoLaunch) -> io::Result<bool> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute()).ok_or(io::ErrorKind::NotFound)?;
    let path = home.join(".config/autostart/com.loki-metris.app.desktop");
    let command = format!("{} {}", handle.get_app_path(), handle.get_args().join(" "));
    read_entry(&path, &command)
}

/// 有界读取区分缺失、损坏与权限错误；查询不改写旧项或其它桌面字段。
fn read_entry(path: &Path, command: &str) -> io::Result<bool> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(16 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 16 * 1024 { return Err(io::ErrorKind::InvalidData.into()); }
    let entry = std::str::from_utf8(&bytes).map_err(|_| io::ErrorKind::InvalidData)?;
    matches_entry(entry, command)
}

/// 仅接受主桌面组中唯一的完整 Exec，保留用户或桌面显式禁用的真实状态。
fn matches_entry(entry: &str, command: &str) -> io::Result<bool> {
    let mut in_main = false;
    let mut found_main = false;
    let mut exec = None;
    let mut disabled = false;
    for line in entry.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')) {
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            if in_main && std::mem::replace(&mut found_main, true) { return Err(io::ErrorKind::InvalidData.into()); }
        } else if in_main {
            let Some((key, value)) = line.split_once('=') else { continue; };
            match (key.trim(), value.trim()) {
                ("Exec", value) => {
                    if exec.replace(value).is_some() { return Err(io::ErrorKind::InvalidData.into()); }
                }
                ("Hidden", "true") | ("X-GNOME-Autostart-enabled", "false") => disabled = true,
                _ => {}
            }
        }
    }
    Ok(!disabled && exec == Some(command))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 旧项、其它副本和显式禁用不冒充当前静默登录项；只有完整命令匹配才开启。
    #[test]
    fn linux_autostart_requires_the_marked_current_command() {
        let command = "/opt/com.loki-metris.app/loki_metris_gui --autostart";
        let current = format!("[Desktop Entry]\nType=Application\nExec={command} \nTerminal=false");
        assert!(matches_entry(&current, command).unwrap());
        for entry in [
            current.replace(" --autostart", ""),
            current.replace("/opt/", "/old/"),
            format!("{current}\nHidden=true"),
            format!("{current}\nX-GNOME-Autostart-enabled=false"),
            format!("[Other Group]\nExec={command}"),
        ] { assert!(!matches_entry(&entry, command).unwrap()); }
        assert!(matches_entry(&format!("{current}\nExec={command}"), command).is_err());
    }

    /// 隔离旧登录项只读且字节保留，损坏和超长输入明确失败；缺失是关闭状态。
    #[test]
    fn linux_autostart_query_preserves_existing_entry() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("app.desktop");
        assert!(!read_entry(&path, "app --autostart").unwrap());
        let original = b"[Desktop Entry]\nExec=app \n# user settings\n";
        std::fs::write(&path, original).unwrap();
        assert!(!read_entry(&path, "app --autostart").unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::write(&path, [0xff]).unwrap();
        assert_eq!(read_entry(&path, "app").unwrap_err().kind(), io::ErrorKind::InvalidData);
        std::fs::write(&path, vec![b' '; 16 * 1024 + 1]).unwrap();
        assert_eq!(read_entry(&path, "app").unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    /// 同路径旧无标记项在用户开启时更新，明确关闭也删除仍会被桌面执行的旧项。
    #[test]
    fn linux_legacy_entry_is_changed_only_by_explicit_toggle() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("app.desktop");
        let old = "[Desktop Entry]\nExec=app \n";
        let current = "[Desktop Entry]\nExec=app --autostart\n";
        std::fs::write(&path, old).unwrap();
        let read = || read_entry(&path, "app --autostart");
        assert_eq!(super::super::apply(None, read, |_| -> io::Result<()> { panic!("查询不能更新旧项"); }), (Some(false), false));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), old);
        assert_eq!(super::super::apply(Some(true), read, |enabled| {
            assert!(enabled);
            std::fs::write(&path, current)
        }), (Some(true), false));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), current);
        std::fs::write(&path, old).unwrap();
        assert_eq!(super::super::apply(Some(false), read, |enabled| {
            assert!(!enabled);
            std::fs::remove_file(&path)
        }), (Some(false), false));
        assert!(!path.exists());
    }
}
