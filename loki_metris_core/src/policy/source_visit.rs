//! 来源文件是否进入本轮解析、以及解析结果中哪些调用允许入库的统一规则。
//!
//! 扫描窗口只负责"挑选要打开的文件"，不能再二次丢弃已经解析出来的调用：
//! 追加到已索引文件末尾的旧日期调用、被重建来源的窗口外历史，一旦在这里
//! 被丢弃，就会在 checkpoint 提交时随旧 generation 一起永久消失。

use crate::UsageCall;

/// adapter 对当前 generation 持久质量的解码结果；未知质量不能视为零警告。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistedSourceQuality {
    /// 缺失、损坏或不兼容，必须重新访问来源。
    Unknown,
    /// 已验证检查点中的当前有效警告数量。
    Known {
        /// 质量损失计数，不包含原始来源内容。
        warning_count: u64,
    },
}

/// 上次成功索引某来源时记录下来的文件事实，用于判断文件是否已变化。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceFileObservation {
    /// 上次 checkpoint 记录的文件字节数。
    pub observed_size: u64,
    /// 上次 checkpoint 记录的修改时间毫秒值。
    pub modified_at_epoch_ms: i64,
    /// 当前索引采用的 parser 是否能读取此 generation。
    pub parser_compatible: bool,
    /// 此 generation 的解析检查点与 Token 快照是否均已提交。
    pub checkpoint_complete: bool,
    /// 已采用质量合同的 adapter 必须提供；None 仅兼容未采用该合同的 adapter。
    pub persisted_quality: Option<PersistedSourceQuality>,
}

/// 候选来源的访问决定；保留来源时同步保留其质量事实。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceVisitDecision {
    /// 必须打开来源，执行增量解析或重建。
    Visit,
    /// 窗口外的已知来源未变化，直接保留当前 generation 及其质量。
    Retain {
        /// 保留来源对本轮覆盖质量的贡献。
        warning_count: u64,
    },
    /// 窗口外的未知来源不进入本轮成本范围。
    SkipUnindexed,
}

/// 先核实已索引来源的兼容性与质量，再应用扫描窗口，防止升级或坏行被跳过。
pub fn source_file_visit_decision(
    modified_at_epoch_ms: i64,
    observed_size: u64,
    scan_since_epoch_ms: i64,
    checkpoint: Option<SourceFileObservation>,
    force_rebuild: bool,
) -> SourceVisitDecision {
    if let Some(checkpoint) = checkpoint
        && (force_rebuild || !checkpoint.parser_compatible || !checkpoint.checkpoint_complete
            || checkpoint.persisted_quality == Some(PersistedSourceQuality::Unknown))
    {
        return SourceVisitDecision::Visit;
    }
    if modified_at_epoch_ms >= scan_since_epoch_ms {
        return SourceVisitDecision::Visit;
    }
    match checkpoint {
        None => SourceVisitDecision::SkipUnindexed,
        Some(checkpoint) if checkpoint.observed_size != observed_size
            || checkpoint.modified_at_epoch_ms != modified_at_epoch_ms => SourceVisitDecision::Visit,
        Some(checkpoint) => SourceVisitDecision::Retain {
            warning_count: match checkpoint.persisted_quality {
                Some(PersistedSourceQuality::Known { warning_count }) => warning_count,
                _ => 0,
            },
        },
    }
}

/// 决定枚举阶段是否要打开并解析一个候选来源文件。
///
/// - 修改时间落在扫描窗口内的文件始终进入解析（增量或重建由 adapter 决定）。
/// - 修改时间早于窗口、且从未索引过的文件不进入本轮，这是控制首次/周期成本的窄口。
/// - 修改时间早于窗口、但大小或修改时间与上次 checkpoint 不一致的文件必须进入
///   解析：它在两次扫描之间被追加或替换过，只按窗口跳过会让这些字节永远丢失。
pub fn source_file_needs_visit(
    modified_at_epoch_ms: i64,
    observed_size: u64,
    scan_since_epoch_ms: i64,
    checkpoint: Option<SourceFileObservation>,
) -> bool {
    source_file_visit_decision(modified_at_epoch_ms, observed_size, scan_since_epoch_ms,
        checkpoint, false) == SourceVisitDecision::Visit
}

/// 只保留发生时间不早于摄入下界的调用；下界应取派生库的永久保留窗口，
/// 而不是本轮扫描窗口，否则窗口外的历史会在重建或追加时被永久丢弃。
pub fn retain_ingestable_calls(calls: &mut Vec<UsageCall>, ingest_since_epoch_ms: i64) {
    calls.retain(|call| call.occurred_at_epoch_ms >= ingest_since_epoch_ms);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 窗口内文件不论是否已索引都必须进入解析。
    #[test]
    fn files_modified_inside_the_window_are_always_visited() {
        assert!(source_file_needs_visit(1_000, 10, 1_000, None));
        assert!(source_file_needs_visit(
            1_500,
            10,
            1_000,
            Some(SourceFileObservation {
                observed_size: 10,
                modified_at_epoch_ms: 1_500,
                parser_compatible: true,
                checkpoint_complete: true,
                persisted_quality: None,
            })
        ));
    }

    /// 窗口外且从未索引的文件是唯一允许跳过的“未知”来源。
    #[test]
    fn unknown_files_outside_the_window_are_skipped() {
        assert!(!source_file_needs_visit(999, 10, 1_000, None));
    }

    /// 窗口外但检查点一致的文件不重复打开。
    #[test]
    fn unchanged_indexed_files_outside_the_window_are_skipped() {
        assert!(!source_file_needs_visit(
            500,
            10,
            1_000,
            Some(SourceFileObservation {
                observed_size: 10,
                modified_at_epoch_ms: 500,
                parser_compatible: true,
                checkpoint_complete: true,
                persisted_quality: None,
            })
        ));
    }

    /// 窗口外但大小或修改时间与检查点不一致的文件必须重新解析。
    #[test]
    fn changed_indexed_files_outside_the_window_are_visited() {
        let checkpoint = Some(SourceFileObservation {
            observed_size: 10,
            modified_at_epoch_ms: 400,
            parser_compatible: true,
            checkpoint_complete: true,
            persisted_quality: None,
        });
        assert!(source_file_needs_visit(500, 10, 1_000, checkpoint));
        assert!(source_file_needs_visit(400, 12, 1_000, checkpoint));
    }

    /// 窗口不能掩盖旧 parser、未提交检查点、未知质量或显式重建。
    #[test]
    fn unverified_indexed_sources_bypass_the_time_window() {
        let trusted = SourceFileObservation {
            observed_size: 10, modified_at_epoch_ms: 400,
            parser_compatible: true, checkpoint_complete: true,
            persisted_quality: Some(PersistedSourceQuality::Known { warning_count: 7 }),
        };
        for checkpoint in [
            SourceFileObservation { parser_compatible: false, ..trusted },
            SourceFileObservation { checkpoint_complete: false, ..trusted },
            SourceFileObservation { persisted_quality: Some(PersistedSourceQuality::Unknown), ..trusted },
        ] {
            assert_eq!(source_file_visit_decision(400, 10, 1_000, Some(checkpoint), false), SourceVisitDecision::Visit);
        }
        assert_eq!(source_file_visit_decision(400, 10, 1_000, Some(trusted), true), SourceVisitDecision::Visit);
        assert_eq!(source_file_visit_decision(400, 10, 1_000, None, true), SourceVisitDecision::SkipUnindexed);
    }

    /// 未变化的窗口外来源保留原计数，不重复读取也不清除质量损失。
    #[test]
    fn retained_sources_preserve_verified_warning_counts() {
        for warning_count in [0, 1, u64::MAX] {
            let checkpoint = SourceFileObservation {
                observed_size: 10, modified_at_epoch_ms: 400,
                parser_compatible: true, checkpoint_complete: true,
                persisted_quality: Some(PersistedSourceQuality::Known { warning_count }),
            };
            assert_eq!(source_file_visit_decision(400, 10, 1_000, Some(checkpoint), false),
                SourceVisitDecision::Retain { warning_count });
        }
    }
}
