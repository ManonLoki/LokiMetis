//! 本机监控图库：图片格式识别、计数与预览 data URL。
//!
//! 不依赖设备 ID 或远端传输；非法格式在进入存储前被拒绝。

use serde::{Deserialize, Serialize};

use crate::HookError;

/// 本机图库允许持久化的格式；枚举与映射在此登记，魔数识别统一维护于 pairing 的共享表。
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ImageFormat {
    /// JPEG。
    Jpeg,
    /// PNG。
    Png,
    /// GIF（含动图）。
    Gif,
    /// BMP。
    Bmp,
    /// WebP（含动图）。
    Webp,
}

impl ImageFormat {
    /// 全部已支持格式，顺序即 GUI 筛选顺序。
    pub const ALL: [Self; 5] = [Self::Jpeg, Self::Png, Self::Gif, Self::Bmp, Self::Webp];

    /// 从文件魔数识别格式；无法识别时返回 `None`。
    ///
    /// 魔数表只在 pairing 登记一份，本机图库与设备传输对同一字节的判定始终一致。
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let mime = loki_metris_pairing::assets::sniff_image_mime(bytes)?;
        Self::ALL
            .into_iter()
            .find(|format| format.mime_type() == mime)
    }

    /// 规范 MIME 类型，供 data URL 与 `<input accept>` 共用。
    pub const fn mime_type(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::Gif => "image/gif",
            Self::Bmp => "image/bmp",
            Self::Webp => "image/webp",
        }
    }

    /// 存储扩展名，不含点。
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Gif => "gif",
            Self::Bmp => "bmp",
            Self::Webp => "webp",
        }
    }

    /// 由存储扩展名反查格式，与 [`Self::extension`] 互逆。
    pub fn from_extension(extension: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|format| format.extension() == extension)
    }

    /// 界面展示的格式名称，属于品牌名而非翻译文案。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Jpeg => "JPEG",
            Self::Png => "PNG",
            Self::Gif => "GIF",
            Self::Bmp => "BMP",
            Self::Webp => "WebP",
        }
    }
}

/// 原生图库的已识别元数据；原始字节由宿主独立持有，不生成展示用编码字符串。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MonitorImageMetadata {
    /// 稳定 ID。
    pub id: String,
    /// 原始文件名。
    pub filename: String,
    /// 由字节魔数判定的格式。
    pub format: ImageFormat,
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
    /// BMP 张数。
    pub bmp: usize,
    /// WebP 张数。
    pub webp: usize,
}

impl MonitorImageCounts {
    /// 空库计数。
    pub const fn empty() -> Self {
        Self {
            jpeg: 0,
            png: 0,
            gif: 0,
            bmp: 0,
            webp: 0,
        }
    }
}

/// 原生图库快照只有条目元数据和格式计数，不承担图片编码或解码。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MonitorImageCatalog {
    /// 元数据条目，顺序与调用方提供的一致。
    pub images: Vec<MonitorImageMetadata>,
    /// 各已支持格式的独立计数。
    pub counts: MonitorImageCounts,
}

/// 从已识别元数据统计原生图库；空输入得到空库与零计数。
pub fn assemble_image_catalog(images: Vec<MonitorImageMetadata>) -> MonitorImageCatalog {
    let mut counts = MonitorImageCounts::empty();
    for item in &images {
        match item.format {
            ImageFormat::Jpeg => counts.jpeg += 1,
            ImageFormat::Png => counts.png += 1,
            ImageFormat::Gif => counts.gif += 1,
            ImageFormat::Bmp => counts.bmp += 1,
            ImageFormat::Webp => counts.webp += 1,
        }
    }
    MonitorImageCatalog { images, counts }
}

/// 只识别格式和构造元数据；错误与完整预览相同，不生成 Base64 或复制图片字节。
pub fn metadata_from_bytes(
    id: impl Into<String>,
    filename: impl Into<String>,
    bytes: &[u8],
) -> Result<MonitorImageMetadata, HookError> {
    if bytes.is_empty() {
        return Err(HookError::new("error.monitor.imageEmpty"));
    }
    let Some(format) = ImageFormat::from_bytes(bytes) else {
        return Err(HookError::new("error.monitor.imageUnsupportedType"));
    };
    Ok(MonitorImageMetadata {
        id: id.into(),
        filename: filename.into(),
        format,
    })
}

/// 生成 CSP 允许的 data URL。
pub fn image_data_url(format: ImageFormat, bytes: &[u8]) -> String {
    format!(
        "data:{};base64,{}",
        format.mime_type(),
        encode_base64(bytes)
    )
}

/// 使用固定标准字母表把图片字节编码为带正确补位的 Base64 文本。
fn encode_base64(input: &[u8]) -> String {
    #[cfg(test)]
    ENCODE_CALLS.with(|calls| calls.set(calls.get() + 1));
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
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
std::thread_local! {
    /// 当前测试线程的编码次数，避免并行测试污染原生元数据路径的零编码断言。
    static ENCODE_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
#[path = "gallery_native_tests.rs"]
mod native_metadata_tests;

#[cfg(test)]
mod tests {
    use super::{ImageFormat, assemble_image_catalog, image_data_url, metadata_from_bytes};

    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00];
    const GIF: &[u8] = b"GIF89a\x01\x00";
    const WEBP: &[u8] = b"RIFF....WEBP";
    const BMP: &[u8] = b"BM....";

    /// 各受支持格式按魔数识别、独立计数，并生成匹配 MIME 和扩展名。
    #[test]
    fn catalog_counts_every_format_and_data_url_uses_mime() {
        let jpeg = metadata_from_bytes("j1", "a.jpg", JPEG).expect("jpeg");
        let png = metadata_from_bytes("p1", "b.png", PNG).expect("png");
        let gif = metadata_from_bytes("g1", "c.gif", GIF).expect("gif");
        let bmp = metadata_from_bytes("b1", "d.bmp", BMP).expect("bmp");
        let webp = metadata_from_bytes("w1", "e.webp", WEBP).expect("webp");
        assert_eq!(jpeg.format, ImageFormat::Jpeg);
        assert_eq!(png.format, ImageFormat::Png);
        assert_eq!(gif.format, ImageFormat::Gif);
        assert_eq!(bmp.format, ImageFormat::Bmp);
        assert_eq!(webp.format, ImageFormat::Webp);
        assert_eq!(bmp.format.extension(), "bmp");
        assert_eq!(webp.format.extension(), "webp");
        assert!(image_data_url(jpeg.format, JPEG).starts_with("data:image/jpeg;base64,"));
        assert!(image_data_url(png.format, PNG).starts_with("data:image/png;base64,"));
        assert!(image_data_url(gif.format, GIF).starts_with("data:image/gif;base64,"));
        assert!(image_data_url(bmp.format, BMP).starts_with("data:image/bmp;base64,"));
        assert!(image_data_url(webp.format, WEBP).starts_with("data:image/webp;base64,"));
        let catalog = assemble_image_catalog(vec![jpeg, png, gif, bmp, webp]);
        assert_eq!(catalog.counts.jpeg, 1);
        assert_eq!(catalog.counts.png, 1);
        assert_eq!(catalog.counts.gif, 1);
        assert_eq!(catalog.counts.bmp, 1);
        assert_eq!(catalog.counts.webp, 1);
        assert_eq!(catalog.images.len(), 5);
    }

    /// 验证空输入生成无条目且各格式计数均为零的目录。
    #[test]
    fn empty_catalog_has_zero_counts() {
        let catalog = assemble_image_catalog(Vec::new());
        assert!(catalog.images.is_empty());
        assert_eq!(catalog.counts.jpeg, 0);
        assert_eq!(catalog.counts.png, 0);
        assert_eq!(catalog.counts.gif, 0);
        assert_eq!(catalog.counts.bmp, 0);
        assert_eq!(catalog.counts.webp, 0);
    }

    /// 扩展名反查覆盖全部格式，未登记扩展名不回退为任何格式。
    #[test]
    fn extension_roundtrips_for_every_format() {
        for format in ImageFormat::ALL {
            assert_eq!(
                ImageFormat::from_extension(format.extension()),
                Some(format)
            );
        }
        assert_eq!(ImageFormat::from_extension("jpeg"), None);
    }

    /// 验证空字节和不受支持的文件魔数会返回各自的稳定错误。
    #[test]
    fn illegal_format_and_empty_bytes_are_rejected() {
        for bytes in [
            b"RIFF....WAVE".as_slice(),
            b"RIFF....WEB",
            b"B",
            b"arbitrary",
        ] {
            let unsupported =
                metadata_from_bytes("bad", "renamed.webp", bytes).expect_err("unsupported");
            assert_eq!(unsupported.code, "error.monitor.imageUnsupportedType");
        }
        let empty = metadata_from_bytes("e1", "empty.png", &[]).expect_err("empty");
        assert_eq!(empty.code, "error.monitor.imageEmpty");
    }
}
