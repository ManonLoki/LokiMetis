//! 检查单个候选根的会话区域与首记录签名；被 [`super::quick`] 和
//! [`super::full_device`] 两条发现路径共用，避免各自维护一份符号链接/签名判定。

use std::fs;
use std::path::Path;

use loki_metis_core::{path_key, stable_id};

use super::{DiscoveredRoot, FullDiscoveryOptions};
use crate::backend::local_index::file_source::{
    RolloutFileInspection, RolloutProbeBudget, inspect_rollout_file, is_rollout_jsonl,
};
use crate::backend::local_index::{
    CancellationToken, DiscoveryMethod, LocalPathStatus, classify_local_path,
    is_obviously_network_path,
};

/// 判断两个已经通过本地卷与链接边界校验的路径是否指向同一物理目录。
///
/// 调用方必须先完成各客户端的严格结构签名；身份读取失败时返回错误，调用方不得据此合并。
pub(crate) fn same_physical_directory(left: &Path, right: &Path) -> std::io::Result<bool> {
    if left == right {
        return Ok(true);
    }
    #[cfg(target_os = "windows")]
    {
        // `same-file 1.0.6` 使用 64-bit file index，而 ReFS 的唯一 ID 是 128-bit；
        // 对 ReFS alias 保守不合并，避免两个不同目录被截断后的 ID 误判为同一目录。
        let left_volume = whichdisk::resolve(left)?;
        let right_volume = whichdisk::resolve(right)?;
        if left_volume.fs_type().eq_ignore_ascii_case("refs")
            || right_volume.fs_type().eq_ignore_ascii_case("refs")
        {
            return Ok(false);
        }
    }
    // `same_file` crate 通过查询设备号 + 文件索引（Unix 是 device+inode，
    // Windows 是 volume serial + file index）来判断两个路径是否指向磁盘上
    // 同一份物理数据，而不是简单比较字符串路径——同一目录完全可能通过
    // 不同路径（符号链接、挂载点、大小写差异等）被访问到，这里要的是
    // “物理身份相同”而不是“字面路径相同”。
    same_file::is_same_file(left, right)
}

/// 重新确认目录仍位于本地持久卷、没有链接组件，且末级仍是普通目录。
///
/// 手动添加在结构签名与数据库登记之间仍可能遇到路径替换；registry 在比较物理
/// 身份前必须再次执行这条窄边界，并把不确定结果当作错误而不是“不同目录”。
pub(crate) fn validate_local_plain_directory(path: &Path) -> std::io::Result<()> {
    if is_obviously_network_path(path) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "network directory is not eligible",
        ));
    }
    if reject_symlink_components(path)? {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "linked directory is not eligible",
        ));
    }
    match classify_local_path(path) {
        LocalPathStatus::ConfirmedLocal => {}
        LocalPathStatus::RejectedNetwork => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "network directory is not eligible",
            ));
        }
        LocalPathStatus::Missing => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "directory disappeared before registration",
            ));
        }
        LocalPathStatus::Indeterminate => {
            return Err(std::io::Error::other(
                "directory volume identity is indeterminate",
            ));
        }
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata_is_link_like(&metadata) || !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "candidate is not a plain directory",
        ));
    }
    Ok(())
}

/// 比较一个历史 registry 路径与刚通过签名的候选目录。
///
/// 候选不再安全时传播错误并禁止写入；历史路径已缺失、变成链接/网络路径或不再是
/// 普通目录时只是不具备去重资格。权限或卷身份不确定仍传播错误，避免制造重复行。
pub(crate) fn registered_path_matches_candidate(
    registered: &Path,
    candidate: &Path,
) -> std::io::Result<bool> {
    validate_local_plain_directory(candidate)?;
    match validate_local_plain_directory(registered) {
        Ok(()) => same_physical_directory(registered, candidate),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidInput
            ) =>
        {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

/// 保存一次主动发现任务共享的结构探测取消令牌与硬预算。
pub(crate) struct SignatureProbeContext<'a> {
    /// 用于中途响应取消请求的令牌。
    pub(crate) cancellation: &'a CancellationToken,
    /// 剩余允许遍历的目录数预算。
    pub(crate) remaining_directories: u64,
    /// 剩余允许处理的目录项数预算。
    pub(crate) remaining_entries: u64,
    /// 剩余的 rollout 签名探测预算。
    pub(crate) rollout_budget: RolloutProbeBudget,
}

impl<'a> SignatureProbeContext<'a> {
    /// 从公开发现边界建立任务级预算，候选之间不会重新计数。
    pub(crate) fn new(options: &FullDiscoveryOptions, cancellation: &'a CancellationToken) -> Self {
        Self {
            cancellation,
            remaining_directories: options.max_signature_directories,
            remaining_entries: options.max_signature_entries,
            rollout_budget: RolloutProbeBudget::new(
                options.max_signature_files,
                options.max_signature_bytes,
            ),
        }
    }
}

/// 区分有效根、普通目录、结构拒绝、用户取消与安全预算耗尽。
pub(crate) enum RootInspection {
    Found(DiscoveredRoot),
    NotRoot,
    RejectedSignature,
    Indeterminate,
    Cancelled,
    BudgetExhausted,
}

/// 描述一个会话区域的最小 rollout 结构探测结果，并把 I/O 不确定与确认拒绝分开。
enum SignatureInspection {
    Matched,
    Rejected,
    Indeterminate,
    Cancelled,
    BudgetExhausted,
}

/// 检查候选根的会话区域；所有入口都必须存在有效 rollout 首记录。
pub(crate) fn inspect_root(
    path: &Path,
    alias: String,
    method: DiscoveryMethod,
    existing_root_id: Option<&str>,
    mut signature_probe: Option<&mut SignatureProbeContext<'_>>,
) -> RootInspection {
    if is_forbidden_auxiliary_root(path) {
        return RootInspection::RejectedSignature;
    }
    let (sessions_directory, sessions_inspection_complete) =
        inspect_plain_directory(&path.join("sessions"));
    let (archived_directory, archived_sessions_inspection_complete) =
        inspect_plain_directory(&path.join("archived_sessions"));
    if signature_probe.is_none() {
        if !sessions_directory && !archived_directory {
            return if sessions_inspection_complete && archived_sessions_inspection_complete {
                RootInspection::NotRoot
            } else {
                RootInspection::Indeterminate
            };
        }
    } else {
        let mut matched = false;
        let mut indeterminate =
            !sessions_inspection_complete || !archived_sessions_inspection_complete;
        for area in [
            sessions_directory.then(|| path.join("sessions")),
            archived_directory.then(|| path.join("archived_sessions")),
        ]
        .into_iter()
        .flatten()
        {
            let probe = signature_probe
                .as_deref_mut()
                .expect("signature context remains available");
            match inspect_rollout_area(&area, probe) {
                SignatureInspection::Matched => {
                    matched = true;
                    break;
                }
                SignatureInspection::Rejected => {}
                SignatureInspection::Indeterminate => indeterminate = true,
                SignatureInspection::Cancelled => return RootInspection::Cancelled,
                SignatureInspection::BudgetExhausted => {
                    return RootInspection::BudgetExhausted;
                }
            }
        }
        if !matched {
            return if indeterminate {
                RootInspection::Indeterminate
            } else if sessions_directory || archived_directory {
                RootInspection::RejectedSignature
            } else {
                RootInspection::NotRoot
            };
        }
    }
    RootInspection::Found(DiscoveredRoot {
        path: path.to_path_buf(),
        root_id: existing_root_id
            .map(str::to_owned)
            .unwrap_or_else(|| stable_id("root", &path_key(path))),
        alias,
        discovery_method: method,
        has_sessions: sessions_directory,
        sessions_inspection_complete,
        has_archived_sessions: archived_directory,
        archived_sessions_inspection_complete,
    })
}

/// 按物理末级目录名拒绝固定辅助根；安全别名不能改变这条数据源边界。
pub(super) fn is_forbidden_auxiliary_root(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            [
                "log", "logs", "cache", "caches", "tmp", "temp", "run", "runtime",
            ]
            .iter()
            .any(|forbidden| name.eq_ignore_ascii_case(forbidden))
        })
}

/// 在固定目录与文件预算内寻找一个符合文件名和首记录结构的普通 rollout 文件。
fn inspect_rollout_area(
    area: &Path,
    context: &mut SignatureProbeContext<'_>,
) -> SignatureInspection {
    let mut stack = vec![area.to_path_buf()];
    let mut indeterminate = false;
    while let Some(directory) = stack.pop() {
        if context.cancellation.is_cancelled() {
            return SignatureInspection::Cancelled;
        }
        if context.remaining_directories == 0 {
            return SignatureInspection::BudgetExhausted;
        }
        context.remaining_directories -= 1;
        let mut entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(_) => {
                indeterminate = true;
                continue;
            }
        };
        loop {
            if context.cancellation.is_cancelled() {
                return SignatureInspection::Cancelled;
            }
            let Some(entry) = entries.next() else {
                break;
            };
            if context.remaining_entries == 0 {
                return SignatureInspection::BudgetExhausted;
            }
            context.remaining_entries -= 1;
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    indeterminate = true;
                    continue;
                }
            };
            let metadata = match fs::symlink_metadata(entry.path()) {
                Ok(metadata) => metadata,
                Err(_) => {
                    indeterminate = true;
                    continue;
                }
            };
            if metadata_is_link_like(&metadata) {
                continue;
            }
            if metadata.is_dir() {
                stack.push(entry.path());
                continue;
            }
            if !metadata.is_file() || !is_rollout_jsonl(&entry.path()) {
                continue;
            }
            match inspect_rollout_file(
                &entry.path(),
                context.cancellation,
                &mut context.rollout_budget,
            ) {
                RolloutFileInspection::Matched(_) => return SignatureInspection::Matched,
                RolloutFileInspection::Rejected => {}
                RolloutFileInspection::Indeterminate => indeterminate = true,
                RolloutFileInspection::Cancelled => return SignatureInspection::Cancelled,
                RolloutFileInspection::BudgetExhausted => {
                    return SignatureInspection::BudgetExhausted;
                }
            }
        }
    }
    if indeterminate {
        SignatureInspection::Indeterminate
    } else {
        SignatureInspection::Rejected
    }
}

/// 区分明确存在/不存在与 I/O 不确定，避免后续清理无法枚举区域的旧索引。
fn inspect_plain_directory(path: &Path) -> (bool, bool) {
    match fs::symlink_metadata(path) {
        Ok(metadata) => (metadata.is_dir() && !metadata_is_link_like(&metadata), true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (false, true),
        Err(_) => (false, false),
    }
}

/// 拒绝候选根及任一祖先组件的符号链接或 reparse point；缺失祖先视为无链接而非错误。
pub(super) fn reject_symlink_components(path: &Path) -> std::io::Result<bool> {
    match walk_ancestors_for_link_component(path) {
        Ok(has_link) => Ok(has_link),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// 从根到叶自顶向下遍历路径祖先，遇到首个链接类节点即短路返回。
///
/// 与 [`reject_symlink_components`] 共享同一遍历实现：发现阶段把缺失祖先当作
/// “尚未产生链接”（宽松容错，避免误杀正在创建中的候选目录），而
/// [`file_source::inspect_rollout_file`] 把任何遍历错误（含缺失祖先）都当作
/// Indeterminate（严格失败，因为文件已经打开，缺失祖先意味着发生了竞态)。
pub(crate) fn walk_ancestors_for_link_component(path: &Path) -> std::io::Result<bool> {
    let mut ancestors = path
        .ancestors()
        .filter(|ancestor| !ancestor.as_os_str().is_empty())
        .collect::<Vec<_>>();
    ancestors.reverse();
    for component_path in ancestors {
        let metadata = fs::symlink_metadata(component_path)?;
        if metadata_is_link_like(&metadata) && !is_platform_root_alias(component_path) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// macOS 保留少数根级兼容链接；它们不是用户控制的候选目录边界。
fn is_platform_root_alias(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        matches!(path.to_str(), Some("/etc" | "/tmp" | "/var"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        false
    }
}

/// 把 Unix 符号链接与 Windows reparse point 统一视为不可穿越边界。
pub(crate) fn metadata_is_link_like(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::MetadataExt;

        /// Windows 文件属性位，标识该项是 reparse point（含符号链接）。
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// 使用末段生成默认别名，不把完整绝对路径推向 GUI。
pub(super) fn safe_path_alias(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Codex 数据根")
        .to_owned()
}
