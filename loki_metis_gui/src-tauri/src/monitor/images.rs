//! 本机监控图片库，不依赖局域网设备。

use std::path::{Path, PathBuf};

use loki_metis_core::HookError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 一张本机监控图片的元数据。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorImageRecord {
    /// 稳定 ID。
    pub id: String,
    /// 原始文件名。
    pub filename: String,
    /// 相对应用数据目录的存储路径。
    pub stored_name: String,
}

/// 图片库目录。
fn images_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("monitor-images")
}

/// 元数据文件。
fn index_path(app_data_dir: &Path) -> PathBuf {
    images_dir(app_data_dir).join("index.json")
}

/// 读取图片索引。
fn load_index(app_data_dir: &Path) -> Result<Vec<MonitorImageRecord>, HookError> {
    let path = index_path(app_data_dir);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|error| HookError::new("error.monitor.imagesReadFailed").param("detail", error.to_string()))?;
    serde_json::from_str(&raw)
        .map_err(|error| HookError::new("error.monitor.imagesInvalid").param("detail", error.to_string()))
}

/// 保存图片索引。
fn save_index(app_data_dir: &Path, records: &[MonitorImageRecord]) -> Result<(), HookError> {
    std::fs::create_dir_all(images_dir(app_data_dir))
        .map_err(|error| HookError::new("error.monitor.imagesWriteFailed").param("detail", error.to_string()))?;
    let raw = serde_json::to_string_pretty(records)
        .map_err(|error| HookError::new("error.monitor.imagesWriteFailed").param("detail", error.to_string()))?;
    std::fs::write(index_path(app_data_dir), raw)
        .map_err(|error| HookError::new("error.monitor.imagesWriteFailed").param("detail", error.to_string()))
}

/// 列出本机监控图片。
pub fn list_monitor_images(app_data_dir: &Path) -> Result<Vec<MonitorImageRecord>, HookError> {
    load_index(app_data_dir)
}

/// 读取一张本机监控图片的原始字节。
pub fn read_monitor_image(app_data_dir: &Path, id: &str) -> Result<Vec<u8>, HookError> {
    let records = load_index(app_data_dir)?;
    let Some(record) = records.iter().find(|item| item.id == id) else {
        return Err(HookError::new("error.monitor.imageNotFound"));
    };
    std::fs::read(images_dir(app_data_dir).join(&record.stored_name))
        .map_err(|error| HookError::new("error.monitor.imagesReadFailed").param("detail", error.to_string()))
}

/// 保存一张本机监控图片。
pub fn save_monitor_image(
    app_data_dir: &Path,
    filename: &str,
    bytes: &[u8],
) -> Result<MonitorImageRecord, HookError> {
    if bytes.is_empty() {
        return Err(HookError::new("error.monitor.imageEmpty"));
    }
    let id = Uuid::new_v4().to_string();
    let extension = Path::new(filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("bin");
    let stored_name = format!("{id}.{extension}");
    std::fs::create_dir_all(images_dir(app_data_dir))
        .map_err(|error| HookError::new("error.monitor.imagesWriteFailed").param("detail", error.to_string()))?;
    std::fs::write(images_dir(app_data_dir).join(&stored_name), bytes)
        .map_err(|error| HookError::new("error.monitor.imagesWriteFailed").param("detail", error.to_string()))?;
    let record = MonitorImageRecord {
        id,
        filename: filename.to_owned(),
        stored_name,
    };
    let mut records = load_index(app_data_dir)?;
    records.push(record.clone());
    save_index(app_data_dir, &records)?;
    Ok(record)
}

/// 删除一张本机监控图片。
pub fn delete_monitor_image(app_data_dir: &Path, id: &str) -> Result<(), HookError> {
    let mut records = load_index(app_data_dir)?;
    let Some(index) = records.iter().position(|item| item.id == id) else {
        return Err(HookError::new("error.monitor.imageNotFound"));
    };
    let removed = records.remove(index);
    let _ = std::fs::remove_file(images_dir(app_data_dir).join(removed.stored_name));
    save_index(app_data_dir, &records)
}
