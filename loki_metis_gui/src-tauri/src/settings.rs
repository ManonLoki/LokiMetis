use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

const HOST_SETTINGS_SCHEMA_VERSION: u8 = 1;
const MAX_HOST_SETTINGS_BYTES: u64 = 4 * 1024;
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// 保存 GUI 宿主自身的窄范围持久设置。
pub(crate) struct HostSettingsDocument {
    schema_version: u8,
    interface_language: Option<String>,
    system_notification_enabled: bool,
}

impl Default for HostSettingsDocument {
    /// 返回当前 schema 版本且通知默认关闭的安全设置。
    fn default() -> Self {
        Self {
            schema_version: HOST_SETTINGS_SCHEMA_VERSION,
            interface_language: None,
            system_notification_enabled: false,
        }
    }
}

impl HostSettingsDocument {
    /// 返回已选择的界面语言。
    pub(crate) fn interface_language(&self) -> Option<String> {
        self.interface_language.clone()
    }

    /// 返回系统通知是否已启用。
    pub(crate) fn system_notification_enabled(&self) -> bool {
        self.system_notification_enabled
    }
}

/// 串行处理 GUI 宿主设置文件的全部读取和修改。
pub(crate) struct HostSettingsState {
    path: PathBuf,
    operation: Mutex<()>,
}

impl HostSettingsState {
    /// 为固定设置路径创建串行状态容器。
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path,
            operation: Mutex::new(()),
        }
    }

    /// 读取有效设置，缺失或无效时回退到安全默认值。
    pub(crate) async fn read(&self) -> HostSettingsDocument {
        let _guard = self.operation.lock().await;
        read_document(&self.path).await.unwrap_or_default()
    }

    /// 串行更新并持久化界面语言。
    pub(crate) async fn set_interface_language(
        &self,
        language: String,
    ) -> Result<(), &'static str> {
        let _guard = self.operation.lock().await;
        let mut document = read_document(&self.path).await.unwrap_or_default();
        document.interface_language = Some(language);
        write_document(&self.path, &document).await
    }

    /// 串行更新并持久化系统通知开关。
    pub(crate) async fn set_system_notification_enabled(
        &self,
        enabled: bool,
    ) -> Result<(), &'static str> {
        let _guard = self.operation.lock().await;
        let mut document = read_document(&self.path).await.unwrap_or_default();
        document.system_notification_enabled = enabled;
        write_document(&self.path, &document).await
    }
}

/// 从普通且受大小限制的文件读取宿主设置文档。
async fn read_document(path: &Path) -> Result<HostSettingsDocument, &'static str> {
    let metadata = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(HostSettingsDocument::default());
        }
        Err(_) => return Err("host-settings-unavailable"),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("host-settings-unavailable");
    }
    if metadata.len() > MAX_HOST_SETTINGS_BYTES {
        return Err("host-settings-invalid");
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|_| "host-settings-unavailable")?;
    decode_document(&bytes)
}

/// 解码并验证设置 schema、大小与受支持语言。
fn decode_document(bytes: &[u8]) -> Result<HostSettingsDocument, &'static str> {
    if bytes.len() as u64 > MAX_HOST_SETTINGS_BYTES {
        return Err("host-settings-invalid");
    }
    let document: HostSettingsDocument =
        serde_json::from_slice(bytes).map_err(|_| "host-settings-invalid")?;
    if document.schema_version != HOST_SETTINGS_SCHEMA_VERSION
        || document
            .interface_language
            .as_deref()
            .is_some_and(|language| !matches!(language, "zh-CN" | "en-US"))
    {
        return Err("host-settings-invalid");
    }
    Ok(document)
}

/// 通过同目录临时文件原子替换宿主设置文档。
async fn write_document(path: &Path, document: &HostSettingsDocument) -> Result<(), &'static str> {
    let bytes = serde_json::to_vec(document).map_err(|_| "host-settings-write-failed")?;
    if bytes.len() as u64 > MAX_HOST_SETTINGS_BYTES {
        return Err("host-settings-write-failed");
    }
    let parent = path.parent().ok_or("host-settings-write-failed")?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|_| "host-settings-write-failed")?;
    if tokio::fs::symlink_metadata(parent)
        .await
        .map_err(|_| "host-settings-write-failed")?
        .file_type()
        .is_symlink()
    {
        return Err("host-settings-write-failed");
    }
    if let Ok(metadata) = tokio::fs::symlink_metadata(path).await
        && (metadata.file_type().is_symlink() || !metadata.is_file())
    {
        return Err("host-settings-write-failed");
    }

    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp_name = format!(
        ".loki-metis-host-settings.{}.{}.tmp",
        std::process::id(),
        sequence
    );
    let temp_path = parent.join(temp_name);
    if tokio::fs::symlink_metadata(&temp_path).await.is_ok() {
        return Err("host-settings-write-failed");
    }
    tokio::fs::write(&temp_path, &bytes)
        .await
        .map_err(|_| "host-settings-write-failed")?;
    if tokio::fs::rename(&temp_path, path).await.is_err() {
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err("host-settings-write-failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 新设置应默认关闭系统通知。
    #[test]
    fn system_notification_defaults_disabled() {
        assert!(!HostSettingsDocument::default().system_notification_enabled());
    }

    /// 未知字段或超过大小上限的设置文档应被拒绝。
    #[test]
    fn rejects_unknown_or_oversized_host_settings() {
        let unknown = br#"{"schemaVersion":1,"interfaceLanguage":null,"systemNotificationEnabled":false,"extra":true}"#;
        assert_eq!(decode_document(unknown), Err("host-settings-invalid"));
        assert_eq!(
            decode_document(&vec![b' '; MAX_HOST_SETTINGS_BYTES as usize + 1]),
            Err("host-settings-invalid")
        );
    }
}
