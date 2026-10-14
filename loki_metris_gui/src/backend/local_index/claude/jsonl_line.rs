// 本文件是 jsonl.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：单行 transcript 解析与 ID 校验，以及本模块单元测试

/// 解析单行 JSON 为分类结果，只提取判定用量所需的最小字段。
fn parse_line(
    line: &[u8],
    context: &mut ClaudeJsonlParseContext,
    provenance: &SourceProvenance,
) -> LineOutcome {
    if line.is_empty() {
        return LineOutcome::Unknown;
    }
    let record = match serde_json::from_slice::<TranscriptRecord>(line) {
        Ok(record) => record,
        Err(_) => return LineOutcome::Malformed,
    };
    let kind = record.kind.as_deref();
    if kind != Some("assistant") {
        // `cost-state` 只是 session 级累计快照（与逐条 assistant usage 重复计数），
        // 其余是会话/权限/文件历史等元数据，都不携带需要单独入库的用量。
        return if matches!(
            kind,
            Some(
                "user"
                    | "system"
                    | "summary"
                    | "progress"
                    | "queue-operation"
                    | "file-history-snapshot"
                    | "file-history-delta"
                    | "attachment"
                    | "last-prompt"
                    | "mode"
                    | "permission-mode"
                    | "atis-latch"
                    | "bridge-session"
                    | "cost-state"
            )
        ) {
            LineOutcome::Ignored
        } else {
            LineOutcome::Unknown
        };
    }
    let session_id = match record.session_id.as_deref().filter(|value| valid_id(value)) {
        Some(value) => value,
        None => return LineOutcome::MissingUsage,
    };
    let occurred_at_epoch_ms = match record
        .timestamp
        .as_deref()
        .and_then(|value| value.parse::<Timestamp>().ok())
        .map(|value| value.as_millisecond())
    {
        Some(value) => value,
        None => return LineOutcome::InvalidTimestamp,
    };
    let reasoning_effort = record.effort.as_deref().and_then(safe_technical_label);
    let message = match record.message {
        Some(message) => message,
        None => return LineOutcome::MissingUsage,
    };
    let message_id = match message.id.as_deref().filter(|value| valid_id(value)) {
        Some(value) => value,
        None => return LineOutcome::MissingUsage,
    };
    let usage = match message.usage {
        Some(usage) => usage,
        None => return LineOutcome::MissingUsage,
    };
    let Some(uncached_input) = usage.input_tokens else {
        return LineOutcome::MissingUsage;
    };
    let Some(output_tokens) = usage.output_tokens else {
        return LineOutcome::MissingUsage;
    };
    let cached_input = usage.cache_read_input_tokens.unwrap_or_default();
    let cache_write = usage.cache_creation_input_tokens.unwrap_or_default();
    let total_input = match uncached_input
        .checked_add(cached_input)
        .and_then(|value| value.checked_add(cache_write))
    {
        Some(value) => value,
        None => return LineOutcome::InvalidUsage,
    };
    let usage = match TokenUsage::new_with_availability(
        total_input,
        Some(cached_input),
        Some(cache_write),
        output_tokens,
        None,
        None,
    ) {
        Ok(usage) => usage,
        Err(_) => return LineOutcome::InvalidUsage,
    };
    let logical_call_id = stable_id(
        "claude-call",
        &format!("{}\u{0}{session_id}\u{0}{message_id}", context.thread_key),
    );
    LineOutcome::Call(Box::new(UsageCall {
        logical_call_id,
        occurred_at_epoch_ms,
        model: message.model.as_deref().and_then(safe_model_label),
        reasoning_effort,
        project_key: context.project_key.clone(),
        thread_key: context.thread_key.clone(),
        project_label: context.project_label.clone(),
        thread_label: None,
        usage,
        adapter_consistency_key: None,
        confidence: Confidence::Exact,
        provenance: vec![provenance.clone()],
    }))
}

/// 校验 session/message ID 非空、有界且不含控制字符。
fn valid_id(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造测试用的固定来源标注。
    fn provenance() -> SourceProvenance {
        SourceProvenance {
            source_id: "source-test".to_owned(),
            root_id: "root-test".to_owned(),
            relative_label: "project/session.jsonl".to_owned(),
            archived: false,
        }
    }

    /// 验证三条相邻单调观察被合并为一条调用，且缓存计入总输入 Token。
    #[test]
    fn coalesces_three_monotonic_observations_and_maps_cache_into_total_input() {
        let input = concat!(
            "{\"type\":\"assistant\",\"sessionId\":\"session-a\",\"timestamp\":\"2026-07-31T08:00:00Z\",\"message\":{\"id\":\"msg-a\",\"model\":\"claude-test\",\"usage\":{\"input_tokens\":10,\"cache_read_input_tokens\":20,\"cache_creation_input_tokens\":30,\"output_tokens\":1},\"content\":\"private bait\"}}\n",
            "{\"type\":\"assistant\",\"sessionId\":\"session-a\",\"timestamp\":\"2026-07-31T08:00:01Z\",\"message\":{\"id\":\"msg-a\",\"model\":\"claude-test\",\"usage\":{\"input_tokens\":10,\"cache_read_input_tokens\":20,\"cache_creation_input_tokens\":30,\"output_tokens\":2}}}\n",
            "{\"type\":\"assistant\",\"sessionId\":\"session-a\",\"timestamp\":\"2026-07-31T08:00:02Z\",\"message\":{\"id\":\"msg-a\",\"model\":\"claude-test\",\"usage\":{\"input_tokens\":10,\"cache_read_input_tokens\":20,\"cache_creation_input_tokens\":30,\"output_tokens\":3}}}\n"
        );
        let mut context = ClaudeJsonlParseContext::new("fixture", Some("project-a"));
        let mut calls = Vec::new();
        let report = parse_claude_jsonl_stream(
            input.as_bytes(),
            1024 * 1024,
            &mut context,
            &provenance(),
            &CancellationToken::new(),
            |call| {
                calls.push(call);
                Ok(())
            },
        )
        .expect("fixture parses");

        assert_eq!(report.calls_emitted, 1);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].usage.input_tokens, 60);
        assert_eq!(calls[0].usage.cached_input_tokens, Some(20));
        assert_eq!(calls[0].usage.cache_write_input_tokens, Some(30));
        assert_eq!(calls[0].usage.output_tokens, 3);
        assert_eq!(calls[0].usage.total_tokens, 63);
        assert_eq!(calls[0].usage.reasoning_output_tokens, None);
        assert_eq!(calls[0].reasoning_effort, None);
    }

    /// 验证数值回退的观察被拒绝合并，且原始消息正文从不被保留。
    #[test]
    fn rejects_regression_and_never_retains_message_content() {
        let input = concat!(
            "{\"type\":\"assistant\",\"sessionId\":\"session-a\",\"timestamp\":\"2026-07-31T08:00:00Z\",\"message\":{\"id\":\"msg-a\",\"model\":\"claude-test\",\"usage\":{\"input_tokens\":10,\"output_tokens\":8},\"content\":\"do-not-store-this\"}}\n",
            "{\"type\":\"assistant\",\"sessionId\":\"session-a\",\"timestamp\":\"2026-07-31T08:00:01Z\",\"message\":{\"id\":\"msg-a\",\"model\":\"claude-test\",\"usage\":{\"input_tokens\":10,\"output_tokens\":7}}}\n"
        );
        let mut context = ClaudeJsonlParseContext::new("fixture", None);
        let mut calls = Vec::new();
        let report = parse_claude_jsonl_stream(
            input.as_bytes(),
            1024,
            &mut context,
            &provenance(),
            &CancellationToken::new(),
            |call| {
                calls.push(call);
                Ok(())
            },
        )
        .expect("fixture parses");

        assert_eq!(report.warnings.regressed_observations, 1);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].usage.output_tokens, 8);
        let debug = format!("{calls:?}");
        assert!(!debug.contains("do-not-store-this"));
    }

    /// 验证即使缓存增长掩盖了总量下降，原始输入 Token 的回退也会被识别。
    #[test]
    fn rejects_raw_input_regression_even_when_cache_growth_masks_the_total() {
        let input = concat!(
            "{\"type\":\"assistant\",\"sessionId\":\"session-a\",\"timestamp\":\"2026-07-31T08:00:00Z\",\"message\":{\"id\":\"msg-a\",\"model\":\"claude-test\",\"usage\":{\"input_tokens\":10,\"cache_read_input_tokens\":20,\"cache_creation_input_tokens\":30,\"output_tokens\":1}}}\n",
            "{\"type\":\"assistant\",\"sessionId\":\"session-a\",\"timestamp\":\"2026-07-31T08:00:01Z\",\"message\":{\"id\":\"msg-a\",\"model\":\"claude-test\",\"usage\":{\"input_tokens\":9,\"cache_read_input_tokens\":20,\"cache_creation_input_tokens\":31,\"output_tokens\":2}}}\n"
        );
        let mut context = ClaudeJsonlParseContext::new("fixture", None);
        let mut calls = Vec::new();
        let report = parse_claude_jsonl_stream(
            input.as_bytes(),
            1024,
            &mut context,
            &provenance(),
            &CancellationToken::new(),
            |call| {
                calls.push(call);
                Ok(())
            },
        )
        .expect("fixture parses");

        assert_eq!(report.warnings.regressed_observations, 1);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].usage.output_tokens, 1);
        assert_eq!(calls[0].usage.cache_write_input_tokens, Some(30));
    }

    /// 验证畸形、超长、未完结与未知行都只计入告警，不产生任何调用。
    #[test]
    fn reports_malformed_oversized_partial_and_unknown_records_without_calls() {
        let input =
            b"{malformed}\n{\"type\":\"future-event\"}\n123456789\n{\"type\":\"assistant\"}";
        let mut context = ClaudeJsonlParseContext::new("fixture", None);
        let report = parse_claude_jsonl_stream(
            &input[..],
            8,
            &mut context,
            &provenance(),
            &CancellationToken::new(),
            |_| Ok(()),
        )
        .expect("fixture is processed");

        assert_eq!(report.calls_emitted, 0);
        assert!(report.warnings.oversized_lines >= 2);
        assert!(report.checkpoint.trailing_bytes > 0);
    }

    /// 验证 Token 计数溢出被记为质量告警，而不是发生数值回绕。
    #[test]
    fn overflow_is_a_quality_warning_instead_of_wrapping() {
        let input = format!(
            "{{\"type\":\"assistant\",\"sessionId\":\"session-a\",\"timestamp\":\"2026-07-31T08:00:00Z\",\"message\":{{\"id\":\"msg-a\",\"model\":\"claude-test\",\"usage\":{{\"input_tokens\":{},\"cache_read_input_tokens\":1,\"output_tokens\":1}}}}}}\n",
            u64::MAX
        );
        let mut context = ClaudeJsonlParseContext::new("fixture", None);
        let report = parse_claude_jsonl_stream(
            input.as_bytes(),
            1024,
            &mut context,
            &provenance(),
            &CancellationToken::new(),
            |_| Ok(()),
        )
        .expect("overflow fixture is processed");
        assert_eq!(report.calls_emitted, 0);
        assert_eq!(report.warnings.invalid_usage, 1);
    }

    /// 验证只忽略已知的附件类事件，未来新事件类型会计入未知告警。
    #[test]
    fn ignores_only_known_attachment_events_and_warns_on_future_types() {
        let input = concat!(
            "{\"type\":\"attachment\",\"message\":{\"content\":\"private bait\"}}\n",
            "{\"type\":\"future-attachment-like\",\"message\":{\"content\":\"private bait\"}}\n"
        );
        let mut context = ClaudeJsonlParseContext::new("fixture", None);
        let report = parse_claude_jsonl_stream(
            input.as_bytes(),
            1024,
            &mut context,
            &provenance(),
            &CancellationToken::new(),
            |_| Ok(()),
        )
        .expect("attachment fixture is processed");

        assert_eq!(report.calls_emitted, 0);
        assert_eq!(report.warnings.unknown_events, 1);
        assert_eq!(report.warnings.total(), 1);
    }

    /// 当前 Claude Code 写出的会话/权限/文件历史/累计成本元数据行都是已知非用量事件，
    /// 不得再计入未知告警把覆盖压成 Partial；`cost-state` 的累计快照也不得单独入库。
    #[test]
    fn ignores_current_claude_metadata_events_without_warnings_or_calls() {
        let input = concat!(
            "{\"type\":\"last-prompt\",\"sessionId\":\"s\",\"lastPrompt\":\"private bait\"}\n",
            "{\"type\":\"mode\",\"sessionId\":\"s\",\"mode\":\"default\"}\n",
            "{\"type\":\"permission-mode\",\"sessionId\":\"s\",\"permissionMode\":\"default\"}\n",
            "{\"type\":\"atis-latch\",\"sessionId\":\"s\",\"latched\":true}\n",
            "{\"type\":\"bridge-session\",\"sessionId\":\"s\",\"ownerAccountUuid\":\"private bait\"}\n",
            "{\"type\":\"file-history-delta\",\"sessionId\":\"s\",\"delta\":{}}\n",
            "{\"type\":\"cost-state\",\"sessionId\":\"s\",\"modelUsage\":{\"claude-test\":{\"inputTokens\":99,\"outputTokens\":99}}}\n"
        );
        let mut context = ClaudeJsonlParseContext::new("fixture", None);
        let report = parse_claude_jsonl_stream(
            input.as_bytes(),
            1024,
            &mut context,
            &provenance(),
            &CancellationToken::new(),
            |_| Ok(()),
        )
        .expect("metadata fixture is processed");

        assert_eq!(report.calls_emitted, 0);
        assert_eq!(report.warnings.total(), 0);
    }

    /// 助手用量行上的 `effort` 必须进入 shipped `UsageCall.reasoning_effort`。
    #[test]
    fn persists_assistant_effort_label_on_the_call() {
        let input = concat!(
            "{\"type\":\"assistant\",\"sessionId\":\"session-a\",\"timestamp\":\"2026-07-31T08:00:00Z\",",
            "\"effort\":\"high\",\"message\":{\"id\":\"msg-a\",\"model\":\"claude-sonnet-5\",",
            "\"usage\":{\"input_tokens\":10,\"output_tokens\":2}}}\n"
        );
        let mut context = ClaudeJsonlParseContext::new("fixture", None);
        let mut calls = Vec::new();
        parse_claude_jsonl_stream(
            input.as_bytes(),
            1024,
            &mut context,
            &provenance(),
            &CancellationToken::new(),
            |call| {
                calls.push(call);
                Ok(())
            },
        )
        .expect("effort fixture parses");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].reasoning_effort.as_deref(), Some("high"));
        assert_eq!(calls[0].model.as_deref(), Some("claude-sonnet-5"));
    }

    /// 缺少 effort 的助手用量行必须保持推理强度为 none，不得发明值。
    #[test]
    fn omits_reasoning_effort_when_assistant_line_has_no_effort() {
        let input = concat!(
            "{\"type\":\"assistant\",\"sessionId\":\"session-a\",\"timestamp\":\"2026-07-31T08:00:00Z\",",
            "\"message\":{\"id\":\"msg-a\",\"model\":\"claude-test\",",
            "\"usage\":{\"input_tokens\":10,\"output_tokens\":2}}}\n"
        );
        let mut context = ClaudeJsonlParseContext::new("fixture", None);
        let mut calls = Vec::new();
        parse_claude_jsonl_stream(
            input.as_bytes(),
            1024,
            &mut context,
            &provenance(),
            &CancellationToken::new(),
            |call| {
                calls.push(call);
                Ok(())
            },
        )
        .expect("no-effort fixture parses");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].reasoning_effort, None);
    }

    /// 路径型 effort 与凭据对象里的邮箱都不得进入调用字段。
    #[test]
    fn rejects_path_like_effort_and_does_not_copy_credential_email() {
        let input = concat!(
            "{\"type\":\"assistant\",\"sessionId\":\"session-a\",\"timestamp\":\"2026-07-31T08:00:00Z\",",
            "\"effort\":\"/Users/alice/secret\",\"auth\":{\"email\":\"alice@example.com\"},",
            "\"message\":{\"id\":\"msg-a\",\"model\":\"claude-test\",",
            "\"usage\":{\"input_tokens\":10,\"output_tokens\":2}}}\n"
        );
        let mut context = ClaudeJsonlParseContext::new("fixture", None);
        let mut calls = Vec::new();
        parse_claude_jsonl_stream(
            input.as_bytes(),
            1024,
            &mut context,
            &provenance(),
            &CancellationToken::new(),
            |call| {
                calls.push(call);
                Ok(())
            },
        )
        .expect("unsafe effort still yields a usage call");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].reasoning_effort, None);
        assert_eq!(calls[0].model.as_deref(), Some("claude-test"));
        let debug = format!("{calls:?}");
        assert!(!debug.contains("alice@example.com"));
        assert!(!debug.contains("/Users/alice"));
    }
}
