//! 原生图片资源解码：支持五种图片、保留动画与透明度，并限制解码资源。

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
/// 动画已保留帧像素预算，另有一帧在解码器内部生成的临时占用。
pub(super) const RETAINED_BYTES: u64 = 128 * 1024 * 1024;
/// 防止极小图片通过极多帧耗尽对象和定时器资源。
pub(super) const MAX_FRAMES: usize = 2048;

/// UUID 图片副本不可变；资源缓存以稳定 ID 标识，字节跨帧共享。
#[derive(Clone)]
pub(crate) struct MonitorImageSource {
    /// 图库图片的稳定 ID，也是解码缓存的键。
    pub id: String,
    /// 不可变原始字节，跨位置共享。
    pub bytes: Arc<[u8]>,
    /// 图片格式。
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

/// 真实生命周期回归暂占两个解码许可，使生产 Asset 停在 pending 而不替换解码器。
#[cfg(test)]
pub(crate) fn hold_decoders_for_test(cx: &mut App) -> [tokio::sync::OwnedSemaphorePermit; 2] {
    super::image_memory::resources_for_app(cx).hold_decoders()
}

/// 真实失败重试回归用全额租约触发预算错误；释放后仍由原解码器正常加载。
#[cfg(test)]
pub(crate) fn hold_pixel_budget_for_test(cx: &mut App) -> impl Drop + use<> {
    super::image_memory::resources_for_app(cx)
        .reserve(512 * 1024 * 1024)
        .unwrap()
}

#[cfg(test)]
pub(crate) use load_observation::LoadProbe;

/// 测试仅计数正式 Asset::load 入口；不改变解码器、许可等待、后台任务或完成通知。
#[cfg(test)]
mod load_observation {
    use std::{collections::HashMap, sync::{Arc, Mutex, OnceLock, Weak, atomic::{AtomicUsize, Ordering}}};

    /// 每个真实图库 ID 独立观察，弱登记不会跨测试保留计数或资源。
    static PROBES: OnceLock<Mutex<HashMap<String, Weak<AtomicUsize>>>> = OnceLock::new();

    /// 启动次数直接区分同一个 pending Asset 与被取消后立即重建的新任务。
    pub(crate) struct LoadProbe {
        /// 被观察的图库 ID。
        id: String,
        /// 解码启动次数。
        count: Arc<AtomicUsize>,
    }
    impl LoadProbe {
        /// 仅为当前测试的实际来源建立唯一观察，不向生产解码注入替代结果。
        pub(crate) fn new(id: &str) -> Self {
            let count = Arc::new(AtomicUsize::new(0));
            let old = PROBES.get_or_init(Mutex::default).lock().unwrap()
                .insert(id.to_owned(), Arc::downgrade(&count));
            assert!(old.and_then(|old| old.upgrade()).is_none(), "同 ID 的测试探针不能重叠");
            Self { id: id.to_owned(), count }
        }
        /// 返回真正进入 Asset::load 的次数，不读取资源登记器的内部状态。
        pub(crate) fn started(&self) -> usize { self.count.load(Ordering::Acquire) }
    }
    impl Drop for LoadProbe {
        /// 失败断言或正常结束都注销当前 ID，避免探针成为进程全局驻留记录。
        fn drop(&mut self) {
            if let Some(probes) = PROBES.get() {
                probes.lock().unwrap().remove(&self.id);
            }
        }
    }
    /// 生产解码入口在测试配置中旁路记录一次调用，未观察的 ID 无副作用。
    pub(super) fn started(id: &str) {
        let count = PROBES.get().and_then(|probes| probes.lock().unwrap().get(id).and_then(Weak::upgrade));
        if let Some(count) = count { count.fetch_add(1, Ordering::AcqRel); }
    }
}
/// 解码像素与浏览器语义的总播放次数；None 表示无限循环。
pub(crate) struct DecodedMonitorImage {
    /// 可直接绘制的解码图像。
    pub image: Arc<RenderImage>,
    /// 纹理含复制边缘保护像素，布局仍使用未经扩展的天然尺寸。
    pub natural_size: Size<DevicePixels>,
    /// 总播放次数；None 表示无限循环。
    pub plays: Option<u32>,
    /// 包含所有帧和边缘保护像素，绘制闭包也持有完整结果 owner。
    pub(super) _memory: super::image_memory::ImageMemoryLease,
}
impl Asset for MonitorImageDecoder {
    /// 已绑定像素预算与来源的解码输入。
    type Source = MonitorImageSource;
    /// 解码结果由资源引用共同持有。
    type Output = Result<Arc<DecodedMonitorImage>, &'static str>;
    /// GPUI 资源 owner 在后台解码；UI render 不执行图像编解码。
    fn load(
        source: Self::Source,
        cx: &mut App,
    ) -> impl Future<Output = Self::Output> + Send + 'static {
        #[cfg(test)]
        load_observation::started(&source.id);
        let resources = super::image_memory::resources_for_app(cx);
        cx.background_executor().spawn(async move {
            let _permit = resources.acquire().await;
            decode(&source, &resources).map_err(|_| "error.monitor.imageDecodeFailed")
        })
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
fn decode(
    source: &MonitorImageSource,
    resources: &super::image_memory::DecodeResources,
) -> anyhow::Result<Arc<DecodedMonitorImage>> {
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
    } else if source.format == ImageFormat::Webp {
        let mut decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(&*source.bytes))?;
        decoder.set_limits(limits())?;
        if decoder.has_animation() {
            if decoder.total_bytes().saturating_mul(2) > WORK_BYTES {
                anyhow::bail!("monitor WebP working set exceeded")
            }
            plays = match decoder.loop_count() {
                image::metadata::LoopCount::Infinite => None,
                image::metadata::LoopCount::Finite(count) => Some(count.get()),
            };
            frames = animation_frames(decoder.into_frames())?;
        } else { frames = vec![static_frame(decoder)?]; }
    } else {
        let format = match source.format {
            ImageFormat::Bmp => image::ImageFormat::Bmp,
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
    let retained = frames
        .iter()
        .map(|frame| {
            u64::from(frame.buffer().width() + 2) * u64::from(frame.buffer().height() + 2) * 4
        })
        .sum();
    let memory = resources.reserve(retained)?;
    let frames = texture_frames(frames)?;
    Ok(Arc::new(DecodedMonitorImage {
        image: Arc::new(RenderImage::new(frames)),
        natural_size,
        plays,
        _memory: memory,
    }))
}

/// GIF/APNG/WebP 均逐帧累计已保留内存，任何损坏帧都明确失败。
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
    for pixel in rgba.as_chunks_mut::<4>().0 {
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
        let decoded = decode(&source, &super::super::image_memory::DecodeResources::new()).unwrap();
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
        let resources = super::super::image_memory::DecodeResources::new();
        let mut bytes = include_bytes!("../../tests/fixtures/monitor-loop.gif").to_vec();
        bytes[6] = 0xff;
        bytes[7] = 0xff;
        assert!(
            decode(&MonitorImageSource {
                id: "large".into(),
                bytes: Arc::from(bytes),
                format: ImageFormat::Gif
            }, &resources)
            .is_err()
        );
        assert!(
            decode(&MonitorImageSource {
                id: "bad".into(),
                bytes: Arc::from(b"\x89PNG\r\n\x1a\n".as_slice()),
                format: ImageFormat::Png
            }, &resources)
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
        let decoded = decode(&source, &super::super::image_memory::DecodeResources::new()).unwrap();
        let image = &decoded.image;
        assert_eq!(
            decoded.natural_size,
            size(DevicePixels(1024), DevicePixels(1024))
        );
        assert_eq!(image.frame_count(), 1);
    }

    /// 六套内置兼容皮肤的辅助 JPEG 必须完整解码；魔数不能掩盖错误的渐进帧头。
    #[test]
    fn builtin_skin_auxiliary_jpegs_decode_without_changing_placeholder_dimensions() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources/builtin-skins");
        let resources = super::super::image_memory::DecodeResources::new();
        for id in [
            "minecraft", "pikachu", "pastoral-landscape", "misty-meadow-dawn",
            "silver-core-voyage", "woodland-dawn",
        ] {
            let source = MonitorImageSource {
                id: format!("builtin-{id}-auxiliary"),
                bytes: Arc::from(std::fs::read(root.join(id).join("qqshow.jpg"))
                    .expect("内置辅助图必须存在")),
                format: ImageFormat::Jpeg,
            };
            let decoded = decode(&source, &resources)
                .unwrap_or_else(|error| panic!("内置皮肤 {id} 的辅助 JPEG 解码失败：{error}"));
            assert_eq!(decoded.natural_size, size(DevicePixels(1), DevicePixels(1)));
            assert_eq!(decoded.image.frame_count(), 1);
            assert_eq!(decoded.image.as_bytes(0).unwrap().len(), 3 * 3 * 4);
        }
    }

    /// APNG 是实际 PNG 动画，保留总播放次数、两帧像素和延迟。
    #[test]
    fn apng_fixture_preserves_animation_and_finite_plays() {
        let source = MonitorImageSource {
            id: "apng".into(),
            bytes: Arc::from(include_bytes!("../../tests/fixtures/monitor-finite.png").as_slice()),
            format: ImageFormat::Png,
        };
        let decoded = decode(&source, &super::super::image_memory::DecodeResources::new()).unwrap();
        assert_eq!(decoded.plays, Some(2));
        assert_eq!(decoded.image.frame_count(), 2);
        assert_eq!(decoded.image.as_bytes(0).unwrap(), [0, 0, 0, 255].repeat(9));
        assert_eq!(
            decoded.image.as_bytes(1).unwrap(),
            [255, 255, 255, 255].repeat(9)
        );
        assert_eq!(decoded.image.delay(0).numer_denom_ms(), (200, 1));
    }

    /// BMP 和静态 WebP 都走真实编码后的字节，保留尺寸、像素和单次播放。
    #[test]
    fn bmp_and_static_webp_decode_their_actual_pixels() {
        for (format, codec) in [(ImageFormat::Bmp, image::ImageFormat::Bmp), (ImageFormat::Webp, image::ImageFormat::WebP)] {
            let pixel = image::RgbaImage::from_pixel(1, 1, image::Rgba([12, 34, 56, 255]));
            let mut bytes = Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(pixel).write_to(&mut bytes, codec).unwrap();
            let source = MonitorImageSource { id: format!("static-{format:?}"), format, bytes: Arc::from(bytes.into_inner()) };
            let decoded = decode(&source, &super::super::image_memory::DecodeResources::new()).unwrap();
            assert_eq!(decoded.natural_size, size(DevicePixels(1), DevicePixels(1)));
            assert_eq!(decoded.plays, Some(1));
            assert_eq!(decoded.image.frame_count(), 1);
            assert_eq!(decoded.image.as_bytes(0).unwrap(), [56, 34, 12, 255].repeat(9));
        }
    }

    /// 用现有无损编码器构造两帧 WebP 容器，循环次数和延迟都由正式解码器读取。
    fn animated_webp(loops: u16) -> Vec<u8> {
        let mut body = b"WEBP".to_vec();
        let chunk = |output: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]| {
            output.extend_from_slice(kind); output.extend_from_slice(&(data.len() as u32).to_le_bytes());
            output.extend_from_slice(data);
            if data.len() % 2 != 0 { output.push(0); }
        };
        chunk(&mut body, b"VP8X", &[2, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let mut animation = vec![0, 0, 0, 0]; animation.extend_from_slice(&loops.to_le_bytes());
        chunk(&mut body, b"ANIM", &animation);
        for (pixel, delay) in [([0, 0, 0, 255], 120u32), ([255, 255, 255, 255], 230u32)] {
            let mut encoded = Vec::new();
            image::codecs::webp::WebPEncoder::new_lossless(&mut encoded)
                .encode(&pixel, 1, 1, image::ExtendedColorType::Rgba8).unwrap();
            let mut frame = vec![0; 16]; frame[12..15].copy_from_slice(&delay.to_le_bytes()[..3]);
            frame[15] = 2; // 测试帧完整替换画布，不叠加上一帧的颜色。
            frame.extend_from_slice(&encoded[12..]);
            chunk(&mut body, b"ANMF", &frame);
        }
        let mut bytes = b"RIFF".to_vec(); bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
        bytes.extend(body); bytes
    }

    /// 动态 WebP 必须保留两帧、各自延迟及有限或无限播放，不能退化成首帧静图。
    #[test]
    fn animated_webp_keeps_frames_delays_and_loop_count() {
        for (loops, plays) in [(0, None), (3, Some(3))] {
            let source = MonitorImageSource { id: format!("animated-webp-{loops}"), format: ImageFormat::Webp, bytes: Arc::from(animated_webp(loops)) };
            let decoded = decode(&source, &super::super::image_memory::DecodeResources::new()).unwrap();
            assert_eq!(decoded.plays, plays);
            assert_eq!(decoded.image.frame_count(), 2);
            assert_eq!(decoded.natural_size, size(DevicePixels(1), DevicePixels(1)));
            assert_eq!(decoded.image.as_bytes(0).unwrap(), [0, 0, 0, 255].repeat(9));
            assert_eq!(decoded.image.as_bytes(1).unwrap(), [255, 255, 255, 255].repeat(9));
            assert_eq!(decoded.image.delay(0).numer_denom_ms(), (120, 1));
            assert_eq!(decoded.image.delay(1).numer_denom_ms(), (230, 1));
        }
    }

    /// 魔数合法但无图像数据的 BMP/WebP 必须失败，不能伪装为已解码图片。
    #[test]
    fn corrupt_bmp_and_webp_fail_decoding() {
        for (format, bytes) in [(ImageFormat::Bmp, b"BM....".as_slice()), (ImageFormat::Webp, b"RIFF....WEBP".as_slice())] {
            let source = MonitorImageSource { id: "corrupt".into(), format, bytes: Arc::from(bytes) };
            assert!(decode(&source, &super::super::image_memory::DecodeResources::new()).is_err());
        }
    }
}
