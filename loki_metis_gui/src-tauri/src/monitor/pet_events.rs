//! 桌宠原生窗口与前端读模型之间的轻量失效通知。

use tauri::{AppHandle, Emitter, Runtime};

/// 桌宠读模型发生变化时广播的固定事件名。
pub const PET_WINDOW_STATE_CHANGED_EVENT: &str = "pet-window-state-changed";

/// 在状态锁和文件 I/O 之外广播桌宠读模型失效通知。
pub fn emit_pet_window_state_changed<R: Runtime>(app: &AppHandle<R>) {
    if let Err(error) = app.emit(PET_WINDOW_STATE_CHANGED_EVENT, ()) {
        tracing::warn!(%error, "failed to emit pet window state change");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 原生事件名必须与前端监听契约保持一致。
    #[test]
    fn event_name_matches_frontend_contract() {
        assert_eq!(PET_WINDOW_STATE_CHANGED_EVENT, "pet-window-state-changed");
    }
}
