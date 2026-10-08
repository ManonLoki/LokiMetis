//! 真正的后台全局热键；中性初始化没有绑定，直到下游明确登记 typed handler。

use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use gpui_kit::App;
use std::collections::HashSet;

/// 每项明确绑定一个编译期函数；不解析脚本或使用 wildcard/no-op 动作兜底。
pub struct Binding {
    pub id: &'static str,
    pub chord: HotKey,
    pub dispatch: fn(&mut App),
}

/// 管理器只在非空登记时创建；模块仅拥有自己成功登记的键位。
#[derive(Default)]
pub struct Service {
    manager: Option<GlobalHotKeyManager>,
    owned: Vec<Binding>,
}

impl Service {
    /// 初始化仅创建空内存表，不触达 OS、不占用任何快捷键。
    pub fn new() -> Self { Self::default() }

    /// 一次登记一组此前不存在的绑定，任何失败都只回滚本次成功项。
    pub fn register(&mut self, bindings: Vec<Binding>) -> Result<(), &'static str> {
        let mut ids: HashSet<_> = self.owned.iter().map(|binding| binding.id).collect();
        let mut keys: HashSet<_> = self.owned.iter().map(|binding| binding.chord.id()).collect();
        for binding in &bindings {
            if !valid_id(binding.id) || binding.chord.mods.is_empty() || !ids.insert(binding.id) || !keys.insert(binding.chord.id()) {
                return Err("shortcut_invalid_or_duplicate");
            }
        }
        if bindings.is_empty() { return Ok(()) }
        if self.manager.is_none() {
            self.manager = Some(GlobalHotKeyManager::new().map_err(|_| "shortcut_manager_unavailable")?);
        }
        let manager = self.manager.as_ref().expect("热键管理器已创建");
        let (owned, failure) = register_group(bindings, |key| manager.register(key).is_ok(), |key| manager.unregister(key).is_ok());
        self.owned.extend(owned);
        failure.map_or(Ok(()), Err)
    }

    /// 只接受 Pressed 和本模块 owned ID；事件处理在 GPUI 主线程执行。
    pub fn pending(&self) -> Vec<fn(&mut App)> {
        let mut handlers = Vec::new();
        for event in GlobalHotKeyEvent::receiver().try_iter().take(64) {
            if event.state == HotKeyState::Pressed {
                if let Some(binding) = self.owned.iter().find(|binding| binding.chord.id() == event.id) {
                    handlers.push(binding.dispatch);
                }
            }
        }
        handlers
    }
}

/// 部分失败时保留回滚未释放项，以便继续分派并在退出重试注销。
fn register_group(bindings: Vec<Binding>, mut register: impl FnMut(HotKey) -> bool, mut unregister: impl FnMut(HotKey) -> bool) -> (Vec<Binding>, Option<&'static str>) {
    let mut owned: Vec<Binding> = Vec::new();
    for binding in bindings {
        if !register(binding.chord) {
            owned.retain(|previous| !unregister(previous.chord));
            let error = if owned.is_empty() { "shortcut_registration_failed" } else { "shortcut_rollback_failed" };
            return (owned, Some(error));
        }
        owned.push(binding);
    }
    (owned, None)
}

/// 中性动作 ID 采用稳定 ASCII snake_case，产品动作仍由下游合同决定。
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.as_bytes()[0].is_ascii_lowercase()
        && id.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

impl Drop for Service {
    /// 不使用全局清空，只注销本模块实际成功注册的键位。
    fn drop(&mut self) {
        if let Some(manager) = &self.manager {
            for binding in self.owned.drain(..) { let _ = manager.unregister(binding.chord); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Service, Binding, register_group, valid_id};
    use global_hotkey::hotkey::{Code, HotKey, Modifiers};
    /// 空合同启动没有 manager、owned chord 或 OS 注册。
    #[test]
    fn empty_contract_creates_no_manager_or_os_binding() {
        let mut service = Service::new();
        assert!(service.register(Vec::new()).is_ok());
        assert!(service.manager.is_none());
        assert!(service.owned.is_empty());
        assert!(valid_id("approved_action"));
        assert!(!valid_id("../../script"));
    }
    /// 故障注入不注册系统键位；回滚失败项不能失去所有权。
    #[test]
    fn failed_rollback_retains_owned_binding_for_cleanup() {
        let binding = |id, code| Binding { id, chord: HotKey::new(Some(Modifiers::CONTROL), code), dispatch: |_| {} };
        let mut count = 0;
        let (owned, error) = register_group(vec![binding("first", Code::KeyA), binding("second", Code::KeyB)], |_| { count += 1; count == 1 }, |_| false);
        assert_eq!(error, Some("shortcut_rollback_failed"));
        assert_eq!(owned.len(), 1);
        assert_eq!(owned[0].id, "first");
        let (owned, error) = register_group(vec![binding("first", Code::KeyA), binding("second", Code::KeyB)], |key| key.key == Code::KeyA, |_| true);
        assert!(owned.is_empty());
        assert_eq!(error, Some("shortcut_registration_failed"));
    }
}
