//! 独立本机图库驱动：索引原子提交，档案引用与删除共用事务锁。

use loki_metris_core::{
    HookError, MonitorImageCatalog, assemble_image_catalog, ensure_monitor_image_unused,
    metadata_from_bytes,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};
use uuid::Uuid;

/// 单张原图资源上限；读写采用同一边界，避免成功写入后无法读回。
pub(crate) const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
/// 完整图库原始字节的单次快照预算，控制原图驻留；不包含解码帧或 GPU 资源。
const MAX_GALLERY_BYTES: u64 = 128 * 1024 * 1024;
/// JSON 索引的读写共同上限。
const MAX_INDEX_BYTES: u64 = 1024 * 1024;

/// 图片原名仅供显示，存储名必须是图库内单个普通图片文件。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct MonitorImageRecord {
    id: String,
    filename: String,
    stored_name: String,
}

/// 同一锁内读出的领域元数据和原始资源；原生快照明确不包含 data URL。
#[derive(Clone, Debug)]
pub(crate) struct MonitorGallerySnapshot {
    pub gallery: MonitorImageCatalog,
    pub bytes: HashMap<String, Arc<[u8]>>,
    pub unavailable: Vec<UnavailableMonitorImage>,
}

/// 索引仍有记录但资源缺失或损坏时保留可清理条目，不阻断其他图库操作。
#[derive(Clone, Debug)]
pub(crate) struct UnavailableMonitorImage {
    pub id: String,
    pub filename: String,
    pub format: loki_metris_core::ImageFormat,
    pub error: HookError,
}

/// 导入回执明确携带本次新增 ID，不能在并发图库中按文件名猜测绑定项。
#[derive(Clone, Debug)]
pub(crate) struct MonitorImageImport {
    pub snapshot: MonitorGallerySnapshot,
    pub id: String,
}

/// 应用私有图库路径，不使用原版目录或原始文件名拼接。
fn images_dir(data: &Path) -> PathBuf {
    data.join("monitor-images")
}

/// 图库目录本身不能重定向到外部；首次读取缺失目录按空库处理。
fn ensure_directory(data: &Path, create: bool) -> Result<(), HookError> {
    let directory = images_dir(data);
    match std::fs::symlink_metadata(&directory) {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(HookError::new("error.monitor.imagesInvalid")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if create {
                std::fs::create_dir_all(directory)
                    .map_err(|_| HookError::new("error.monitor.imagesWriteFailed"))?;
            }
            Ok(())
        }
        Err(_) => Err(HookError::new("error.monitor.imagesReadFailed")),
    }
}

/// 索引字段不能携带目录、平台分隔符、别名或重复身份。
fn validate_records(records: &[MonitorImageRecord]) -> Result<(), HookError> {
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for record in records {
        let path = Path::new(&record.stored_name);
        let extension = path.extension().and_then(|value| value.to_str());
        if record.id.is_empty()
            || record.id.len() > 128
            || record.filename.len() > 4096
            || record.stored_name.contains(['\\', ':'])
            || path.components().count() != 1
            || !matches!(
                path.components().next(),
                Some(std::path::Component::Normal(_))
            )
            || !matches!(extension, Some("jpg" | "png" | "gif"))
            || !ids.insert(&record.id)
            || !names.insert(&record.stored_name)
        {
            return Err(HookError::new("error.monitor.imagesInvalid"));
        }
    }
    Ok(())
}

/// 调用方已持有目录事务锁；不存在的索引为空，损坏索引不得覆盖。
fn load_index(data: &Path) -> Result<Vec<MonitorImageRecord>, HookError> {
    ensure_directory(data, false)?;
    let path = images_dir(data).join("index.json");
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(HookError::new("error.monitor.imagesReadFailed")),
        Ok(_) => {}
    }
    let raw = super::catalog::read_regular(&path, MAX_INDEX_BYTES, "error.monitor.imagesInvalid")?;
    let records: Vec<MonitorImageRecord> =
        serde_json::from_slice(&raw).map_err(|_| HookError::new("error.monitor.imagesInvalid"))?;
    validate_records(&records)?;
    Ok(records)
}

/// 写入前验证同一索引边界，再用同目录临时文件原子提交。
fn save_index(data: &Path, records: &[MonitorImageRecord]) -> Result<(), HookError> {
    validate_records(records)?;
    let bytes = serde_json::to_vec_pretty(records)
        .map_err(|_| HookError::new("error.monitor.imagesWriteFailed"))?;
    if bytes.len() as u64 > MAX_INDEX_BYTES {
        return Err(HookError::new("error.monitor.imagesInvalid"));
    }
    super::atomic_file::write_monitor_file_atomically(
        &images_dir(data).join("index.json"),
        &bytes,
        "error.monitor.imagesWriteFailed",
    )
}

/// 已持有图库许可时解析图片引用；不把缺失图片默认为新 ID。
pub(super) fn monitor_image_ids_unlocked(data: &Path) -> Result<HashSet<String>, HookError> {
    Ok(load_index(data)?
        .into_iter()
        .map(|record| record.id)
        .collect())
}

/// 从已校验索引读取一致原始资源并交 core 识别格式和计数。
fn snapshot_from_records(
    data: &Path,
    records: &[MonitorImageRecord],
) -> Result<MonitorGallerySnapshot, HookError> {
    let directory = images_dir(data);
    let mut total = 0u64;
    let mut metadata = Vec::new();
    let mut bytes = HashMap::new();
    let mut unavailable = Vec::new();
    for record in records {
        let result = super::catalog::read_regular(
            &directory.join(&record.stored_name),
            MAX_IMAGE_BYTES,
            "error.monitor.imagesReadFailed",
        )
        .and_then(|raw| {
            if total.saturating_add(raw.len() as u64) > MAX_GALLERY_BYTES {
                return Err(HookError::new("error.monitor.imagesTooLarge"));
            }
            let entry = metadata_from_bytes(&record.id, &record.filename, &raw)?;
            Ok((raw, entry))
        });
        match result {
            Ok((raw, entry)) => {
                total += raw.len() as u64;
                metadata.push(entry);
                bytes.insert(record.id.clone(), Arc::from(raw));
            }
            Err(error) => unavailable.push(UnavailableMonitorImage {
                id: record.id.clone(),
                filename: record.filename.clone(),
                error,
                format: match Path::new(&record.stored_name)
                    .extension()
                    .and_then(|value| value.to_str())
                {
                    Some("gif") => loki_metris_core::ImageFormat::Gif,
                    Some("png") => loki_metris_core::ImageFormat::Png,
                    _ => loki_metris_core::ImageFormat::Jpeg,
                },
            }),
        }
    }
    Ok(MonitorGallerySnapshot {
        gallery: assemble_image_catalog(metadata),
        bytes,
        unavailable,
    })
}

/// 完整快照读取不会与导入、删除或档案保存穿插。
pub(crate) fn list_monitor_images(data: &Path) -> Result<MonitorGallerySnapshot, HookError> {
    let _guard = super::catalog::read_lock()?;
    snapshot_from_records(data, &load_index(data)?)
}

/// 在新图片与索引都提交后返回准确 ID；索引失败清理本次新文件。
pub(crate) fn save_monitor_image(
    data: &Path,
    filename: &str,
    bytes: &[u8],
) -> Result<MonitorImageImport, HookError> {
    save_monitor_image_with_commit(data, filename, bytes, save_index)
}

/// 索引提交边界显式注入，回归可验证新文件提交后失败时的真实回滚。
fn save_monitor_image_with_commit(
    data: &Path,
    filename: &str,
    bytes: &[u8],
    commit: impl FnOnce(&Path, &[MonitorImageRecord]) -> Result<(), HookError>,
) -> Result<MonitorImageImport, HookError> {
    let _guard = super::catalog::write_lock()?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(HookError::new("error.monitor.imagesTooLarge"));
    }
    let id = Uuid::new_v4().to_string();
    let entry = metadata_from_bytes(&id, filename, bytes)?;
    let mut records = load_index(data)?;
    let mut snapshot = snapshot_from_records(data, &records)?;
    // 旧索引已超预算时先允许清理，不能排除超额项后继续承诺新导入成功。
    if snapshot
        .unavailable
        .iter()
        .any(|image| image.error.code == "error.monitor.imagesTooLarge")
    {
        return Err(HookError::new("error.monitor.imagesTooLarge"));
    }
    let total = snapshot
        .bytes
        .values()
        .map(|value| value.len() as u64)
        .sum::<u64>();
    if total + bytes.len() as u64 > MAX_GALLERY_BYTES {
        return Err(HookError::new("error.monitor.imagesTooLarge"));
    }
    ensure_directory(data, true)?;
    let name = format!("{id}.{}", entry.format.extension());
    let path = images_dir(data).join(&name);
    let mut temp = tempfile::NamedTempFile::new_in(images_dir(data))
        .map_err(|_| HookError::new("error.monitor.imagesWriteFailed"))?;
    temp.write_all(bytes)
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|_| HookError::new("error.monitor.imagesWriteFailed"))?;
    temp.persist_noclobber(&path)
        .map_err(|_| HookError::new("error.monitor.imagesWriteFailed"))?;
    records.push(MonitorImageRecord {
        id: id.clone(),
        filename: filename.to_owned(),
        stored_name: name,
    });
    if let Err(error) = commit(data, &records) {
        let _ = std::fs::remove_file(&path);
        return Err(error);
    }
    snapshot.bytes.insert(id.clone(), Arc::from(bytes));
    snapshot.gallery.images.push(entry);
    snapshot.gallery = assemble_image_catalog(snapshot.gallery.images);
    Ok(MonitorImageImport { snapshot, id })
}

/// 原生选择器给出路径后，在后台有界读取原文件；只复制，不修改原文件。
pub(crate) fn import_monitor_image(
    data: &Path,
    path: &Path,
) -> Result<MonitorImageImport, HookError> {
    let bytes =
        super::catalog::read_regular(path, MAX_IMAGE_BYTES, "error.monitor.imagesReadFailed")?;
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| HookError::new("error.monitor.imagesInvalid"))?;
    save_monitor_image(data, filename, &bytes)
}

/// 所有档案引用检查和索引提交在同一许可内，失败保持原库和图片。
pub(crate) fn delete_monitor_image(
    config: &Path,
    data: &Path,
    id: &str,
) -> Result<MonitorGallerySnapshot, HookError> {
    let _guard = super::catalog::write_lock()?;
    let profiles = super::profiles::load_profile_drafts_unlocked(config)?;
    ensure_monitor_image_unused(&profiles, id)?;
    let mut records = load_index(data)?;
    let index = records
        .iter()
        .position(|record| record.id == id)
        .ok_or_else(|| HookError::new("error.monitor.imageNotFound"))?;
    let removed = records.remove(index);
    let path = images_dir(data).join(removed.stored_name);
    if std::fs::symlink_metadata(&path).is_ok_and(|meta| !meta.is_file()) {
        return Err(HookError::new("error.monitor.imagesInvalid"));
    }
    let snapshot = snapshot_from_records(data, &records)?;
    save_index(data, &records)?;
    if let Err(error) = std::fs::remove_file(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(event = "monitor_image_orphan_cleanup_failed");
        }
    }
    Ok(snapshot)
}

/// 桌宠单图读取通过已校验索引，不接受外部路径。
pub(crate) fn read_monitor_image(data: &Path, id: &str) -> Result<Arc<[u8]>, HookError> {
    let _guard = super::catalog::read_lock()?;
    let records = load_index(data)?;
    let record = records
        .iter()
        .find(|record| record.id == id)
        .ok_or_else(|| HookError::new("error.monitor.imageNotFound"))?;
    super::catalog::read_regular(
        &images_dir(data).join(&record.stored_name),
        MAX_IMAGE_BYTES,
        "error.monitor.imagesReadFailed",
    )
    .map(Arc::from)
}

#[cfg(test)]
#[path = "images_tests.rs"]
mod tests;
