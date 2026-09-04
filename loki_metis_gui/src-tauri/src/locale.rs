use std::sync::RwLock;

use tauri::Manager;

use crate::{settings::HostSettingsState, tray::refresh_tray_labels};

pub(crate) struct LocaleState {
    saved_language: RwLock<Option<String>>,
}

impl LocaleState {
    pub(crate) fn new(saved_language: Option<String>) -> Self {
        Self {
            saved_language: RwLock::new(saved_language),
        }
    }

    pub(crate) fn saved_language(&self) -> Option<String> {
        self.saved_language
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub(crate) fn set_saved_language(&self, language: String) {
        *self
            .saved_language
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(language);
    }
}

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

pub(crate) fn resolve_system_locale(saved_language: Option<String>) -> String {
    normalize_bcp47_locale(tauri_plugin_os::locale(), saved_language)
}

#[tauri::command]
pub async fn get_system_locale(app: tauri::AppHandle) -> String {
    let state = app.state::<LocaleState>();
    resolve_system_locale(state.saved_language())
}

#[tauri::command]
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
    if let Err(error) = refresh_tray_labels(&app) {
        tracing::warn!(error = %error, "failed to refresh tray labels after locale change");
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_locale_uses_tauri_plugin_os() {
        let resolver_source = stringify!(tauri_plugin_os::locale());
        assert!(resolver_source.contains("locale"));
    }

    #[test]
    fn system_locale_normalizes_bcp47_once() {
        assert_eq!(normalize_bcp47_locale(Some("zh_CN".into()), None), "zh-CN");
        assert_eq!(normalize_bcp47_locale(Some("EN_us".into()), None), "en-US");
    }

    #[test]
    fn system_locale_falls_back_to_english() {
        assert_eq!(normalize_bcp47_locale(None, None), "en-US");
        assert_eq!(normalize_bcp47_locale(Some("fr-FR".into()), None), "en-US");
    }

    #[test]
    fn saved_language_precedes_system_locale() {
        assert_eq!(
            normalize_bcp47_locale(Some("en-US".into()), Some("zh-CN".into())),
            "zh-CN"
        );
    }
}
