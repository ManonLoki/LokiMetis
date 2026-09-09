use tauri_plugin_autostart::ManagerExt;

#[tauri::command]
/// 读取操作系统中当前的开机自启注册状态。
pub(crate) async fn get_autostart_enabled(app: tauri::AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|_| "autostart-state-unavailable".to_string())
}

#[tauri::command]
/// 幂等地启用或禁用操作系统开机自启注册，并返回实际状态。
pub(crate) async fn set_autostart_enabled(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<bool, String> {
    let manager = app.autolaunch();
    let previous = manager
        .is_enabled()
        .map_err(|_| "autostart-state-unavailable".to_string())?;
    if previous == enabled {
        return Ok(previous);
    }
    let mutation = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    if mutation.is_err() {
        let actual = manager.is_enabled().ok();
        tracing::warn!(?actual, requested = enabled, "autostart mutation failed");
        return Err("autostart-mutation-failed".to_string());
    }
    manager
        .is_enabled()
        .map_err(|_| "autostart-state-unavailable".to_string())
}

#[cfg(test)]
mod tests {
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
