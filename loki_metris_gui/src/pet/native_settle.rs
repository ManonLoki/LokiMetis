//! 原生 WM 回执通过前台异步 timer 确认，等待期间不阻塞 UI 或提前写入领域偏好。

use super::{Controller, host};
use gpui_kit::{AnyWindowHandle, AsyncApp, WeakEntity};
use loki_metris_core::HookError;
use std::time::Duration;

/// 偏好与显示各自拥有独立租约；旧窗口、退出和较新动作都会撤销等待。
#[derive(Clone, Copy)]
pub(super) enum Lease {
    /// 尚未交给服务的字段意图保持同一设置代次。
    Preference(u64),
    /// 迁屏和显隐只接受同一显示代次与可见意图；macOS/Windows 正式构建改由原生宿主事务确认。
    #[cfg(any(not(any(target_os = "macos", windows)), test))]
    Display { generation: u64, visible: bool },
    /// 设置窗另有自身句柄与显示代次，关闭不能影响桌宠保存事务。
    #[cfg(any(not(any(target_os = "macos", windows)), test))]
    Settings(u64),
}

/// 返回 false 表示租约已取消；原生错误或总预算耗尽返回可见失败，不能解释成成功。
pub(super) async fn wait(owner: &WeakEntity<Controller>, handle: AnyWindowHandle, lease: Lease, cx: &mut AsyncApp)
    -> Result<bool, HookError> {
    let deadline = cx.background_executor().now() + Duration::from_secs(2);
    loop {
        let result = cx.update(|cx| -> anyhow::Result<Option<bool>> {
            let Some(owner) = owner.upgrade() else { return Ok(None); };
            let current = {
                let owner = owner.read(cx);
                !owner.quitting && match lease {
                    Lease::Preference(generation) => owner.overlay == Some(handle) && owner.busy && owner.settings_generation == generation,
                    #[cfg(any(not(any(target_os = "macos", windows)), test))]
                    Lease::Display { generation, visible } => owner.overlay == Some(handle)
                        && owner.display_generation == generation && owner.visible == visible,
                    #[cfg(any(not(any(target_os = "macos", windows)), test))]
                    Lease::Settings(generation) => owner.settings_window == Some(handle) && owner.settings_window_generation == generation,
                }
            };
            if !current { return Ok(None); }
            handle.update(cx, |_, window, _| host::settled(window)).and_then(|result| result).map(Some)
        });
        match result {
            Ok(None) => return Ok(false),
            Ok(Some(true)) => return Ok(true),
            Ok(Some(false)) => {}
            Err(error) => {
                tracing::warn!(event = "pet_native_settle_failed", %error);
                return Err(HookError::new("error.monitor.petApplyFailed"));
            }
        }
        if cx.background_executor().now() >= deadline {
            tracing::warn!(event = "pet_native_settle_timeout");
            return Err(HookError::new("error.monitor.petApplyFailed"));
        }
        cx.background_executor().timer(Duration::from_millis(16)).await;
    }
}
