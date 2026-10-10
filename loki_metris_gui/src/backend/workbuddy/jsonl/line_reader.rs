//! 有界完整行读取；正常行使用标准库缓冲扫描，超长行跳过而不继续分配。

use std::io::{self, BufRead};

/// 只有观察到换行才提交正常行或超长行；文件末尾半行留待下一轮读取。
#[derive(Debug, PartialEq, Eq)]
pub(super) enum LineRead {
    Complete,
    Oversized,
    End,
}

/// 保留换行之前的原始字节（含 CR），最多保留 limit 字节。
pub(super) fn read_complete_line(
    reader: &mut impl BufRead,
    line: &mut Vec<u8>,
    limit: usize,
) -> io::Result<LineRead> {
    assert!(limit > 0, "完整行读取上限必须为正数");
    line.clear();
    let mut oversized = false;
    loop {
        // 直接传播每次读取错误，与原逐字节读取的 Interrupted 语义一致。
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            return Ok(LineRead::End);
        }
        let newline = buffer.iter().position(|byte| *byte == b'\n');
        let content_len = newline.unwrap_or(buffer.len());
        if !oversized {
            if content_len <= limit - line.len() {
                line.extend_from_slice(&buffer[..content_len]);
            } else {
                line.clear();
                oversized = true;
            }
        }
        let consumed = content_len + usize::from(newline.is_some());
        reader.consume(consumed);
        if newline.is_some() {
            return Ok(if oversized { LineRead::Oversized } else { LineRead::Complete });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor, Read};

    /// 冻结原逐字节提交规则，独立于新标准库行扫描实现。
    fn old_lines(bytes: &[u8], limit: usize) -> Vec<(LineRead, Vec<u8>)> {
        let mut outcomes = Vec::new();
        let mut line = Vec::new();
        let mut oversized = false;
        for byte in bytes {
            if *byte == b'\n' {
                if oversized {
                    outcomes.push((LineRead::Oversized, Vec::new()));
                } else {
                    outcomes.push((LineRead::Complete, std::mem::take(&mut line)));
                }
                line.clear();
                oversized = false;
            } else if !oversized {
                if line.len() < limit {
                    line.push(*byte);
                } else {
                    line.clear();
                    oversized = true;
                }
            }
        }
        outcomes
    }

    /// 用真实 BufReader 的短缓冲反复跨越换行、CR、UTF-8 和超长行边界。
    #[test]
    fn buffered_lines_match_frozen_bytewise_rules_at_every_boundary() {
        for limit in [1_usize, 2, 7, 16, 31, 64, 16_384] {
            let mut bytes = "\n\r\n\0\n中文\n".as_bytes().to_vec();
            for length in [0, limit.saturating_sub(1), limit, limit + 1, limit * 2 + 3] {
                bytes.extend(std::iter::repeat_n(b'x', length));
                bytes.extend_from_slice(b"\nvalid\r\n");
            }
            for tail in [Vec::new(), vec![b'x'; limit], vec![b'x'; limit + 5]] {
                let mut input = bytes.clone();
                input.extend(tail);
                let expected = old_lines(&input, limit);
                for capacity in [1, 2, 3, 7, 16_384] {
                    let mut reader = BufReader::with_capacity(capacity, Cursor::new(&input));
                    let mut line = Vec::new();
                    let mut actual = Vec::new();
                    loop {
                        match read_complete_line(&mut reader, &mut line, limit).unwrap() {
                            LineRead::End => break,
                            outcome => actual.push((outcome, line.clone())),
                        }
                        assert!(line.len() <= limit);
                    }
                    assert_eq!(actual, expected, "limit={limit}, capacity={capacity}");
                }
            }
        }
    }

    /// 实际 1MiB 内容上限包含 CR；超长半行不计警告且不扩大内容缓冲。
    #[test]
    fn production_limit_keeps_exact_lines_and_discards_unterminated_oversize() {
        let limit = crate::backend::local_index::DEFAULT_MAX_JSONL_LINE_BYTES;
        let mut input = vec![b'x'; limit - 1];
        input.extend_from_slice(b"\r\n");
        input.extend(std::iter::repeat_n(b'x', limit + 1));
        input.extend_from_slice(b"\nnext\n");
        input.extend(std::iter::repeat_n(b'x', limit * 3));
        let mut reader = BufReader::with_capacity(16_384, Cursor::new(input));
        let mut line = Vec::with_capacity(16_384);
        assert_eq!(read_complete_line(&mut reader, &mut line, limit).unwrap(), LineRead::Complete);
        assert_eq!(line.len(), limit);
        assert_eq!(line.last(), Some(&b'\r'));
        assert_eq!(read_complete_line(&mut reader, &mut line, limit).unwrap(), LineRead::Oversized);
        assert!(line.is_empty());
        assert_eq!(read_complete_line(&mut reader, &mut line, limit).unwrap(), LineRead::Complete);
        assert_eq!(line, b"next");
        assert_eq!(read_complete_line(&mut reader, &mut line, limit).unwrap(), LineRead::End);
        assert!(line.capacity() <= limit);
    }

    /// 冻结前缀外的 LF 不得把半行升级为完整行，包括恰好内容上限的情况。
    #[test]
    fn fixed_prefix_never_commits_a_newline_outside_the_observed_length() {
        for length in [0, 6, 7, 8, 24] {
            let mut input = vec![b'x'; length];
            input.extend_from_slice(b"\nnext\n");
            for capacity in [1, 3, 16_384] {
                let prefix = Cursor::new(&input).take(length as u64);
                let mut reader = BufReader::with_capacity(capacity, prefix);
                let mut line = Vec::new();
                assert_eq!(read_complete_line(&mut reader, &mut line, 7).unwrap(), LineRead::End);
            }
        }
    }

    /// 在指定字节后固定失败的真实 Read，避免错误被后续成功读取掩盖。
    struct FaultAfterPrefix {
        prefix: Cursor<Vec<u8>>,
        kind: io::ErrorKind,
    }

    impl Read for FaultAfterPrefix {
        /// 先返回可读前缀，消费完后保留原错误类型。
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            if self.prefix.position() < self.prefix.get_ref().len() as u64 {
                self.prefix.read(output)
            } else {
                Err(io::Error::new(self.kind, "fixture read failure"))
            }
        }
    }

    /// 普通半行、恰好上限及超长跳过都传播错误；之前的完整行仍可独立提交。
    #[test]
    fn read_errors_preserve_the_original_kind_in_every_line_state() {
        for kind in [io::ErrorKind::Interrupted, io::ErrorKind::PermissionDenied, io::ErrorKind::Other] {
            for length in [0, 6, 7, 8, 24] {
                let mut prefix = b"ok\n".to_vec();
                prefix.extend(std::iter::repeat_n(b'x', length));
                for capacity in [1, 3, 16_384] {
                    let source = FaultAfterPrefix { prefix: Cursor::new(prefix.clone()), kind };
                    let mut reader = BufReader::with_capacity(capacity, source);
                    let mut line = Vec::new();
                    assert_eq!(read_complete_line(&mut reader, &mut line, 7).unwrap(), LineRead::Complete);
                    assert_eq!(line, b"ok");
                    assert_eq!(read_complete_line(&mut reader, &mut line, 7).unwrap_err().kind(), kind);
                }
            }
        }
    }
}
