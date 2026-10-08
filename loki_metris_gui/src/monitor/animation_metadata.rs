//! GIF 的额外重复次数映射为实际总播放次数，区分缺少扩展和无限循环。

/// GIF 应用扩展中 0 表示无限；没有扩展只播放一次。
pub(super) fn gif_plays(bytes: &[u8]) -> anyhow::Result<Option<u32>> {
    if bytes.len() < 13 {
        anyhow::bail!("truncated GIF header")
    }
    let mut cursor = 13;
    if bytes[10] & 0x80 != 0 {
        cursor += 3 * (1usize << ((bytes[10] & 7) + 1));
    }
    let mut plays = Some(1);
    while let Some(marker) = bytes.get(cursor).copied() {
        cursor += 1;
        match marker {
            0x3b => break,
            0x21 => {
                let label = *bytes
                    .get(cursor)
                    .ok_or_else(|| anyhow::anyhow!("truncated GIF extension"))?;
                cursor += 1;
                let mut application = false;
                let mut first = true;
                while let Some(block) = block(bytes, &mut cursor)? {
                    if first && label == 0xff {
                        application = block == b"NETSCAPE2.0" || block == b"ANIMEXTS1.0";
                    } else if application && block.len() == 3 && block[0] == 1 {
                        let repeats = u16::from_le_bytes([block[1], block[2]]);
                        plays = if repeats == 0 {
                            None
                        } else {
                            Some(u32::from(repeats) + 1)
                        };
                    }
                    first = false;
                }
            }
            0x2c => {
                let descriptor = bytes
                    .get(cursor..cursor + 9)
                    .ok_or_else(|| anyhow::anyhow!("truncated GIF descriptor"))?;
                cursor += 9;
                if descriptor[8] & 0x80 != 0 {
                    cursor += 3 * (1usize << ((descriptor[8] & 7) + 1));
                }
                cursor += 1; // LZW 最小码长，由实际解码器校验。
                while block(bytes, &mut cursor)?.is_some() {}
            }
            _ => anyhow::bail!("unexpected GIF block"),
        }
    }
    Ok(plays)
}

/// 子块长度最多 255，始终在输入边界内推进。
fn block<'a>(bytes: &'a [u8], cursor: &mut usize) -> anyhow::Result<Option<&'a [u8]>> {
    let len = usize::from(
        *bytes
            .get(*cursor)
            .ok_or_else(|| anyhow::anyhow!("truncated GIF block"))?,
    );
    *cursor += 1;
    if len == 0 {
        return Ok(None);
    }
    let value = bytes
        .get(*cursor..*cursor + len)
        .ok_or_else(|| anyhow::anyhow!("truncated GIF block data"))?;
    *cursor += len;
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 不把缺少 Netscape 扩展的普通 GIF 误判为无限循环。
    #[test]
    fn gif_loop_semantics_distinguish_missing_zero_and_extra_repeats() {
        let mut bytes = include_bytes!("../../tests/fixtures/monitor-loop.gif").to_vec();
        let start = bytes
            .windows(14)
            .position(|block| block == b"\x21\xff\x0bNETSCAPE2.0")
            .unwrap();
        assert_eq!(gif_plays(&bytes).unwrap(), None);
        bytes[start + 16] = 2;
        assert_eq!(gif_plays(&bytes).unwrap(), Some(3));
        bytes.drain(start..start + 19);
        assert_eq!(gif_plays(&bytes).unwrap(), Some(1));
    }
}
