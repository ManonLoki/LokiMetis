//! 在第三方 WebP 解码器分配像素前核对 RIFF、动画子图与实际编码头的尺寸。

use super::image_decode::MAX_DIMENSION;

/// 零像素分配扫描所有编码帧，避免小 VP8X/ANMF 画布掩盖超大 VP8 缓冲。
pub(super) fn validate_dimensions(bytes: &[u8]) -> anyhow::Result<()> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        anyhow::bail!("monitor WebP container invalid")
    }
    let length = u32::from_le_bytes(bytes[4..8].try_into()?) as u64 + 8;
    if length != bytes.len() as u64 {
        anyhow::bail!("monitor WebP container length invalid")
    }
    let mut canvas = None;
    let mut chunks = &bytes[12..];
    while let Some((kind, payload)) = next_chunk(&mut chunks)? {
        if kind == b"VP8X" {
            if payload.len() != 10 || canvas.is_some() {
                anyhow::bail!("monitor WebP canvas invalid")
            }
            canvas = Some((little24(&payload[4..7]) + 1, little24(&payload[7..10]) + 1));
        }
    }
    if let Some(size) = canvas { check_size(size)?; }
    chunks = &bytes[12..];
    let mut encoded = false;
    while let Some((kind, payload)) = next_chunk(&mut chunks)? {
        match kind {
            b"VP8 " | b"VP8L" => {
                check_bitstream(kind, payload, canvas)?;
                encoded = true;
            }
            b"ANMF" => {
                let canvas = canvas.ok_or_else(|| anyhow::anyhow!("monitor WebP canvas missing"))?;
                check_frame(payload, canvas)?;
                encoded = true;
            }
            _ => {}
        }
    }
    if !encoded { anyhow::bail!("monitor WebP image missing") }
    Ok(())
}

/// 长度与偶数字节填充均在切片前检查，不分配或递归解析任意嵌套数据。
fn next_chunk<'a>(chunks: &mut &'a [u8]) -> anyhow::Result<Option<(&'a [u8], &'a [u8])>> {
    if chunks.is_empty() { return Ok(None); }
    let header = chunks.get(..8).ok_or_else(|| anyhow::anyhow!("monitor WebP chunk truncated"))?;
    let length = u32::from_le_bytes(header[4..8].try_into()?) as usize;
    let end = 8usize.checked_add(length).ok_or_else(|| anyhow::anyhow!("monitor WebP chunk overflow"))?;
    let padded = end.checked_add(length % 2).ok_or_else(|| anyhow::anyhow!("monitor WebP chunk overflow"))?;
    let payload = chunks.get(8..end).ok_or_else(|| anyhow::anyhow!("monitor WebP chunk truncated"))?;
    *chunks = chunks.get(padded..).ok_or_else(|| anyhow::anyhow!("monitor WebP padding missing"))?;
    Ok(Some((&header[..4], payload)))
}

/// 动画只接受规范的 VP8、VP8L 或 ALPH+VP8；ALPH 后的类型也必须复核。
fn check_frame(payload: &[u8], canvas: (u32, u32)) -> anyhow::Result<()> {
    let header = payload.get(..16).ok_or_else(|| anyhow::anyhow!("monitor WebP frame truncated"))?;
    let position = (little24(&header[..3]) * 2, little24(&header[3..6]) * 2);
    let size = (little24(&header[6..9]) + 1, little24(&header[9..12]) + 1);
    check_size(size)?;
    if position.0 + size.0 > canvas.0 || position.1 + size.1 > canvas.1 {
        anyhow::bail!("monitor WebP frame outside canvas")
    }
    let mut chunks = &payload[16..];
    let (mut kind, mut data) = next_chunk(&mut chunks)?.ok_or_else(|| anyhow::anyhow!("monitor WebP frame missing"))?;
    if kind == b"ALPH" {
        (kind, data) = next_chunk(&mut chunks)?.ok_or_else(|| anyhow::anyhow!("monitor WebP alpha frame missing"))?;
        if kind != b"VP8 " { anyhow::bail!("monitor WebP alpha frame invalid") }
    }
    check_bitstream(kind, data, Some(size))?;
    // 规范允许编码帧后附未知扩展块；逐块校验边界，但不能再出现已知重建或元数据块。
    while let Some((kind, _)) = next_chunk(&mut chunks)? {
        if matches!(kind, b"VP8 " | b"VP8L" | b"ALPH" | b"ANMF" | b"VP8X" | b"ANIM" | b"ICCP" | b"EXIF" | b"XMP ") {
            anyhow::bail!("monitor WebP extra frame chunks")
        }
    }
    Ok(())
}

/// VP8 关键帧与 VP8L 固定头直接给出真实像素尺寸，无需解压熵编码正文。
fn check_bitstream(kind: &[u8], data: &[u8], expected: Option<(u32, u32)>) -> anyhow::Result<()> {
    let size = match kind {
        b"VP8 " if data.len() >= 10 && data[0] & 1 == 0 && data[3..6] == [0x9d, 0x01, 0x2a] => {
            (u32::from(u16::from_le_bytes(data[6..8].try_into()?) & 0x3fff),
             u32::from(u16::from_le_bytes(data[8..10].try_into()?) & 0x3fff))
        }
        b"VP8L" if data.len() >= 5 && data[0] == 0x2f && data[4] & 0xe0 == 0 => {
            let bits = u32::from_le_bytes(data[1..5].try_into()?);
            ((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1)
        }
        _ => anyhow::bail!("monitor WebP bitstream header invalid"),
    };
    check_size(size)?;
    if expected.is_some_and(|expected| expected != size) {
        anyhow::bail!("monitor WebP frame dimensions mismatch")
    }
    Ok(())
}

/// 图像驱动的严格尺寸限制同时作用于容器、子图与编码帧。
fn check_size((width, height): (u32, u32)) -> anyhow::Result<()> {
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        anyhow::bail!("monitor WebP dimensions exceeded")
    }
    Ok(())
}

/// WebP 的三字节整数为无符号小端，调用方已检查头长度。
fn little24(bytes: &[u8]) -> u32 { u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0]) }
