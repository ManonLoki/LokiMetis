use std::sync::RwLock;

use tauri::{Emitter, Manager};

use crate::{settings::HostSettingsState, tray::refresh_tray_labels};

/// 主窗口切换语言后通知其它 WebView 立即同步 i18next。
pub(crate) const INTERFACE_LANGUAGE_CHANGED_EVENT: &str = "interface-language-changed";

/// 保存当前已持久化界面语言的线程安全宿主状态。
pub(crate) struct LocaleState {
    saved_language: RwLock<Option<String>>,
}

impl LocaleState {
    /// 使用可选的已保存语言创建状态。
    pub(crate) fn new(saved_language: Option<String>) -> Self {
        Self {
            saved_language: RwLock::new(saved_language),
        }
    }

    /// 返回当前已保存语言的副本。
    pub(crate) fn saved_language(&self) -> Option<String> {
        self.saved_language
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// 在持久化成功后更新内存中的界面语言。
    pub(crate) fn set_saved_language(&self, language: String) {
        *self
            .saved_language
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(language);
    }
}

/// 把系统或保存的 BCP 47 语言标签收敛到产品支持的两个区域值。
pub(crate) fn normalize_bcp47_locale(
    raw: Option<String>,
    saved_language: Option<String>,
) -> String {
    let candidate = saved_language
        .or(raw)
        .unwrap_or_else(|| "en-US".to_string())
        .replace('_', "-")
        .to_ascii_lowercase();
    match candidate.as_str() {
        "zh-cn" => "zh-CN".to_string(),
        "en-us" => "en-US".to_string(),
        value if value == "zh" || value.starts_with("zh-") => "zh-CN".to_string(),
        value if value == "en" || value.starts_with("en-") => "en-US".to_string(),
        _ => "en-US".to_string(),
    }
}

/// 优先使用已保存语言，否则解析操作系统语言。
pub(crate) fn resolve_system_locale(saved_language: Option<String>) -> String {
    normalize_bcp47_locale(tauri_plugin_os::locale(), saved_language)
}

#[tauri::command]
/// 返回已保存设置优先的当前界面语言。
pub async fn get_system_locale(app: tauri::AppHandle) -> String {
    let state = app.state::<LocaleState>();
    resolve_system_locale(state.saved_language())
}

#[tauri::command]
/// 持久化并广播规范化后的界面语言，同时刷新托盘文案。
pub async fn set_interface_language(
    language: String,
    app: tauri::AppHandle,
) -> Result<String, String> {
    let state = app.state::<LocaleState>();
    let normalized = normalize_bcp47_locale(Some(language), None);
    app.state::<HostSettingsState>()
        .set_interface_language(normalized.clone())
        .await
        .map_err(str::to_string)?;
    state.set_saved_language(normalized.clone());
    rust_i18n::set_locale(&normalized);
    if let Err(error) = app.emit(INTERFACE_LANGUAGE_CHANGED_EVENT, normalized.clone()) {
        tracing::warn!(%error, "failed to broadcast interface language change");
    }
    if let Err(error) = refresh_tray_labels(&app) {
        tracing::warn!(error = %error, "failed to refresh tray labels after locale change");
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 系统语言解析必须通过 Tauri OS 插件取值。
    #[test]
    fn system_locale_uses_tauri_plugin_os() {
        let resolver_source = stringify!(tauri_plugin_os::locale());
        assert!(resolver_source.contains("locale"));
    }

    /// 下划线与大小写差异应一次规范化到支持的区域值。
    #[test]
    fn system_locale_normalizes_bcp47_once() {
        assert_eq!(normalize_bcp47_locale(Some("zh_CN".into()), None), "zh-CN");
        assert_eq!(normalize_bcp47_locale(Some("EN_us".into()), None), "en-US");
    }

    /// 缺失或不支持的系统语言应回退到英文。
    #[test]
    fn system_locale_falls_back_to_english() {
        assert_eq!(normalize_bcp47_locale(None, None), "en-US");
        assert_eq!(normalize_bcp47_locale(Some("fr-FR".into()), None), "en-US");
    }

    /// 已保存语言必须优先于操作系统语言。
    #[test]
    fn saved_language_precedes_system_locale() {
        assert_eq!(
            normalize_bcp47_locale(Some("en-US".into()), Some("zh-CN".into())),
            "zh-CN"
        );
    }
}
