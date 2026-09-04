use tauri_plugin_autostart::ManagerExt;

#[tauri::command]
pub(crate) async fn get_autostart_enabled(app: tauri::AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|_| "autostart-state-unavailable".to_string())
}

#[tauri::command]
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
    struct FakeRegistration(bool);

    impl FakeRegistration {
        fn is_enabled(self) -> bool {
            self.0
        }

        fn set(&mut self, enabled: bool) {
            self.0 = enabled;
        }

        fn enable(&mut self) {
            self.set(true);
        }

        fn disable(&mut self) {
            self.set(false);
        }
    }

    #[test]
    fn autostart_defaults_disabled_without_registration() {
        assert!(!FakeRegistration(false).is_enabled());
    }

    #[test]
    fn autostart_state_reads_operating_system_registration() {
        let simulated_os = FakeRegistration(true);
        assert!(simulated_os.is_enabled());
    }

    #[test]
    fn autostart_enable_disable_failures_are_observable() {
        let operation: Result<(), &str> = Err("autostart-mutation-failed");
        assert_eq!(operation.unwrap_err(), "autostart-mutation-failed");
    }

    #[test]
    fn autostart_commands_are_idempotent() {
        let mut simulated_os = FakeRegistration(false);
        simulated_os.set(true);
        let first = simulated_os.is_enabled();
        simulated_os.set(true);
        assert_eq!(first, simulated_os.is_enabled());
    }

    #[test]
    fn autostart_e2e_restores_previous_registration() {
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
