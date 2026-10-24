// 本文件是 mod.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：对任意只读字节流执行有界 JSONL 解析，并逐调用交给流式 sink。；表示一条完整记录对流式解析状态产生的最小结果。；只反序列化一条完整记录中的允许字段，所有正文和工具字段由 serde 忽略。

/// 对任意只读字节流执行有界 JSONL 解析，并逐调用交给流式 sink。
/// 整体是一个手写的“逐字节扫描 + 状态机”实现，刻意不用
/// `BufRead::read_line` 之类的高层 API，原因有二：
///   1. 需要精确控制单行内存上限（`max_line_bytes`）——遇到异常巨大的
///      单行时主动丢弃而不是无限增长缓冲区，防止恶意/损坏文件耗尽内存；
///   2. 需要在任意读取间隙检查取消标志（`cancellation`），
///      让用户点“取消扫描”后能在很短时间内响应，而不必等一整个大文件读完。
///
/// `R: Read` 泛型让这个函数既能处理真实文件，也能在测试里处理内存中的
/// `&[u8]`，不关心底层是什么具体的字节来源。
pub fn parse_jsonl_stream<R, F>(
    mut reader: R,
    max_line_bytes: usize,
    context: &mut JsonlParseContext,
    provenance: &SourceProvenance,
    cancellation: &CancellationToken,
    mut on_call: F,
) -> Result<JsonlParseReport, LocalError>
where
    R: Read,
    F: FnMut(UsageCall) -> Result<(), LocalError>,
{
    let mut chunk = [0_u8; 16 * 1024]; // 每次系统调用读取的固定大小缓冲区
    let mut line = Vec::with_capacity(max_line_bytes.min(16 * 1024)); // 当前正在累积的一行
    let mut line_bytes = 0_u64; // 当前行的真实字节数（可能超过 line 的容量，用于统计）
    let mut bytes_consumed = 0_u64; // 本次调用总共读取的字节数（含尚未提交的半行）
    let mut committed_bytes = 0_u64; // 已确认到完整换行为止、可安全写入 checkpoint 的字节数
    let mut oversized = false; // 当前行是否已超过上限、正在被丢弃
    let mut non_usage_line = None;
    let mut warnings = JsonlWarningCounts::default();
    let mut calls_emitted = 0_u64;
    let mut cancelled = false;
    let mut snapshots = Vec::new();

    // 外层 'read 循环：每轮从底层 reader 读一块（chunk）字节；
    // 内层 for 循环：逐字节扫描这块数据，用 `\n` 切分成一行行处理，
    // 这样即使一行跨越了多个 chunk 边界也能正确累积。
    'read: loop {
        if cancellation.is_cancelled() {
            cancelled = true;
            break;
        }
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        for byte in &chunk[..read] {
            bytes_consumed = bytes_consumed.saturating_add(1);
            if *byte == b'\n' {
                // 遇到换行符，说明这一行已经完整：
                //   - 超长行只有完整通过有界 JSON 语法和非用量路由确认才跳过；
                //     其余计一次警告，不尝试恢复已丢弃的正文；
                //   - 否则去掉可能的尾随 `\r`（兼容 CRLF 换行）后真正解析这一行。
                if oversized {
                    if !non_usage_line.take().is_some_and(non_usage_line::NonUsageJsonLine::finish_and_check_complete) {
                        warnings.oversized_lines = warnings.oversized_lines.saturating_add(1);
                    }
                } else {
                    if line.last() == Some(&b'\r') {
                        line.pop();
                    }
                    match parse_complete_line(&line, context, provenance) {
                        LineOutcome::Call(call, used_cumulative_fallback, snapshot) => {
                            if used_cumulative_fallback {
                                warnings.cumulative_fallbacks =
                                    warnings.cumulative_fallbacks.saturating_add(1);
                            }
                            if let Some(snapshot) = snapshot {
                                snapshots.push(*snapshot);
                            }
                            on_call(*call)?;
                            calls_emitted = calls_emitted.saturating_add(1);
                        }
                        LineOutcome::Context => {}
                        LineOutcome::Ignored => {}
                        LineOutcome::Snapshot(snapshot) => {
                            snapshots.push(*snapshot);
                        }
                        LineOutcome::Unknown => {
                            warnings.unknown_events = warnings.unknown_events.saturating_add(1);
                        }
                        LineOutcome::Malformed => {
                            warnings.malformed_lines = warnings.malformed_lines.saturating_add(1);
                        }
                        LineOutcome::MissingUsage => {
                            warnings.missing_usage = warnings.missing_usage.saturating_add(1);
                        }
                        LineOutcome::InvalidUsage => {
                            warnings.invalid_usage = warnings.invalid_usage.saturating_add(1);
                        }
                        LineOutcome::InvalidTimestamp => {
                            warnings.invalid_timestamps =
                                warnings.invalid_timestamps.saturating_add(1);
                        }
                    }
                }
                line.clear();
                line_bytes = 0;
                oversized = false;
                // 只有处理完一条完整行之后，才把 checkpoint 的“已提交字节数”
                // 推进到当前位置——这保证了 checkpoint 永远落在完整行边界上，
                // 下次增量扫描从这里续读不会切断一行导致解析错误。
                committed_bytes = bytes_consumed;
                if cancellation.is_cancelled() {
                    cancelled = true;
                    break 'read;
                }
                continue;
            }

            line_bytes = line_bytes.saturating_add(1);
            if !oversized {
                if line.len() < max_line_bytes {
                    line.push(*byte);
                } else {
                    // 超过单行上限：清空已缓冲内容释放内存，并标记 oversized，
                    // 后续只做有界语法/类型验证，完整已知非用量行可安全越过。
                    let mut classifier = non_usage_line::NonUsageJsonLine::new();
                    classifier.push(&line);
                    classifier.push_byte(*byte);
                    non_usage_line = Some(classifier);
                    line.clear();
                    oversized = true;
                }
            } else if let Some(classifier) = &mut non_usage_line {
                classifier.push_byte(*byte);
            }
        }
    }

    if !cancelled && context.unresolved_legacy_prefix {
        warnings.unresolved_owned_boundaries = 1;
    }

    Ok(JsonlParseReport {
        calls_emitted,
        bytes_consumed,
        checkpoint: JsonlCheckpoint {
            committed_bytes,
            trailing_bytes: line_bytes,
            discarding_oversized_line: oversized,
        },
        warnings,
        cancelled,
        snapshots,
    })
}

/// 表示一条完整记录对流式解析状态产生的最小结果。
enum LineOutcome {
    /// 规范化出一个调用，并标识是否使用累计回退；可附带 cumulative 快照。
    Call(Box<UsageCall>, bool, Option<Box<SessionTokenSnapshot>>),
    /// 仅保存 cumulative 快照（IgnoreNonIncremental 但仍有 cumulative）。
    Snapshot(Box<SessionTokenSnapshot>),
    /// 更新了 session、模型、推理或项目上下文。
    Context,
    /// 已确认是正常但不参与用量统计的 rollout 记录。
    Ignored,
    /// 当前解析器不识别该事件。
    Unknown,
    /// 该行不是有效 JSON 对象。
    Malformed,
    /// Token 事件缺失可用的单次和累计事实。
    MissingUsage,
    /// Token 字段违反 core 不变量。
    InvalidUsage,
    /// Token 事件缺失或包含无法解析的发生时间。
    InvalidTimestamp,
}

/// 只反序列化一条完整记录中的允许字段，所有正文和工具字段由 serde 忽略。
/// 这是单行 JSONL 记录的核心路由函数，按事件种类分流到不同结果：
///   1. 反序列化失败 -> Malformed；
///   2. 已知的非用量事件（如纯文本消息、工具调用正文）-> Ignored
///      （明确识别但业务上不关心，区别于“完全不认识”）；
///   3. `session_meta`/`turn_context` -> 更新解析上下文（模型、项目等），
///      不产生调用记录；
///   4. `token_count` -> 真正的用量事件，继续往下解析 Token 字段；
///   5. 其他未知种类 -> Unknown（用于统计格式漂移，可能是上游新增了
///      本解析器还不认识的事件类型）。
///
/// 之所以要严格区分 Ignored 与 Unknown，是为了让“新版本 Codex 输出了
/// 全新事件格式”这种情况能被统计出来，而不是和“正常的非用量事件”混在一起。
fn parse_complete_line(
    line: &[u8],
    context: &mut JsonlParseContext,
    provenance: &SourceProvenance,
) -> LineOutcome {
    if line.is_empty() {
        return LineOutcome::Unknown;
    }
    // `protocol` 模块的 `RolloutEnvelope`/`RolloutPayload` 结构体
    // 只声明了业务需要的少数字段，serde 默认会忽略 JSON 里其余的所有字段
    // （包括提示词、回答正文等敏感内容）——“只反序列化白名单字段”本身
    // 就是本解析器不留存会话正文的关键手段。
    let envelope = match serde_json::from_slice::<RolloutEnvelope>(line) {
        Ok(envelope) => envelope,
        Err(_) => return LineOutcome::Malformed,
    };
    let envelope_kind = envelope.kind.as_deref();
    let payload = match envelope.payload {
        Some(payload) => payload,
        None if is_known_non_usage_envelope(envelope_kind) => return LineOutcome::Ignored,
        None => return LineOutcome::Unknown,
    };
    let payload_kind = payload.kind.as_deref();
    if envelope_kind == Some("token_usage_record") {
        return response::parse_response_record(context, payload, envelope.timestamp.as_ref(), provenance);
    }
    // 事件类型可能记录在信封层或 payload 层，任一层给出即可。
    let event_kind = payload_kind.or(envelope_kind);

    if event_kind == Some("session_meta") {
        if payload.id.as_deref().or(payload.session_id.as_deref())
            .is_some_and(|id| id.trim().is_empty() || id.len() > 512) {
            return LineOutcome::Malformed;
        }
        initialize_owner_context(
            context,
            &payload,
            parse_timestamp(envelope.timestamp.as_ref())
                .or_else(|| parse_timestamp(payload.timestamp.as_ref())),
        );
        return LineOutcome::Context;
    }
    if event_kind == Some("turn_context") {
        if context.owns_current_segment() {
            update_dynamic_context(context, &payload);
        }
        return LineOutcome::Context;
    }
    if event_kind == Some("task_started") {
        open_owned_segment_if_matching(context, parse_timestamp(payload.started_at.as_ref()));
        return LineOutcome::Context;
    }
    if event_kind == Some("thread_name_updated") {
        if context.owns_current_segment() {
            update_thread_title(context, &payload);
        }
        return LineOutcome::Context;
    }
    if event_kind != Some("token_count") {
        if is_known_non_usage_event(envelope_kind, payload_kind) {
            return LineOutcome::Ignored;
        }
        return LineOutcome::Unknown;
    }

    // 现代 writer 的 token_count 可被工具执行延迟，也可仅更新额度/上下文。
    // 可解码影子在 core 数值校验及上下文写入前忽略；无法解码的信封仍记坏行。
    if context.usage_authority.ignores_legacy_shadow() {
        return LineOutcome::Ignored;
    }

    let owned_segment = context.owns_current_segment();
    if owned_segment {
        update_dynamic_context(context, &payload);
        context.call_sequence = context.call_sequence.saturating_add(1);
    }
    let info = match payload.info {
        Some(info) => info,
        None if payload.rate_limits.is_some() => return LineOutcome::Ignored,
        None => return LineOutcome::MissingUsage,
    };
    let last = match info.last_token_usage.map(normalize_usage).transpose() {
        Ok(last) => last,
        Err(()) => return LineOutcome::InvalidUsage,
    };
    let cumulative = match info.total_token_usage.map(normalize_usage).transpose() {
        Ok(cumulative) => cumulative,
        Err(()) => return LineOutcome::InvalidUsage,
    };
    let occurred_at_epoch_ms = match parse_timestamp(envelope.timestamp.as_ref())
        .or_else(|| parse_timestamp(payload.timestamp.as_ref()))
    {
        Some(timestamp) => timestamp,
        None => return LineOutcome::InvalidTimestamp,
    };
    let decision = match context.usage_authority.select_legacy(
        last.clone(),
        cumulative.clone(),
        context.previous_cumulative.as_ref(),
        context.previous_last.as_ref(),
    ) {
        Ok(decision) => decision,
        Err(_) => return LineOutcome::MissingUsage,
    };
    if !owned_segment {
        // 旧协议没有线程身份；有效用量在自身边界确认前仍可能属于 owner。
        context.unresolved_legacy_prefix = true;
        if matches!(decision, IncrementalTokenUsageDecision::Emit(_)) {
            update_incremental_baseline(context, last, cumulative);
        }
        return LineOutcome::Ignored;
    }
    let snapshot = cumulative.as_ref().map(|usage| {
        Box::new(session_token_snapshot(
            context,
            occurred_at_epoch_ms,
            usage.total_tokens,
            payload
                .call_id
                .as_deref()
                .or(payload.turn_id.as_deref())
                .or(payload.request_id.as_deref()),
            provenance,
        ))
    });
    let selected = match decision {
        IncrementalTokenUsageDecision::Emit(selected) => selected,
        IncrementalTokenUsageDecision::IgnoreNonIncremental => {
            // 重放、same-last 与 total-only 估算都不是账单事实，累计基线保持上次已发出调用。
            // 兼容快照仍可落库供历史诊断，但它不再覆盖 ADR-104 调用总量。
            return match snapshot {
                Some(snapshot) => LineOutcome::Snapshot(snapshot),
                None => LineOutcome::Ignored,
            };
        }
    };
    update_incremental_baseline(context, last, cumulative.clone());
    let adapter_consistency_key = cumulative.as_ref().map(|usage| {
        encode_incremental_checkpoint(context.call_sequence, usage, context.previous_last.as_ref())
    });
    // `logical_call_id` 是 aggregate.rs 里去重合并的关键（同一次真实调用
    // 无论出现在活动会话还是归档备份里，指纹都必须一致）。三级优先级：
    //   1. 上游给了明确的 call_id/turn_id/request_id -> 直接用它拼指纹，
    //      最可靠；
    //   2. 用了“累计回退”（没有单次 last_token_usage，只能用整段累计值）
    //      -> 固定用一个特殊后缀，因为累计值本身没有天然的“单次”边界；
    //   3. 都没有 -> 退化用“线程键 + 序号 + 时间 + 全部 Token 数值”拼出一个
    //      尽量唯一的合成指纹，只是这种情况下两次几乎相同的调用理论上
    //      可能被误判为同一条（概率很低，属于已知取舍）。
    // `\u{0}`（NUL 字节）作分隔符：普通文本字段几乎不可能包含它，
    // 用来避免像 "ab"+"c" 和 "a"+"bc" 拼接后产生同一个指纹字符串的歧义。
    let explicit_call_id = payload
        .call_id
        .as_deref()
        .or(payload.turn_id.as_deref())
        .or(payload.request_id.as_deref());
    let fingerprint_material = if selected.used_cumulative_fallback {
        format!("{}\u{0}cumulative-fallback", context.thread_key)
    } else {
        match explicit_call_id {
            Some(call_id) => format!("{}\u{0}{call_id}", context.thread_key),
            None => format!(
                "{}\u{0}{}\u{0}{occurred_at_epoch_ms}\u{0}{}\u{0}{}\u{0}{}\u{0}{}\u{0}{}",
                context.thread_key,
                context.call_sequence,
                selected.usage.input_tokens,
                optional_token_fingerprint(selected.usage.cached_input_tokens),
                selected.usage.output_tokens,
                optional_token_fingerprint(selected.usage.reasoning_output_tokens),
                selected.usage.total_tokens,
            ),
        }
    };

    let logical_call_id = stable_id("call", &fingerprint_material);
    let snapshot = snapshot.map(|mut snapshot| {
        snapshot.logical_call_id = logical_call_id.clone();
        snapshot
    });
    LineOutcome::Call(
        Box::new(UsageCall {
            logical_call_id,
            occurred_at_epoch_ms,
            model: context.model.clone(),
            reasoning_effort: context.reasoning_effort.clone(),
            project_key: context.project_key.clone(),
            thread_key: context.thread_key.clone(),
            project_label: context.project_label.clone(),
            thread_label: context.thread_label.clone(),
            usage: selected.usage,
            adapter_consistency_key,
            confidence: selected.confidence,
            provenance: vec![provenance.clone()],
        }),
        selected.used_cumulative_fallback,
        snapshot,
    )
}

/// 为 token_count 的 cumulative.total 构造会话快照；Ignore 与 Emit 共用。
fn session_token_snapshot(
    context: &JsonlParseContext,
    occurred_at_epoch_ms: i64,
    cumulative_total_tokens: u64,
    explicit_call_id: Option<&str>,
    provenance: &SourceProvenance,
) -> SessionTokenSnapshot {
    let fingerprint_material = match explicit_call_id {
        Some(call_id) => format!("{0}\u{0}{call_id}\u{0}snapshot", context.thread_key),
        None => format!(
            "{0}\u{0}snapshot\u{0}{1}\u{0}{2}\u{0}{3}",
            context.thread_key,
            context.call_sequence,
            occurred_at_epoch_ms,
            cumulative_total_tokens
        ),
    };
    SessionTokenSnapshot {
        thread_key: context.thread_key.clone(),
        occurred_at_epoch_ms,
        cumulative_total_tokens,
        logical_call_id: stable_id("snap", &fingerprint_material),
        model: context.model.clone(),
        reasoning_effort: context.reasoning_effort.clone(),
        project_key: context.project_key.clone(),
        provenance: vec![provenance.clone()],
    }
}

/// 验证未知候选 rollout 的首条记录具有真实 session 元数据最小结构。
/// 这个函数不是用来解析真实数据的，而是“数据源发现”阶段的签名校验：
/// 遇到一个陌生目录/文件时，只读它的第一行，检查是不是长得像 Codex
/// rollout 文件（`session_meta` 事件 + 合法时间戳 + 非空 session id），
/// 从而拒绝把普通 JSONL 文件、日志文件误认成用量数据源，同时避免解析
/// 完整文件带来的开销。
pub fn is_rollout_signature_line(line: &[u8]) -> bool {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    if line.is_empty()
        || u64::try_from(line.len()).unwrap_or(u64::MAX) > ROLLOUT_SIGNATURE_MAX_BYTES
    {
        return false;
    }
    let Ok(envelope) = serde_json::from_slice::<RolloutEnvelope>(line) else {
        return false;
    };
    if envelope.kind.as_deref() != Some("session_meta")
        || parse_timestamp(envelope.timestamp.as_ref()).is_none()
    {
        return false;
    }
    envelope.payload.is_some_and(|payload| {
        payload
            .id
            .as_deref()
            .or(payload.session_id.as_deref())
            .is_some_and(|id| !id.trim().is_empty() && id.len() <= 512)
    })
}

#[cfg(test)]
mod ownership_tests;

#[cfg(test)]
mod utc_day_tests;

#[cfg(test)]
mod tests;
