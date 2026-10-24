//! 把服务层交付的皮肤目录转换为界面持有的原生预览图片。

use std::sync::Arc;

use loki_metris_service::skins::SkinDescriptor;
use loki_metris_service::usage_service::{SkinCatalogEntry, SkinPreviewFormat};

/// 界面持有的目录条目；预览已转换为 GPUI 原生图片，驻留与退役按图片 ID 管理。
pub(crate) struct SkinEntry {
    /// 皮肤描述。
    pub skin: SkinDescriptor,
    /// 原生预览图；无预览或解码失败时为 None。
    pub preview: Option<Arc<gpui_kit::Image>>,
}

/// 逐项包装预览字节；`Image::from_bytes` 会对全部字节求哈希，调用方须在后台执行器上运行。
pub(crate) fn entries_from_catalog(entries: Vec<SkinCatalogEntry>) -> Vec<SkinEntry> {
    entries
        .into_iter()
        .map(|entry| SkinEntry {
            skin: entry.skin,
            preview: entry.preview.map(|preview| {
                let format = match preview.format {
                    SkinPreviewFormat::Png => gpui_kit::ImageFormat::Png,
                    SkinPreviewFormat::Jpeg => gpui_kit::ImageFormat::Jpeg,
                };
                Arc::new(gpui_kit::Image::from_bytes(format, preview.bytes))
            }),
        })
        .collect()
}
