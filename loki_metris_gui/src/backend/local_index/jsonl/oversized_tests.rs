//! 验证超长行只有完整换行后才能解除警告，取消与尾行仍可安全重放。

use super::*;
use std::io::Read;

/// 解析隔离字节流，不保存任何真实用户内容。
fn parse<R: Read>(reader: R, cancellation: &CancellationToken) -> JsonlParseReport {
    parse_jsonl_stream(
        reader,
        2048,
        &mut JsonlParseContext::new("fixture"),
        &SourceProvenance {
            source_id: "source".into(),
            root_id: "root".into(),
            relative_label: "sessions/fixture.jsonl".into(),
            archived: false,
        },
        cancellation,
        |_| Ok(()),
    )
    .expect("oversized fixture parses")
}

/// 已知大正文之后的合法用量仍被处理；未知、损坏、用量和上下文大行仍警告。
#[test]
fn oversized_known_non_usage_is_skipped_without_masking_other_lines() {
    let body = "x".repeat(20000);
    let input = format!(
        concat!(
            "{{\"type\":\"response_item\",\"payload\":{{\"type\":\"function_call_output\",\"output\":\"{0}\"}}}}\n",
            "{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"future_event\",\"body\":\"{0}\"}}}}\n",
            "{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"body\":\"{0}\"}}}}\n",
            "{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_started\",\"body\":\"{0}\"}}}}\n",
            "{{\"type\":\"response_item\",\"payload\":{{\"body\":\"{0}\",}}}}\n",
            "{{\"type\":\"event_msg\",\"timestamp\":\"2026-10-11T00:00:00Z\",\"payload\":{{\"type\":\"token_count\",\"info\":{{\"last_token_usage\":{{\"input_tokens\":10,\"output_tokens\":2,\"total_tokens\":12}}}}}}}}\n"
        ),
        body
    );
    let report = parse(input.as_bytes(), &CancellationToken::new());
    assert_eq!(report.calls_emitted, 1);
    assert_eq!(report.warnings.oversized_lines, 4);
    assert_eq!(report.warnings.total(), 4);
    assert_eq!(report.checkpoint.committed_bytes, input.len() as u64);
}

/// 第一次读取时在超长正文中取消，阻止下一块而不伪造完整行。
struct CancelAfterChunk<'a> {
    /// 剩余可读的字节。
    bytes: &'a [u8],
    /// 第一次读取后触发的取消令牌。
    cancellation: CancellationToken,
}

impl Read for CancelAfterChunk<'_> {
    /// 将第一块移交解析器后设置取消，模拟大行中途取消。
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let size = self.bytes.len().min(buffer.len());
        buffer[..size].copy_from_slice(&self.bytes[..size]);
        self.bytes = &self.bytes[size..];
        self.cancellation.cancel();
        Ok(size)
    }
}

/// 半行与取消不提交 classifier 的推断，下次从原起点重放完整大行。
#[test]
fn incomplete_and_cancelled_oversized_lines_remain_uncommitted() {
    let input = format!(
        "{{\"type\":\"response_item\",\"payload\":{{\"output\":\"{}\"}}}}\n",
        "x".repeat(40000)
    );
    let partial = parse(&input.as_bytes()[..10000], &CancellationToken::new());
    assert_eq!(partial.checkpoint.committed_bytes, 0);
    assert_eq!(partial.checkpoint.trailing_bytes, 10000);
    assert!(partial.checkpoint.discarding_oversized_line);
    assert_eq!(partial.warnings.total(), 0);
    let cancellation = CancellationToken::new();
    let cancelled = parse(
        CancelAfterChunk {
            bytes: input.as_bytes(),
            cancellation: cancellation.clone(),
        },
        &cancellation,
    );
    assert!(cancelled.cancelled && cancelled.checkpoint.discarding_oversized_line);
    assert_eq!(cancelled.checkpoint.committed_bytes, 0);
    assert_eq!(cancelled.warnings.total(), 0);
    let replay = parse(input.as_bytes(), &CancellationToken::new());
    assert_eq!(replay.checkpoint.committed_bytes, input.len() as u64);
    assert_eq!(replay.warnings.total(), 0);
}
