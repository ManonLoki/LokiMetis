//! 本机监控图库：JPEG/PNG/GIF 识别、计数与预览 data URL。
//!
//! 不依赖设备 ID 或远端传输；非法格式在进入存储前被拒绝。

use serde::{Deserialize, Serialize};

use crate::HookError;

/// 本机图库允许持久化的三种格式。
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ImageFormat {
    /// JPEG。
    Jpeg,
    /// PNG。
    Png,
    /// GIF（含动图）。
    Gif,
}

impl ImageFormat {
    /// 从文件魔数识别格式；无法识别时返回 `None`。
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
            return Some(Self::Jpeg);
        }
        if bytes.len() >= 8
            && bytes[0] == 0x89
            && bytes[1] == b'P'
            && bytes[2] == b'N'
            && bytes[3] == b'G'
            && bytes[4] == 0x0D
            && bytes[5] == 0x0A
            && bytes[6] == 0x1A
            && bytes[7] == 0x0A
        {
            return Some(Self::Png);
        }
        if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            return Some(Self::Gif);
        }
        None
    }

    /// 规范 MIME 类型，供 data URL 与 `<input accept>` 共用。
    pub const fn mime_type(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::Gif => "image/gif",
        }
    }

    /// 存储扩展名，不含点。
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Gif => "gif",
        }
    }
}

/// 前端文件选择器使用的 MIME 与扩展名清单。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImageUploadAccept {
    /// 允许的 MIME 类型。
    pub mime_types: Vec<String>,
    /// 允许的文件扩展名（含点）。
    pub extensions: Vec<String>,
}

/// 上传 accept 与格式识别共用的固定清单，只含本机可持久化的三种格式。
pub fn image_upload_accept() -> ImageUploadAccept {
    ImageUploadAccept {
        mime_types: vec![
            "image/jpeg".to_owned(),
            "image/png".to_owned(),
            "image/gif".to_owned(),
        ],
        extensions: vec![
            ".jpg".to_owned(),
            ".jpeg".to_owned(),
            ".png".to_owned(),
            ".gif".to_owned(),
        ],
    }
}

/// 一张已识别的本机图库条目，含预览 data URL。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MonitorImagePreview {
    /// 稳定 ID。
    pub id: String,
    /// 原始文件名。
    pub filename: String,
    /// 由字节魔数判定的格式。
    pub format: ImageFormat,
    /// CSP 允许的 `data:` 预览。
    pub image: String,
}

/// 按格式统计的图库数量。
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MonitorImageCounts {
    /// JPEG 张数。
    pub jpeg: usize,
    /// PNG 张数。
    pub png: usize,
    /// GIF 张数。
    pub gif: usize,
}

impl MonitorImageCounts {
    /// 空库计数。
    pub const fn empty() -> Self {
        Self {
            jpeg: 0,
            png: 0,
            gif: 0,
        }
    }
}

/// 本机图库快照：预览列表与格式计数。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MonitorImageGallery {
    /// 预览条目，顺序与调用方提供的一致。
    pub images: Vec<MonitorImagePreview>,
    /// JPEG/PNG/GIF 计数。
    pub counts: MonitorImageCounts,
}

/// 把已识别条目组装为图库快照；空输入得到空库与零计数。
pub fn assemble_image_gallery(images: Vec<MonitorImagePreview>) -> MonitorImageGallery {
    let mut counts = MonitorImageCounts::empty();
    for item in &images {
        match item.format {
            ImageFormat::Jpeg => counts.jpeg += 1,
            ImageFormat::Png => counts.png += 1,
            ImageFormat::Gif => counts.gif += 1,
        }
    }
    MonitorImageGallery { images, counts }
}

/// 用魔数识别格式并生成预览；空字节或非法格式失败。
pub fn preview_from_bytes(
    id: impl Into<String>,
    filename: impl Into<String>,
    bytes: &[u8],
) -> Result<MonitorImagePreview, HookError> {
    if bytes.is_empty() {
        return Err(HookError::new("error.monitor.imageEmpty"));
    }
    let Some(format) = ImageFormat::from_bytes(bytes) else {
        return Err(HookError::new("error.monitor.imageUnsupportedType"));
    };
    Ok(MonitorImagePreview {
        id: id.into(),
        filename: filename.into(),
        format,
        image: image_data_url(format, bytes),
    })
}

/// 生成 CSP 允许的 data URL。
pub fn image_data_url(format: ImageFormat, bytes: &[u8]) -> String {
    format!("data:{};base64,{}", format.mime_type(), encode_base64(bytes))
}

fn encode_base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let a = u32::from(chunk[0]);
        let b = u32::from(chunk.get(1).copied().unwrap_or(0));
        let c = u32::from(chunk.get(2).copied().unwrap_or(0));
        let triple = (a << 16) | (b << 8) | c;
        out.push(char::from(ALPHABET[((triple >> 18) & 0x3F) as usize]));
        out.push(char::from(ALPHABET[((triple >> 12) & 0x3F) as usize]));
        if chunk.len() > 1 {
            out.push(char::from(ALPHABET[((triple >> 6) & 0x3F) as usize]));
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(char::from(ALPHABET[(triple & 0x3F) as usize]));
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        ImageFormat, assemble_image_gallery, image_upload_accept, preview_from_bytes,
    };

    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00];
    const GIF: &[u8] = b"GIF89a\x01\x00";
    const WEBP: &[u8] = b"RIFF....WEBP";

    #[test]
    fn gallery_counts_jpeg_png_gif_and_encodes_preview() {
        let jpeg = preview_from_bytes("j1", "a.jpg", JPEG).expect("jpeg");
        let png = preview_from_bytes("p1", "b.png", PNG).expect("png");
        let gif = preview_from_bytes("g1", "c.gif", GIF).expect("gif");
        assert_eq!(jpeg.format, ImageFormat::Jpeg);
        assert_eq!(png.format, ImageFormat::Png);
        assert_eq!(gif.format, ImageFormat::Gif);
        assert!(jpeg.image.starts_with("data:image/jpeg;base64,"));
        assert!(png.image.starts_with("data:image/png;base64,"));
        assert!(gif.image.starts_with("data:image/gif;base64,"));
        let gallery = assemble_image_gallery(vec![jpeg, png, gif]);
        assert_eq!(gallery.counts.jpeg, 1);
        assert_eq!(gallery.counts.png, 1);
        assert_eq!(gallery.counts.gif, 1);
        assert_eq!(gallery.images.len(), 3);
    }

    #[test]
    fn empty_gallery_has_zero_counts() {
        let gallery = assemble_image_gallery(Vec::new());
        assert!(gallery.images.is_empty());
        assert_eq!(gallery.counts.jpeg, 0);
        assert_eq!(gallery.counts.png, 0);
        assert_eq!(gallery.counts.gif, 0);
    }

    #[test]
    fn illegal_format_and_empty_bytes_are_rejected() {
        let unsupported = preview_from_bytes("w1", "x.webp", WEBP).expect_err("webp");
        assert_eq!(unsupported.code, "error.monitor.imageUnsupportedType");
        let empty = preview_from_bytes("e1", "empty.png", &[]).expect_err("empty");
        assert_eq!(empty.code, "error.monitor.imageEmpty");
    }

    #[test]
    fn upload_accept_lists_only_jpeg_png_gif() {
        let accept = image_upload_accept();
        assert_eq!(
            accept.mime_types,
            ["image/jpeg", "image/png", "image/gif"]
        );
        assert_eq!(accept.extensions, [".jpg", ".jpeg", ".png", ".gif"]);
        assert!(!accept.mime_types.iter().any(|item| item.contains("webp")));
        assert!(!accept.extensions.iter().any(|item| item == ".bmp"));
    }
}
