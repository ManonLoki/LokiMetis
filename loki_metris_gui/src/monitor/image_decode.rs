//! 原生图片资源解码：保留 GIF 多帧、透明度和 JPEG 方向，并限制解码资源。

use gpui_kit::{App, Asset, DevicePixels, RenderImage, Size, size};
use image::{AnimationDecoder, ImageDecoder, Limits};
use loki_metris_core::ImageFormat;
use std::{
    hash::{Hash, Hasher},
    io::Cursor,
    sync::Arc,
};

/// 单幅严格宽高上限。
pub(super) const MAX_DIMENSION: u32 = 8192;
/// 解码器内部工作内存的 best-effort 预算，不等同进程峰值。
pub(super) const WORK_BYTES: u64 = 64 * 1024 * 1024;
/// GIF 已保留帧像素预算，另有一帧在解码器内部生成的临时占用。
pub(super) const RETAINED_BYTES: u64 = 128 * 1024 * 1024;
/// 防止极小图片通过极多帧耗尽对象和定时器资源。
pub(super) const MAX_FRAMES: usize = 2048;

/// UUID 图片副本不可变；资源缓存以稳定 ID 标识，字节跨帧共享。
#[derive(Clone)]
pub(crate) struct MonitorImageSource {
    pub id: String,
    pub bytes: Arc<[u8]>,
    pub format: ImageFormat,
}
impl Hash for MonitorImageSource {
    /// ID 由实际导入回执产生，相同 ID 的原图在整个图库生命周期保持不变。
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

/// 自有 Asset 保留未加载/失败/成功三态，成功结果可直接交 img 渲染。
pub(crate) struct MonitorImageDecoder;
/// 解码像素与浏览器语义的总播放次数；None 表示无限循环。
pub(crate) struct DecodedMonitorImage {
    pub image: Arc<RenderImage>,
    /// 纹理含复制边缘保护像素，布局仍使用未经扩展的天然尺寸。
    pub natural_size: Size<DevicePixels>,
    pub plays: Option<u32>,
}
impl Asset for MonitorImageDecoder {
    type Source = MonitorImageSource;
    type Output = Result<Arc<DecodedMonitorImage>, &'static str>;
    /// GPUI 资源 owner 在后台解码；UI render 不执行图像编解码。
    fn load(
        source: Self::Source,
        cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        cx.background_executor()
            .spawn(async move { decode(&source).map_err(|_| "error.monitor.imageDecodeFailed") })
    }
}

/// 每个 decoder 都先配置严格尺寸与工作内存，再实际产生像素。
fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(WORK_BYTES);
    limits
}

/// 逐帧累计保留内存；静态图方向与 GPUI 默认解码一致，最终转换为 BGRA。
fn decode(source: &MonitorImageSource) -> anyhow::Result<Arc<DecodedMonitorImage>> {
    let mut plays = Some(1);
    let frames;
    if source.format == ImageFormat::Gif {
        let mut decoder = image::codecs::gif::GifDecoder::new(Cursor::new(&*source.bytes))?;
        decoder.set_limits(limits())?;
        plays = super::animation_metadata::gif_plays(&source.bytes)?;
        frames = animation_frames(decoder.into_frames())?;
    } else if source.format == ImageFormat::Png {
        let decoder =
            image::codecs::png::PngDecoder::with_limits(Cursor::new(&*source.bytes), limits())?;
        if decoder.is_apng()? {
            drop(decoder);
            (frames, plays) = super::png_animation::decode(&source.bytes)?;
        } else {
            frames = vec![static_frame(decoder)?];
        }
    } else {
        let format = match source.format {
            ImageFormat::Png => image::ImageFormat::Png,
            _ => image::ImageFormat::Jpeg,
        };
        let mut reader = image::ImageReader::with_format(Cursor::new(&*source.bytes), format);
        reader.limits(limits());
        frames = vec![static_frame(reader.into_decoder()?)?];
    }
    if frames.is_empty() {
        anyhow::bail!("monitor image has no frames")
    }
    let natural_size = size(
        DevicePixels(frames[0].buffer().width() as i32),
        DevicePixels(frames[0].buffer().height() as i32),
    );
    let frames = texture_frames(frames)?;
    Ok(Arc::new(DecodedMonitorImage {
        image: Arc::new(RenderImage::new(frames)),
        natural_size,
        plays,
    }))
}

/// GIF/APNG 均逐帧累计已保留内存，任何损坏帧都明确失败。
fn animation_frames(input: image::Frames<'_>) -> anyhow::Result<Vec<image::Frame>> {
    let mut frames = Vec::new();
    let mut retained = Limits::default();
    retained.max_alloc = Some(RETAINED_BYTES);
    for result in input {
        if frames.len() >= MAX_FRAMES {
            anyhow::bail!("monitor frame budget exceeded")
        }
        let mut frame = result?;
        retained.reserve_usize(frame.buffer().as_raw().len())?;
        bgra(frame.buffer_mut());
        frames.push(frame);
    }
    Ok(frames)
}

/// 静态图先验像素预算，再应用 EXIF 方向并转换通道。
fn static_frame(mut decoder: impl ImageDecoder) -> anyhow::Result<image::Frame> {
    let orientation = decoder.orientation()?;
    let (width, height) = decoder.dimensions();
    let rgba = u64::from(width) * u64::from(height) * 4;
    if width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || decoder.total_bytes().saturating_add(rgba) > RETAINED_BYTES
    {
        anyhow::bail!("monitor pixel budget exceeded")
    }
    let mut decoded = image::DynamicImage::from_decoder(decoder)?;
    decoded.apply_orientation(orientation);
    let mut rgba = decoded.into_rgba8();
    bgra(&mut rgba);
    Ok(image::Frame::new(rgba))
}

/// GPUI 纹理使用 BGRA，保留原 alpha 不做背景合成。
pub(super) fn bgra(rgba: &mut image::RgbaImage) {
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
}

/// GPUI atlas 无边距且采用线性采样，复制一圈边缘避免放大小图时混入透明或邻块。
fn texture_frames(frames: Vec<image::Frame>) -> anyhow::Result<Vec<image::Frame>> {
    let mut retained = 0u64;
    frames
        .into_iter()
        .map(|frame| {
            let delay = frame.delay();
            let source = frame.into_buffer();
            let (width, height) = source.dimensions();
            let (padded_width, padded_height) = (width + 2, height + 2);
            retained =
                retained.saturating_add(u64::from(padded_width) * u64::from(padded_height) * 4);
            if retained > RETAINED_BYTES {
                anyhow::bail!("monitor texture budget exceeded")
            }
            let padded = image::RgbaImage::from_fn(padded_width, padded_height, |x, y| {
                *source.get_pixel(
                    x.saturating_sub(1).min(width - 1),
                    y.saturating_sub(1).min(height - 1),
                )
            });
            Ok(image::Frame::from_parts(padded, 0, 0, delay))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 自有解码器必须保留真实两帧、延迟、尺寸和不同像素。
    #[test]
    fn animated_fixture_decodes_both_frames_and_delays() {
        let source = MonitorImageSource {
            id: "fixture".into(),
            format: ImageFormat::Gif,
            bytes: Arc::from(include_bytes!("../../tests/fixtures/monitor-loop.gif").as_slice()),
        };
        let decoded = decode(&source).unwrap();
        assert_eq!(decoded.plays, None);
        let image = &decoded.image;
        assert_eq!(image.frame_count(), 2);
        assert_eq!(decoded.natural_size, size(DevicePixels(1), DevicePixels(1)));
        assert_eq!(image.size(0).width.0, 3);
        assert_eq!(image.size(0).height.0, 3);
        assert_eq!(image.as_bytes(0).unwrap(), [0, 0, 0, 255].repeat(9));
        assert_eq!(image.as_bytes(1).unwrap(), [255, 255, 255, 255].repeat(9));
        assert_eq!(image.delay(0).numer_denom_ms(), (200, 1));
    }
    /// 超宽 GIF 在解码像素前拒绝；魔数合法但损坏的静态图也明确失败。
    #[test]
    fn oversized_and_corrupt_resources_fail_decoding() {
        let mut bytes = include_bytes!("../../tests/fixtures/monitor-loop.gif").to_vec();
        bytes[6] = 0xff;
        bytes[7] = 0xff;
        assert!(
            decode(&MonitorImageSource {
                id: "large".into(),
                bytes: Arc::from(bytes),
                format: ImageFormat::Gif
            })
            .is_err()
        );
        assert!(
            decode(&MonitorImageSource {
                id: "bad".into(),
                bytes: Arc::from(b"\x89PNG\r\n\x1a\n".as_slice()),
                format: ImageFormat::Png
            })
            .is_err()
        );
    }
    /// 实际应用 PNG 完整解码，图标预览不能由魔数占位数据通过。
    #[test]
    fn actual_png_has_natural_dimensions() {
        let source = MonitorImageSource {
            id: "logo".into(),
            bytes: Arc::from(include_bytes!("../../assets/logo.png").as_slice()),
            format: ImageFormat::Png,
        };
        let decoded = decode(&source).unwrap();
        let image = &decoded.image;
        assert_eq!(
            decoded.natural_size,
            size(DevicePixels(1024), DevicePixels(1024))
        );
        assert_eq!(image.frame_count(), 1);
    }

    /// APNG 是实际 PNG 动画，保留总播放次数、两帧像素和延迟。
    #[test]
    fn apng_fixture_preserves_animation_and_finite_plays() {
        let source = MonitorImageSource {
            id: "apng".into(),
            bytes: Arc::from(include_bytes!("../../tests/fixtures/monitor-finite.png").as_slice()),
            format: ImageFormat::Png,
        };
        let decoded = decode(&source).unwrap();
        assert_eq!(decoded.plays, Some(2));
        assert_eq!(decoded.image.frame_count(), 2);
        assert_eq!(decoded.image.as_bytes(0).unwrap(), [0, 0, 0, 255].repeat(9));
        assert_eq!(
            decoded.image.as_bytes(1).unwrap(),
            [255, 255, 255, 255].repeat(9)
        );
        assert_eq!(decoded.image.delay(0).numer_denom_ms(), (200, 1));
    }
}
