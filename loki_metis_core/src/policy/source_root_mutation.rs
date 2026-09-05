//! 数据根变更归约（共享给 GUI、CLI、MCP）。

use crate::{
    source_root_alias_updated_message, source_root_already_registered_message,
    source_root_disabled_message, source_root_enabled_message,
    source_root_primary_already_selected_message, source_root_primary_changed_message,
    source_root_primary_cleared_message, source_root_primary_not_set_message,
    source_root_registered_message, source_root_removed_message,
};

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

/// 将“启用/停用”分支统一归约为共享语义结果。
pub const fn source_root_toggle_enabled_outcome(enabled: bool) -> SourceRootMutationOutcome {
    source_root_set_enabled_outcome(enabled, true)
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
    if changed {
        SourceRootMutationOutcome {
            changed: true,
            kind: SourceRootMutationKind::SourceRootRenamed,
        }
    } else {
        SourceRootMutationOutcome {
            changed: false,
            kind: SourceRootMutationKind::SourceRootRenamed,
        }
    }
}

/// 将“移除”分支统一归约为共享语义结果。
pub const fn source_root_remove_outcome(changed: bool) -> SourceRootMutationOutcome {
    if changed {
        SourceRootMutationOutcome {
            changed: true,
            kind: SourceRootMutationKind::SourceRootRemoved,
        }
    } else {
        SourceRootMutationOutcome {
            changed: false,
            kind: SourceRootMutationKind::SourceRootRemoved,
        }
    }
}

/// 表示源根变更结果对应的通用消息代码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRootMutationMessageCode {
    /// 数据根已登记。
    SourceRegistered,
    /// 数据根已经存在，无需重复登记。
    SourceAlreadyRegistered,
    /// 数据根已启用。
    SourceEnabled,
    /// 数据根已停用。
    SourceDisabled,
    /// 数据根别名已更新。
    SourceRenamed,
    /// 数据根已从本产品索引移除。
    SourceRemoved,
    /// Codex 主数据目录已切换。
    PrimaryChanged,
    /// 尝试设置的主目录与当前一致。
    PrimaryAlreadySelected,
    /// 已清除主数据目录。
    PrimaryCleared,
    /// 当前无主数据目录。
    PrimaryNotSet,
}

/// 表示源根变更结果到文案与消息码的共享装配。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRootMutationFeedback {
    /// 变更对应的展示消息文本。
    pub message: String,
    /// 变更对应的稳定消息码。
    pub message_code: SourceRootMutationMessageCode,
}

/// 根据结果类型返回可复用的展示语义。
pub fn source_root_mutation_feedback(
    client_name: &str,
    kind: SourceRootMutationKind,
) -> SourceRootMutationFeedback {
    let (message, message_code): (String, SourceRootMutationMessageCode) = match kind {
        SourceRootMutationKind::SourceRootRegistered => (
            source_root_registered_message().to_owned(),
            SourceRootMutationMessageCode::SourceRegistered,
        ),
        SourceRootMutationKind::SourceRootAlreadyRegistered => (
            source_root_already_registered_message().to_owned(),
            SourceRootMutationMessageCode::SourceAlreadyRegistered,
        ),
        SourceRootMutationKind::SourceRootEnabled => (
            source_root_enabled_message().to_owned(),
            SourceRootMutationMessageCode::SourceEnabled,
        ),
        SourceRootMutationKind::SourceRootDisabled => (
            source_root_disabled_message(client_name),
            SourceRootMutationMessageCode::SourceDisabled,
        ),
        SourceRootMutationKind::SourceRootRenamed => (
            source_root_alias_updated_message().to_owned(),
            SourceRootMutationMessageCode::SourceRenamed,
        ),
        SourceRootMutationKind::SourceRootRemoved => (
            source_root_removed_message(client_name),
            SourceRootMutationMessageCode::SourceRemoved,
        ),
        SourceRootMutationKind::PrimarySourceRootChanged => (
            source_root_primary_changed_message().to_owned(),
            SourceRootMutationMessageCode::PrimaryChanged,
        ),
        SourceRootMutationKind::PrimarySourceRootAlreadySelected => (
            source_root_primary_already_selected_message().to_owned(),
            SourceRootMutationMessageCode::PrimaryAlreadySelected,
        ),
        SourceRootMutationKind::PrimarySourceRootCleared => (
            source_root_primary_cleared_message().to_owned(),
            SourceRootMutationMessageCode::PrimaryCleared,
        ),
        SourceRootMutationKind::PrimarySourceRootNotSet => (
            source_root_primary_not_set_message().to_owned(),
            SourceRootMutationMessageCode::PrimaryNotSet,
        ),
    };
    SourceRootMutationFeedback {
        message,
        message_code,
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
