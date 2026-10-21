//! 原生图片资源解码：支持全部图库格式、保留动画与透明度，并限制解码资源。

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
    // 穷尽匹配：core 新增格式时必须在此显式选择解码路径。
    let frames = match source.format {
        ImageFormat::Gif => {
            let mut decoder = image::codecs::gif::GifDecoder::new(Cursor::new(&*source.bytes))?;
            decoder.set_limits(limits())?;
            plays = super::animation_metadata::gif_plays(&source.bytes)?;
            texture_frames(animation_frames(decoder.into_frames())?)?
        }
        ImageFormat::Png => {
            let decoder =
                image::codecs::png::PngDecoder::with_limits(Cursor::new(&*source.bytes), limits())?;
            if decoder.is_apng()? {
                drop(decoder);
                let (frames, count) = super::png_animation::decode(&source.bytes)?;
                plays = count;
                texture_frames(frames)?
            } else {
                vec![static_frame(decoder)?]
            }
        }
        ImageFormat::Webp => {
            let mut decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(&*source.bytes))?;
            // WebP 的 set_limits 只检查宽高，不执行 max_alloc，预算在解码像素前由容器头核对。
            decoder.set_limits(limits())?;
            check_webp_budget(&source.bytes)?;
            if decoder.has_animation() {
                plays = match decoder.loop_count() {
                    image::metadata::LoopCount::Infinite => None,
                    image::metadata::LoopCount::Finite(count) => Some(count.get()),
                };
                texture_frames(animation_frames(decoder.into_frames())?)?
            } else {
                vec![static_frame(decoder)?]
            }
        }
        ImageFormat::Jpeg | ImageFormat::Bmp => {
            let format = if source.format == ImageFormat::Bmp {
                image::ImageFormat::Bmp
            } else {
                image::ImageFormat::Jpeg
            };
            let mut reader = image::ImageReader::with_format(Cursor::new(&*source.bytes), format);
            reader.limits(limits());
            vec![static_frame(reader.into_decoder()?)?]
        }
    };
    if frames.is_empty() {
        anyhow::bail!("monitor image has no frames")
    }
    // 各分支产出的帧均已含一圈边缘保护像素，天然尺寸需扣除。
    let natural_size = size(
        DevicePixels(frames[0].buffer().width() as i32 - 2),
        DevicePixels(frames[0].buffer().height() as i32 - 2),
    );
    let retained = frames
        .iter()
        .map(|frame| frame.buffer().as_raw().len() as u64)
        .sum();
    let memory = resources.reserve(retained)?;
    Ok(Arc::new(DecodedMonitorImage {
        image: Arc::new(RenderImage::new(frames)),
        natural_size,
        plays,
        _memory: memory,
    }))
}

/// WebP 只解析容器头即可得到画布、帧数与编码方式，在产生任何像素前完成全部预算。
///
/// 动态图逐帧解码时同时存在常驻 RGBA 画布、子帧缓冲与输出帧；无 alpha 时解码器
/// 先产出 RGB 帧再转 RGBA，另多一份 RGB。静态无损无 alpha 图在解码器内部另有一份
/// RGBA 临时缓冲，叠加在 `static_frame` 的解码结果与 RGBA 副本之上。
fn check_webp_budget(bytes: &[u8]) -> anyhow::Result<()> {
    let mut header = image_webp::WebPDecoder::new(Cursor::new(bytes))?;
    let (width, height) = header.dimensions();
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        anyhow::bail!("monitor WebP dimensions exceeded")
    }
    let pixels = u64::from(width) * u64::from(height);
    let rgb_extra = if header.has_alpha() { 0 } else { 3 };
    if header.is_animated() {
        let frames = header.num_frames() as usize;
        let retained = frames as u64 * u64::from(width + 2) * u64::from(height + 2) * 4;
        if frames == 0 || frames > MAX_FRAMES || retained > RETAINED_BYTES {
            anyhow::bail!("monitor WebP frame budget exceeded")
        }
        if pixels * (4 * 3 + rgb_extra) > WORK_BYTES {
            anyhow::bail!("monitor WebP working set exceeded")
        }
    } else {
        let decoded = pixels * if header.has_alpha() { 4 } else { 3 };
        let lossless_temp = if header.is_lossy() || header.has_alpha() {
            0
        } else {
            pixels * 4
        };
        if decoded + pixels * 4 + lossless_temp > RETAINED_BYTES {
            anyhow::bail!("monitor WebP pixel budget exceeded")
        }
    }
    Ok(())
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

/// 静态图先验像素预算，再应用 EXIF 方向；一次遍历直接生成带边缘保护的 BGRA 纹理，
/// RGB 来源不再先整幅复制为 RGBA。
fn static_frame(mut decoder: impl ImageDecoder) -> anyhow::Result<image::Frame> {
    let orientation = decoder.orientation()?;
    let (width, height) = decoder.dimensions();
    let texture = u64::from(width + 2) * u64::from(height + 2) * 4;
    if width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || decoder.total_bytes().saturating_add(texture) > RETAINED_BYTES
    {
        anyhow::bail!("monitor pixel budget exceeded")
    }
    let mut decoded = image::DynamicImage::from_decoder(decoder)?;
    decoded.apply_orientation(orientation);
    let texture = match decoded {
        image::DynamicImage::ImageRgb8(source) => padded_texture(&source, |pixel| {
            let [red, green, blue] = pixel.0;
            image::Rgba([blue, green, red, 255])
        }),
        image::DynamicImage::ImageRgba8(source) => padded_texture(&source, rgba_to_bgra),
        other => padded_texture(&other.into_rgba8(), rgba_to_bgra),
    };
    Ok(image::Frame::new(texture))
}

/// 单像素 RGBA 到 BGRA。
fn rgba_to_bgra(pixel: image::Rgba<u8>) -> image::Rgba<u8> {
    let [red, green, blue, alpha] = pixel.0;
    image::Rgba([blue, green, red, alpha])
}

/// GPUI 纹理使用 BGRA，保留原 alpha 不做背景合成。
pub(super) fn bgra(rgba: &mut image::RgbaImage) {
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
}

/// 动画帧已是 BGRA，只补边缘保护像素并累计纹理预算。
fn texture_frames(frames: Vec<image::Frame>) -> anyhow::Result<Vec<image::Frame>> {
    let mut retained = 0u64;
    frames
        .into_iter()
        .map(|frame| {
            let delay = frame.delay();
            let source = frame.into_buffer();
            retained = retained
                .saturating_add(u64::from(source.width() + 2) * u64::from(source.height() + 2) * 4);
            if retained > RETAINED_BYTES {
                anyhow::bail!("monitor texture budget exceeded")
            }
            let padded = padded_texture(&source, |pixel| pixel);
            Ok(image::Frame::from_parts(padded, 0, 0, delay))
        })
        .collect()
}

/// GPUI atlas 无边距且采用线性采样，复制一圈边缘避免放大小图时混入透明或邻块；
/// `to_bgra` 在同一遍历中把来源像素转换为纹理像素。
fn padded_texture<P: image::Pixel<Subpixel = u8>>(
    source: &image::ImageBuffer<P, Vec<u8>>,
    to_bgra: impl Fn(P) -> image::Rgba<u8>,
) -> image::RgbaImage {
    let (width, height) = source.dimensions();
    image::RgbaImage::from_fn(width + 2, height + 2, |x, y| {
        to_bgra(*source.get_pixel(
            x.saturating_sub(1).min(width - 1),
            y.saturating_sub(1).min(height - 1),
        ))
    })
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
        for (format, codec) in [
            (ImageFormat::Bmp, image::ImageFormat::Bmp),
            (ImageFormat::Webp, image::ImageFormat::WebP),
        ] {
            let pixel = image::RgbaImage::from_pixel(1, 1, image::Rgba([12, 34, 56, 255]));
            let mut bytes = Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(pixel)
                .write_to(&mut bytes, codec)
                .unwrap();
            let source = MonitorImageSource {
                id: format!("static-{format:?}"),
                format,
                bytes: Arc::from(bytes.into_inner()),
            };
            let decoded =
                decode(&source, &super::super::image_memory::DecodeResources::new()).unwrap();
            assert_eq!(decoded.natural_size, size(DevicePixels(1), DevicePixels(1)));
            assert_eq!(decoded.plays, Some(1));
            assert_eq!(decoded.image.frame_count(), 1);
            assert_eq!(
                decoded.image.as_bytes(0).unwrap(),
                [56, 34, 12, 255].repeat(9)
            );
        }
    }

    /// 用现有无损编码器构造 1×1 动态 WebP 容器，循环次数和延迟都由正式解码器读取。
    fn animated_webp(loops: u16, frames: &[([u8; 4], u32)]) -> Vec<u8> {
        let mut body = b"WEBP".to_vec();
        // RIFF 子块：四字节类型、小端长度、正文，奇数长度补一字节对齐。
        let chunk = |output: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]| {
            output.extend_from_slice(kind);
            output.extend_from_slice(&(data.len() as u32).to_le_bytes());
            output.extend_from_slice(data);
            if !data.len().is_multiple_of(2) {
                output.push(0);
            }
        };
        // 首字节 2 为动画标志；画布宽高均以“减一”存储，0 即 1 像素。
        chunk(&mut body, b"VP8X", &[2, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        // ANIM：四字节背景色后接两字节循环次数，0 表示无限循环。
        let mut animation = vec![0, 0, 0, 0];
        animation.extend_from_slice(&loops.to_le_bytes());
        chunk(&mut body, b"ANIM", &animation);
        for &(pixel, delay) in frames {
            let mut encoded = Vec::new();
            image::codecs::webp::WebPEncoder::new_lossless(&mut encoded)
                .encode(&pixel, 1, 1, image::ExtendedColorType::Rgba8)
                .unwrap();
            // ANMF 头 16 字节：偏移与“减一”宽高均为 0，第 12..15 字节是三字节延迟。
            let mut frame = vec![0; 16];
            frame[12..15].copy_from_slice(&delay.to_le_bytes()[..3]);
            frame[15] = 2; // 测试帧完整替换画布，不叠加上一帧的颜色。
            // 去掉编码结果的 RIFF/WEBP 12 字节文件头，只嵌入 VP8L 子块。
            frame.extend_from_slice(&encoded[12..]);
            chunk(&mut body, b"ANMF", &frame);
        }
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
        bytes.extend(body);
        bytes
    }

    /// 动态 WebP 必须保留两帧、各自延迟及有限或无限播放，不能退化成首帧静图。
    #[test]
    fn animated_webp_keeps_frames_delays_and_loop_count() {
        for (loops, plays) in [(0, None), (3, Some(3))] {
            let source = MonitorImageSource {
                id: format!("animated-webp-{loops}"),
                format: ImageFormat::Webp,
                bytes: Arc::from(animated_webp(
                    loops,
                    &[([0, 0, 0, 255], 120), ([255, 255, 255, 255], 230)],
                )),
            };
            let decoded =
                decode(&source, &super::super::image_memory::DecodeResources::new()).unwrap();
            assert_eq!(decoded.plays, plays);
            assert_eq!(decoded.image.frame_count(), 2);
            assert_eq!(decoded.natural_size, size(DevicePixels(1), DevicePixels(1)));
            assert_eq!(decoded.image.as_bytes(0).unwrap(), [0, 0, 0, 255].repeat(9));
            assert_eq!(
                decoded.image.as_bytes(1).unwrap(),
                [255, 255, 255, 255].repeat(9)
            );
            assert_eq!(decoded.image.delay(0).numer_denom_ms(), (120, 1));
            assert_eq!(decoded.image.delay(1).numer_denom_ms(), (230, 1));
        }
    }

    /// 帧数超限的动态 WebP 只凭容器头即被拒绝，不进入逐帧解码。
    #[test]
    fn animated_webp_over_frame_budget_fails_before_decoding() {
        let frames = vec![([0, 0, 0, 255], 10); MAX_FRAMES + 1];
        let bytes = animated_webp(0, &frames);
        assert!(check_webp_budget(&bytes).is_err());
        assert!(check_webp_budget(&animated_webp(0, &frames[..2])).is_ok());
    }

    /// 魔数合法但无图像数据的 BMP/WebP 必须失败，不能伪装为已解码图片。
    #[test]
    fn corrupt_bmp_and_webp_fail_decoding() {
        for (format, bytes) in [
            (ImageFormat::Bmp, b"BM....".as_slice()),
            (ImageFormat::Webp, b"RIFF....WEBP".as_slice()),
        ] {
            let source = MonitorImageSource {
                id: "corrupt".into(),
                format,
                bytes: Arc::from(bytes),
            };
            assert!(decode(&source, &super::super::image_memory::DecodeResources::new()).is_err());
        }
    }
}
