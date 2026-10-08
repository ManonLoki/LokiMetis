//! 通过目标平台的用户目录 API 取得默认数据根锚点。

use std::path::PathBuf;

/// 返回 OS API 解析的当前用户主目录，不直接读取 Windows/macOS 的 HOME/USERPROFILE。
pub(crate) fn current_user_home() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let path = PathBuf::from(objc2_foundation::NSHomeDirectory().to_string());
        path.is_absolute().then_some(path)
    }
    #[cfg(target_os = "windows")]
    {
        windows::Storage::UserDataPaths::GetDefault()
            .ok()
            .and_then(|paths| paths.Profile().ok())
            .map(|profile| PathBuf::from(profile.to_string()))
            .filter(|path| path.is_absolute())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证读取到的当前用户主目录总是绝对路径。
    #[test]
    fn current_platform_user_home_is_absolute_when_available() {
        let home = current_user_home().expect("current platform user home is available");
        assert!(home.is_absolute());
    }
}
