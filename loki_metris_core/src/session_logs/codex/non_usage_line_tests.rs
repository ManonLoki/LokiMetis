// 本文件是 non_usage_line.rs 的分片（第 3/3 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：本模块的单元测试

#[cfg(test)]
mod tests {
    use super::*;

    /// 按单字节切片校验分类器，强制覆盖跨切块字符串与 UTF-8 状态。
    fn classified(input: &[u8]) -> bool {
        let mut line = NonUsageJsonLine::new();
        for byte in input {
            line.push(std::slice::from_ref(byte));
        }
        line.finish_and_check_complete()
    }

    /// 完整工具输出、压缩正文与 item_completed 不依赖键顺序或正文大小。
    #[test]
    fn classifies_complete_known_non_usage_without_retaining_body() {
        for kind in ["response_item", "compacted"] {
            let input = format!(
                r#"{{"payload":{{"output":"{}中😀","nested":{{"type":"token_count"}}}},"type":"{kind}"}}"#,
                "x".repeat(2 * 1024 * 1024)
            );
            assert!(classified(input.as_bytes()));
        }
        assert!(classified(br#"{"payload":{"item":{"text":"ignored"},"type":"item_completed"},"type":"event_msg"}"#));
    }

    /// 信封与直接 payload 类型必须唯一，内层已知类型不能冒充外层。
    #[test]
    fn rejects_duplicate_or_spoofed_protocol_fields() {
        for input in [
            r#"{"type":"token_usage_record","payload":{"type":"item_completed"}}"#,
            r#"{"type":"response_item","payload":{"type":"token_count"}}"#,
            r#"{"type":"response_item","type":"token_usage_record","payload":{}}"#,
            r#"{"type":"response_item","payload":{},"payload":{"type":"token_count"}}"#,
            r#"{"type":"event_msg","payload":{"type":"item_completed","type":"token_count"}}"#,
            r#"{"type":"event_msg","payload":{"item":{"type":"item_completed"}}}"#,
            r#"{"type":"future_usage","payload":{"type":"item_completed"}}"#,
            r#"{"type":"response_item","payload":"not an object"}"#,
        ] {
            assert!(!classified(input.as_bytes()), "{input}");
        }
    }

    /// 已知类型之后的损坏尾部、非法转义、UTF-8 与错误标点都不能解除警告。
    #[test]
    fn validates_entire_line_and_rejects_malformed_suffix() {
        for input in [
            r#"{"type":"response_item","payload":{"output":"unfinished"}"#,
            r#"{"type":"response_item","payload":{"output":"bad\q"}}"#,
            r#"{"type":"response_item","payload":{"output":"bad\u12xx"}}"#,
            r#"{"type":"response_item","payload":{"x":1,}}"#,
            r#"{"type":"response_item","payload":{"x":[1,]}}"#,
            r#"{"type":"response_item","payload":{"x":01}}"#,
            r#"{"type":"response_item","payload":{"x":truefalse}}"#,
            r#"{"type":"response_item"}{}"#,
            r#"{"type":"response_item","payload":{"x":[1}}}"#,
        ] {
            assert!(!classified(input.as_bytes()), "{input}");
        }
        let mut invalid = br#"{"type":"response_item","payload":{"x":""#.to_vec();
        invalid.extend_from_slice(&[0xc0, 0xaf]);
        invalid.extend_from_slice(br#""}}"#);
        assert!(!classified(&invalid));
    }

    /// 超深嵌套和巨大协议键/类型保守失败，正文大小不扩大任何缓冲上限。
    #[test]
    fn enforces_fixed_memory_budgets() {
        let deep = format!(
            r#"{{"type":"response_item","payload":{{"x":{}0{}}}}}"#,
            "[".repeat(MAX_DEPTH),
            "]".repeat(MAX_DEPTH)
        );
        assert!(!classified(deep.as_bytes()));
        let key = format!(
            r#"{{"type":"response_item","payload":{{"{}":0}}}}"#,
            "x".repeat(MAX_LABEL_BYTES)
        );
        assert!(!classified(key.as_bytes()));
        let atom = format!(
            r#"{{"type":"response_item","payload":{{"x":{}}}}}"#,
            "1".repeat(MAX_ATOM_BYTES + 1)
        );
        assert!(!classified(atom.as_bytes()));
        let mut line = NonUsageJsonLine::new();
        line.push(br#"{"type":"response_item","payload":{"output":""#);
        for _ in 0..10000 {
            line.push(&[b'x'; 1024]);
        }
        assert!(
            line.frames.len() <= MAX_DEPTH
                && line.label.len() <= MAX_LABEL_BYTES
                && line.atom.len() <= MAX_ATOM_BYTES
        );
        assert!(line.label.capacity() <= MAX_LABEL_BYTES && line.atom.capacity() <= MAX_ATOM_BYTES);
        line.push(br#""}}"#);
        assert!(line.finish_and_check_complete());
    }
}
