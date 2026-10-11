//! APNG 原生合成器补齐 image 当前不支持的 16 位动画，保留子帧与处置语义。

use super::image_decode::{MAX_DIMENSION, MAX_FRAMES, RETAINED_BYTES, WORK_BYTES, bgra};
use image::{Delay, Frame, Pixel, Rgba, RgbaImage};
use std::io::Cursor;

/// png 的规范化先按原始位深处理透明键，再把所有颜色类型转成 8 位采样。
pub(super) fn decode(bytes: &[u8]) -> anyhow::Result<(Vec<Frame>, Option<u32>)> {
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: WORK_BYTES as usize,
        },
    );
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info()?;
    let (width, height, interlaced) = (
        reader.info().width,
        reader.info().height,
        reader.info().interlaced,
    );
    let control = reader
        .info()
        .animation_control
        .ok_or_else(|| anyhow::anyhow!("PNG has no animation"))?;
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || control.num_frames == 0
        || control.num_frames as usize > MAX_FRAMES
    {
        anyhow::bail!("monitor APNG dimensions or frame count exceeded")
    }
    let raw_size = reader
        .output_buffer_size()
        .ok_or_else(|| anyhow::anyhow!("APNG output too large"))?;
    let canvas_bytes = u64::from(width) * u64::from(height) * 4;
    if raw_size as u64 + canvas_bytes > WORK_BYTES {
        anyhow::bail!("APNG work budget exceeded")
    }
    let canvas_stride = reader
        .output_line_size(width)
        .ok_or_else(|| anyhow::anyhow!("APNG row too large"))?;
    // 独立默认图是旧阅读器的封面，不应成为动画的第一帧。
    if reader.info().frame_control.is_none() {
        reader.next_frame_info()?;
    }
    let mut raw = vec![0; raw_size];
    let mut canvas = RgbaImage::new(width, height);
    let mut frames = Vec::new();
    let mut retained = 0u64;
    for index in 0..control.num_frames {
        let output = reader.next_frame(&mut raw)?;
        let frame = reader
            .info()
            .frame_control
            .ok_or_else(|| anyhow::anyhow!("APNG frame control missing"))?;
        if frame.width == 0
            || frame.height == 0
            || frame
                .x_offset
                .checked_add(frame.width)
                .is_none_or(|right| right > width)
            || frame
                .y_offset
                .checked_add(frame.height)
                .is_none_or(|bottom| bottom > height)
        {
            anyhow::bail!("APNG frame outside canvas")
        }
        let dispose = if index == 0 && frame.dispose_op == png::DisposeOp::Previous {
            png::DisposeOp::Background
        } else {
            frame.dispose_op
        };
        if dispose == png::DisposeOp::Previous && raw_size as u64 + canvas_bytes * 2 > WORK_BYTES {
            anyhow::bail!("APNG previous-frame work budget exceeded")
        }
        let previous = (dispose == png::DisposeOp::Previous).then(|| canvas.clone());
        let stride = if interlaced {
            canvas_stride
        } else {
            output.line_size
        };
        let samples = output.color_type.samples();
        for y in 0..frame.height {
            for x in 0..frame.width {
                let start = y as usize * stride + x as usize * samples;
                let values = raw
                    .get(start..start + samples)
                    .ok_or_else(|| anyhow::anyhow!("APNG row truncated"))?;
                let source = match output.color_type {
                    png::ColorType::Rgba => Rgba([values[0], values[1], values[2], values[3]]),
                    png::ColorType::Rgb => Rgba([values[0], values[1], values[2], 255]),
                    png::ColorType::GrayscaleAlpha => {
                        Rgba([values[0], values[0], values[0], values[1]])
                    }
                    png::ColorType::Grayscale => Rgba([values[0], values[0], values[0], 255]),
                    png::ColorType::Indexed => anyhow::bail!("APNG palette was not expanded"),
                };
                let target = canvas.get_pixel_mut(frame.x_offset + x, frame.y_offset + y);
                match frame.blend_op {
                    png::BlendOp::Source => *target = source,
                    png::BlendOp::Over => target.blend(&source),
                }
            }
        }
        retained = retained.saturating_add(canvas_bytes);
        if retained > RETAINED_BYTES {
            anyhow::bail!("APNG retained budget exceeded")
        }
        let mut pixels = canvas.clone();
        bgra(&mut pixels);
        let denominator = if frame.delay_den == 0 {
            100
        } else {
            u32::from(frame.delay_den)
        };
        frames.push(Frame::from_parts(
            pixels,
            0,
            0,
            Delay::from_numer_denom_ms(u32::from(frame.delay_num) * 1000, denominator),
        ));
        match dispose {
            png::DisposeOp::None => {}
            png::DisposeOp::Background => {
                for y in frame.y_offset..frame.y_offset + frame.height {
                    for x in frame.x_offset..frame.x_offset + frame.width {
                        canvas.put_pixel(x, y, Rgba([0, 0, 0, 0]));
                    }
                }
            }
            png::DisposeOp::Previous => canvas = previous.expect("previous canvas was reserved"),
        }
    }
    reader.finish()?;
    Ok((
        frames,
        (control.num_plays != 0).then_some(control.num_plays),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Adam7 窄子帧按完整画布跨度展开，实际偏移必须落在右下角。
    #[test]
    fn interlaced_subframe_uses_full_canvas_stride() {
        let (frames, plays) = decode(include_bytes!(
            "../../tests/fixtures/monitor-interlaced.png"
        ))
        .unwrap();
        assert_eq!(plays, Some(1));
        assert_eq!(frames.len(), 2);
        for y in 0..3 {
            for x in 0..3 {
                assert_eq!(*frames[0].buffer().get_pixel(x, y), Rgba([0, 0, 255, 255]));
                assert_eq!(
                    *frames[1].buffer().get_pixel(x, y),
                    if x > 0 && y > 0 {
                        Rgba([255, 0, 0, 255])
                    } else {
                        Rgba([0, 0, 255, 255])
                    }
                );
            }
        }
    }

    /// 实际 16 位子帧覆盖、恢复和清背景必须按画布合成，不能仅接受文件头。
    #[test]
    fn sixteen_bit_subframes_preserve_blend_disposal_and_delays() {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 2, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Sixteen);
            encoder.set_animated(4, 3).unwrap();
            encoder.set_frame_delay(1, 5).unwrap();
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&[
                    255, 255, 0, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0, 255, 255,
                ])
                .unwrap();
            writer.set_frame_dimension(1, 1).unwrap();
            writer.set_frame_position(1, 0).unwrap();
            writer.set_dispose_op(png::DisposeOp::Previous).unwrap();
            writer
                .write_image_data(&[0, 0, 255, 255, 0, 0, 255, 255])
                .unwrap();
            writer.set_frame_position(0, 0).unwrap();
            writer.set_dispose_op(png::DisposeOp::Background).unwrap();
            writer
                .write_image_data(&[0, 0, 0, 0, 255, 255, 255, 255])
                .unwrap();
            writer.set_frame_position(1, 0).unwrap();
            writer.set_dispose_op(png::DisposeOp::None).unwrap();
            writer
                .write_image_data(&[0, 0, 255, 255, 0, 0, 255, 255])
                .unwrap();
            writer.finish().unwrap();
        }
        let (frames, plays) = decode(&bytes).unwrap();
        assert_eq!(plays, Some(3));
        let pixels = |index: usize| frames[index].buffer().as_raw().clone();
        assert_eq!(pixels(0), [0, 0, 255, 255, 0, 0, 255, 255]);
        assert_eq!(pixels(1), [0, 0, 255, 255, 0, 255, 0, 255]);
        assert_eq!(pixels(2), [255, 0, 0, 255, 0, 0, 255, 255]);
        assert_eq!(pixels(3), [0, 0, 0, 0, 0, 255, 0, 255]);
        assert_eq!(frames[0].delay().numer_denom_ms(), (200, 1));
    }

    /// 独立默认图不进入动画，16 位透明键须在降位前比较完整采样值。
    #[test]
    fn separate_default_and_sixteen_bit_transparent_key() {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Sixteen);
            encoder.set_trns(vec![0x12, 0x34]);
            encoder.set_animated(2, 0).unwrap();
            encoder.set_sep_def_img(true).unwrap();
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[0xff, 0xff]).unwrap();
            writer.write_image_data(&[0x12, 0x34]).unwrap();
            writer.write_image_data(&[0x12, 0x35]).unwrap();
            writer.finish().unwrap();
        }
        let (frames, plays) = decode(&bytes).unwrap();
        assert_eq!(plays, None);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].buffer().as_raw(), &[0x12, 0x12, 0x12, 0]);
        assert_eq!(frames[1].buffer().as_raw(), &[0x12, 0x12, 0x12, 255]);
    }
}
