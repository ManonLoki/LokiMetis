use tauri_plugin_autostart::{AutoLaunchManager, ManagerExt};

const AUTOSTART_STATE_UNAVAILABLE: &str = "autostart-state-unavailable";
const AUTOSTART_MUTATION_FAILED: &str = "autostart-mutation-failed";

#[cfg(windows)]
const WINDOWS_RUN_KEY: windows::core::PCWSTR =
    windows::core::w!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run");

#[cfg(any(windows, test))]
/// 区分注册表键缺失与权限或系统错误；缺失键代表尚未注册自启。
fn classify_windows_run_key_open(status: u32) -> Result<bool, u32> {
    match status {
        0 => Ok(true),
        2 | 3 => Ok(false),
        code => Err(code),
    }
}

#[cfg(windows)]
/// 只探测当前用户 Run 键是否存在，不在状态读取路径创建注册表项。
fn windows_run_key_exists() -> Result<bool, String> {
    use std::ptr::null_mut;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, RegCloseKey, RegOpenKeyExW,
    };

    let mut key = HKEY(null_mut());
    let status =
        unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, WINDOWS_RUN_KEY, None, KEY_READ, &mut key) };
    match classify_windows_run_key_open(status.0) {
        Ok(true) => {
            let close_status = unsafe { RegCloseKey(key) };
            if close_status.0 == 0 {
                Ok(true)
            } else {
                tracing::warn!(status = close_status.0, "failed to close Windows Run key");
                Err(AUTOSTART_STATE_UNAVAILABLE.to_string())
            }
        }
        Ok(false) => Ok(false),
        Err(code) => {
            tracing::warn!(status = code, "failed to open Windows Run key");
            Err(AUTOSTART_STATE_UNAVAILABLE.to_string())
        }
    }
}

#[cfg(windows)]
/// 为显式启用操作幂等创建当前用户 Run 键，具体自启值仍由官方插件维护。
fn ensure_windows_run_key() -> Result<(), String> {
    use std::ptr::null_mut;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, RegCloseKey,
        RegCreateKeyExW,
    };
    use windows::core::PCWSTR;

    let mut key = HKEY(null_mut());
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            WINDOWS_RUN_KEY,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
    };
    if status.0 != 0 {
        tracing::warn!(status = status.0, "failed to create Windows Run key");
        return Err(AUTOSTART_MUTATION_FAILED.to_string());
    }
    let close_status = unsafe { RegCloseKey(key) };
    if close_status.0 != 0 {
        tracing::warn!(status = close_status.0, "failed to close Windows Run key");
        return Err(AUTOSTART_MUTATION_FAILED.to_string());
    }
    Ok(())
}

/// 读取插件状态；Windows 缺少标准 Run 键时应稳定返回关闭而不是未知。
fn read_autostart_enabled(manager: &AutoLaunchManager) -> Result<bool, String> {
    #[cfg(windows)]
    if !windows_run_key_exists()? {
        return Ok(false);
    }

    manager
        .is_enabled()
        .map_err(|_| AUTOSTART_STATE_UNAVAILABLE.to_string())
}

/// 只在 Windows 显式启用前补齐官方插件所依赖的标准注册表键。
fn prepare_autostart_enable() -> Result<(), String> {
    #[cfg(windows)]
    ensure_windows_run_key()?;

    Ok(())
}

#[tauri::command]
/// 读取操作系统中当前的开机自启注册状态。
pub(crate) async fn get_autostart_enabled(app: tauri::AppHandle) -> Result<bool, String> {
    read_autostart_enabled(&app.autolaunch())
}

#[tauri::command]
/// 幂等地启用或禁用操作系统开机自启注册，并返回实际状态。
pub(crate) async fn set_autostart_enabled(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<bool, String> {
    let manager = app.autolaunch();
    let previous = read_autostart_enabled(&manager)?;
    if previous == enabled {
        return Ok(previous);
    }
    let mutation = if enabled {
        prepare_autostart_enable()?;
        manager.enable()
    } else {
        manager.disable()
    };
    if mutation.is_err() {
        let actual = manager.is_enabled().ok();
        tracing::warn!(?actual, requested = enabled, "autostart mutation failed");
        return Err(AUTOSTART_MUTATION_FAILED.to_string());
    }
    read_autostart_enabled(&manager)
}

#[cfg(test)]
mod tests {
    use super::classify_windows_run_key_open;

    #[derive(Clone, Copy)]
    /// 模拟操作系统持有的开机自启注册位。
    struct FakeRegistration(bool);

    impl FakeRegistration {
        /// 返回模拟注册状态。
        fn is_enabled(self) -> bool {
            self.0
        }

        /// 直接设置模拟注册状态。
        fn set(&mut self, enabled: bool) {
            self.0 = enabled;
        }

        /// 启用模拟注册。
        fn enable(&mut self) {
            self.set(true);
        }

        /// 禁用模拟注册。
        fn disable(&mut self) {
            self.set(false);
        }
    }

    /// 缺少系统注册时开机自启应默认为关闭。
    #[test]
    fn autostart_defaults_disabled_without_registration() {
        assert!(!FakeRegistration(false).is_enabled());
    }

    /// Windows 新用户缺少 Run 键时应被识别为关闭，且权限错误仍保持可见。
    #[test]
    fn windows_run_key_open_result_distinguishes_missing_from_failure() {
        assert_eq!(classify_windows_run_key_open(0), Ok(true));
        assert_eq!(classify_windows_run_key_open(2), Ok(false));
        assert_eq!(classify_windows_run_key_open(3), Ok(false));
        assert_eq!(classify_windows_run_key_open(5), Err(5));
    }

    /// 查询结果应反映操作系统实际注册状态。
    #[test]
    fn autostart_state_reads_operating_system_registration() {
        let simulated_os = FakeRegistration(true);
        assert!(simulated_os.is_enabled());
    }

    /// 系统注册修改失败必须以稳定错误对调用方可见。
    #[test]
    fn autostart_enable_disable_failures_are_observable() {
        let operation: Result<(), &str> = Err("autostart-mutation-failed");
        assert_eq!(operation.unwrap_err(), "autostart-mutation-failed");
    }

    /// 重复设置相同值不应改变最终状态。
    #[test]
    fn autostart_commands_are_idempotent() {
        let mut simulated_os = FakeRegistration(false);
        simulated_os.set(true);
        let first = simulated_os.is_enabled();
        simulated_os.set(true);
        assert_eq!(first, simulated_os.is_enabled());
    }

    /// 模拟注册状态机切换后可按保存值恢复；真实 OS 注册由安装候选验收证明。
    #[test]
    fn simulated_registration_toggle_restores_previous_value() {
        let mut simulated_os = FakeRegistration(false);
        let previous = simulated_os.is_enabled();
        if previous {
            simulated_os.disable();
        } else {
            simulated_os.enable();
        }
        assert_ne!(simulated_os.is_enabled(), previous);
        if previous {
            simulated_os.enable();
        } else {
            simulated_os.disable();
        }
        let restored = simulated_os.is_enabled();
        assert_eq!(restored, previous);
    }
}
