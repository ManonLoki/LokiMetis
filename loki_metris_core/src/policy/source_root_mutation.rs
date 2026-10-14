//! 数据根变更归约（共享给 GUI、CLI、MCP）。


/// 表示数据根变更返回码，避免每个适配器重复映射文本与变更标志。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRootMutationKind {
    /// 首次成功添加数据根。
    SourceRootRegistered,
    /// 数据根已存在，无需再次写入。
    SourceRootAlreadyRegistered,
    /// 数据根启用成功。
    SourceRootEnabled,
    /// 数据根停用成功。
    SourceRootDisabled,
    /// 别名更新成功。
    SourceRootRenamed,
    /// 数据根移除成功。
    SourceRootRemoved,
    /// 主目录已切换。
    PrimarySourceRootChanged,
    /// 请求主目录已是当前主目录。
    PrimarySourceRootAlreadySelected,
    /// 主目录已清空。
    PrimarySourceRootCleared,
    /// 主目录未设置。
    PrimarySourceRootNotSet,
}

/// 数据根变更的通用结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRootMutationOutcome {
    /// 是否发生了持久化变更。
    pub changed: bool,
    /// 变更语义码。
    pub kind: SourceRootMutationKind,
}

impl SourceRootMutationOutcome {
    /// 便于调用端直接复用变化布尔位。
    pub const fn changed(self) -> bool {
        self.changed
    }
}

/// 将“已添加/已存在”分支统一归约为共享语义结果。
pub const fn source_root_add_outcome(added: bool) -> SourceRootMutationOutcome {
    if added {
        SourceRootMutationOutcome {
            changed: true,
            kind: SourceRootMutationKind::SourceRootRegistered,
        }
    } else {
        SourceRootMutationOutcome {
            changed: false,
            kind: SourceRootMutationKind::SourceRootAlreadyRegistered,
        }
    }
}

/// 将“启用/停用”分支在包含幂等返回时统一归约为共享语义结果。
pub const fn source_root_set_enabled_outcome(
    enabled: bool,
    changed: bool,
) -> SourceRootMutationOutcome {
    if enabled {
        SourceRootMutationOutcome {
            changed,
            kind: SourceRootMutationKind::SourceRootEnabled,
        }
    } else {
        SourceRootMutationOutcome {
            changed,
            kind: SourceRootMutationKind::SourceRootDisabled,
        }
    }
}

/// 将“更名”分支统一归约为共享语义结果。
pub const fn source_root_rename_outcome(changed: bool) -> SourceRootMutationOutcome {
    SourceRootMutationOutcome {
        changed,
        kind: SourceRootMutationKind::SourceRootRenamed,
    }
}

/// 将“移除”分支统一归约为共享语义结果。
pub const fn source_root_remove_outcome(changed: bool) -> SourceRootMutationOutcome {
    SourceRootMutationOutcome {
        changed,
        kind: SourceRootMutationKind::SourceRootRemoved,
    }
}

/// 将主目录设置场景的成功/幂等返回归约为共享语义结果。
pub const fn source_root_primary_outcome(
    requested_primary_root: bool,
    changed: bool,
) -> SourceRootMutationOutcome {
    match (requested_primary_root, changed) {
        (true, true) => SourceRootMutationOutcome {
            changed: true,
            kind: SourceRootMutationKind::PrimarySourceRootChanged,
        },
        (true, false) => SourceRootMutationOutcome {
            changed: false,
            kind: SourceRootMutationKind::PrimarySourceRootAlreadySelected,
        },
        (false, true) => SourceRootMutationOutcome {
            changed: true,
            kind: SourceRootMutationKind::PrimarySourceRootCleared,
        },
        (false, false) => SourceRootMutationOutcome {
            changed: false,
            kind: SourceRootMutationKind::PrimarySourceRootNotSet,
        },
    }
}
