//! 在 GUI backend 内验证 rollout 来源边界并生成增量替换检测所需文件摘要。

use std::fs::{self, File, Metadata};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::discovery::walk_ancestors_for_link_component;
use super::jsonl::{ROLLOUT_SIGNATURE_MAX_BYTES, is_rollout_signature_line};
use super::{CancellationToken, DiscoveredRoot, LocalError, LocalErrorKind};

/// 保存已经通过首记录结构验证的同一文件句柄，避免验证后重新打开其他内容。
pub(crate) struct ValidatedRolloutFile {
    /// 已验证文件的原始访问路径。
    path: PathBuf,
    /// 已打开、已通过首记录验证的文件句柄。
    file: File,
    /// 打开时读取到的文件元数据。
    metadata: Metadata,
}

impl ValidatedRolloutFile {
    /// 返回仅供 adapter 做根内相对路径校验的原始访问路径。
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// 返回从已打开句柄读取的元数据，避免依赖验证后的路径状态。
    pub(crate) fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// 返回已验证句柄的可变借用，供摘要计算回到文件开头。
    pub(crate) fn file_mut(&mut self) -> &mut File {
        &mut self.file
    }

    /// 把已验证句柄交给流式解析器继续读取。
    pub(crate) fn into_file(self) -> File {
        self.file
    }
}

/// 保存一次发现或扫描任务跨文件共享的首记录文件数与字节预算。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RolloutProbeBudget {
    /// 剩余允许打开的文件数预算。
    remaining_files: u64,
    /// 剩余允许读取的字节数预算。
    remaining_bytes: u64,
}

impl RolloutProbeBudget {
    /// 从调用方批准的硬上限建立任务级余额。
    pub(crate) const fn new(remaining_files: u64, remaining_bytes: u64) -> Self {
        Self {
            remaining_files,
            remaining_bytes,
        }
    }
}

/// 区分单个同名文件的确认有效、确认拒绝与不能安全下结论的状态。
// 五态设计的关键在于区分“确认不是”（Rejected）和“无法确认”
// （Indeterminate/Cancelled/BudgetExhausted）：前者是发现阶段可以放心
// 丢弃的候选，后者必须保留旧记录、不能当作否定结论处理——
// 例如权限突然被拒绝、文件在探测过程中被移动，都不代表这个文件“确实
// 不是”rollout 来源，贸然按 Rejected 处理会错误丢弃用户已有的数据源。
pub(crate) enum RolloutFileInspection {
    /// 首条完整记录具有 Codex rollout 会话签名，并携带同一已打开句柄。
    Matched(ValidatedRolloutFile),
    /// 文件可完整探测但首记录不符合最小会话签名。
    Rejected,
    /// 打开、元数据或读取失败，调用方必须保留旧 checkpoint。
    Indeterminate,
    /// 用户在探测边界请求取消。
    Cancelled,
    /// 调用方提供的共享字节预算不足以完成首记录判断。
    BudgetExhausted,
}

/// 验证来源严格位于允许的会话区域且文件名符合 rollout JSONL 约定。
pub(crate) fn validate_rollout_path(
    root: &DiscoveredRoot,
    file_path: &Path,
    archived: bool,
) -> Result<(), LocalError> {
    let relative = file_path.strip_prefix(&root.path).map_err(|_| {
        LocalError::new(
            LocalErrorKind::InvalidPath,
            "rollout source escaped its root",
        )
    })?;
    let mut components = relative.components();
    let expected_area = if archived {
        "archived_sessions"
    } else {
        "sessions"
    };
    if components.next().and_then(|item| item.as_os_str().to_str()) != Some(expected_area) {
        return Err(LocalError::new(
            LocalErrorKind::InvalidPath,
            "rollout source is outside the selected session area",
        ));
    }
    if !is_rollout_jsonl(file_path) {
        return Err(LocalError::new(
            LocalErrorKind::InvalidPath,
            "rollout source name is unsupported",
        ));
    }
    Ok(())
}

/// 判断文件名满足唯一获批的 rollout 前缀与 JSONL 扩展名。
pub(crate) fn is_rollout_jsonl(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
}

/// 只读探测一个同名文件的首条完整记录，并在成功时保留同一已验证句柄。
// 这个函数处处在防御 TOCTOU（Time-Of-Check to Time-Of-Use）攻击/竞态：
// 文件路径在“打开”和“读取内容”之间的窗口期，理论上可能被替换成符号
// 链接指向别处（尤其是共享/网络目录）。做法是：
//   1. 先用路径打开一次拿到句柄和元数据；
//   2. 单独检查路径祖先链上是否含链接组件、路径本身现在是否是符号链接；
//   3. 用 same_opened_file 比较“已打开的句柄”与“现在路径指向的文件”
//      是否物理同一份（设备号+inode，或平台无此信息时退回句柄比较）；
//   4. 只有前面全部确认一致，才真正读取内容——全程只信任已经打开的句柄，
//      不会因为路径被中途调包而读到不该读的文件。
pub(crate) fn inspect_rollout_file(
    path: &Path,
    cancellation: &CancellationToken,
    budget: &mut RolloutProbeBudget,
) -> RolloutFileInspection {
    if cancellation.is_cancelled() {
        return RolloutFileInspection::Cancelled;
    }
    if budget.remaining_files == 0 || budget.remaining_bytes == 0 {
        return RolloutFileInspection::BudgetExhausted;
    }
    budget.remaining_files -= 1;
    let Ok(file) = File::open(path) else {
        return RolloutFileInspection::Indeterminate;
    };
    let Ok(metadata) = file.metadata() else {
        return RolloutFileInspection::Indeterminate;
    };
    if !metadata.is_file() {
        return RolloutFileInspection::Rejected;
    }
    let path_has_link = match walk_ancestors_for_link_component(path) {
        Ok(path_has_link) => path_has_link,
        Err(_) => return RolloutFileInspection::Indeterminate,
    };
    let Ok(current_metadata) = fs::symlink_metadata(path) else {
        return RolloutFileInspection::Indeterminate;
    };
    if path_has_link || current_metadata.file_type().is_symlink() || !current_metadata.is_file() {
        return RolloutFileInspection::Indeterminate;
    }
    let same_file = match same_opened_file(&file, path, &metadata, &current_metadata) {
        Ok(same_file) => same_file,
        Err(_) => return RolloutFileInspection::Indeterminate,
    };
    if !same_file {
        return RolloutFileInspection::Indeterminate;
    }
    if cancellation.is_cancelled() {
        return RolloutFileInspection::Cancelled;
    }
    // Read one byte past the cap so `bytes_read == maximum_read` below can distinguish
    // "line is exactly at the cap" from "line exceeds it" instead of guessing.
    let maximum_read = ROLLOUT_SIGNATURE_MAX_BYTES
        .saturating_add(1)
        .min(budget.remaining_bytes);
    let mut reader = BufReader::new(file.take(maximum_read));
    let mut line = Vec::new();
    let read_result = reader.read_until(b'\n', &mut line);
    let limited_file = reader.into_inner();
    let consumed_bytes = maximum_read.saturating_sub(limited_file.limit());
    budget.remaining_bytes = budget.remaining_bytes.saturating_sub(consumed_bytes);
    let file = limited_file.into_inner();
    let Ok(bytes_read) = read_result else {
        return RolloutFileInspection::Indeterminate;
    };
    if cancellation.is_cancelled() {
        return RolloutFileInspection::Cancelled;
    }
    let bytes_read = u64::try_from(bytes_read).unwrap_or(u64::MAX);
    if bytes_read > 0
        && line.ends_with(b"\n")
        && bytes_read <= ROLLOUT_SIGNATURE_MAX_BYTES
        && is_rollout_signature_line(&line)
    {
        RolloutFileInspection::Matched(ValidatedRolloutFile {
            path: path.to_path_buf(),
            file,
            metadata,
        })
    } else if !line.ends_with(b"\n") {
        if bytes_read == maximum_read {
            if maximum_read < ROLLOUT_SIGNATURE_MAX_BYTES.saturating_add(1) {
                RolloutFileInspection::BudgetExhausted
            } else {
                RolloutFileInspection::Rejected
            }
        } else {
            RolloutFileInspection::Indeterminate
        }
    } else {
        RolloutFileInspection::Rejected
    }
}

/// 比较已打开句柄与打开后路径句柄，避免把路径替换后的其他文件交给解析器。
pub(crate) fn same_opened_file(
    opened_file: &File,
    current_path: &Path,
    opened_metadata: &Metadata,
    current_metadata: &Metadata,
) -> std::io::Result<bool> {
    if let Some(same_identity) = identity_from_metadata(opened_metadata, current_metadata) {
        return Ok(same_identity && opened_metadata.len() == current_metadata.len());
    }
    // 平台元数据未提供设备/文件标识（如部分网络文件系统），退回到额外的句柄比较。
    let opened_handle = same_file::Handle::from_file(opened_file.try_clone()?)?;
    let current_handle = same_file::Handle::from_path(current_path)?;
    Ok(opened_handle == current_handle && opened_metadata.len() == current_metadata.len())
}

/// 直接从已经取得的元数据推导设备与文件标识，避免额外的 stat/fstat 系统调用。
#[cfg(unix)]
fn identity_from_metadata(opened: &Metadata, current: &Metadata) -> Option<bool> {
    use std::os::unix::fs::MetadataExt;

    Some(opened.dev() == current.dev() && opened.ino() == current.ino())
}

/// Windows 的卷/文件索引仅能通过句柄查询获取，标准库 `Metadata` 未携带该信息且
/// 对应 API 仍处于 unstable（`windows_by_handle`），因此退回到句柄比较。
#[cfg(windows)]
fn identity_from_metadata(_opened: &Metadata, _current: &Metadata) -> Option<bool> {
    None
}

/// 其他非 Unix 平台同样退回到受审句柄比较实现复查。
#[cfg(not(any(unix, windows)))]
fn identity_from_metadata(_opened: &Metadata, _current: &Metadata) -> Option<bool> {
    None
}

/// 对已验证句柄开头固定窗口求稳定摘要，用于区分正常追加与替换。
// 只对文件“开头 64 字节”做 FNV-1a 哈希：session 文件一旦创建，
// 开头内容（首条 session_meta 记录）在正常追加写入场景下永远不变；
// 如果下次扫描发现同一路径的“开头指纹”变了，就说明文件被整体替换/
// 截断重写过（而不是简单追加了新行），索引层据此判断是否需要重建
// 该来源的全部 checkpoint，而不是继续假设可以从旧偏移量继续增量读取。
pub(crate) fn file_identity(file: &mut File) -> Result<String, LocalError> {
    file.seek(SeekFrom::Start(0))?;
    let mut prefix = [0_u8; 64];
    let read = file.read(&mut prefix)?;
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in &prefix[..read] {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    Ok(format!("file-{read}-{hash:016x}"))
}

/// 把文件修改时间转换为 Unix 毫秒，无法表达时使用零作为未知值。
pub(crate) fn modified_epoch_ms(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    /// 验证已打开文件与同一路径仍指向同一文件时通过句柄身份和长度复查。
    #[test]
    fn same_opened_file_accepts_unchanged_path() {
        let temp = tempdir().expect("temporary source root is available");
        let path = temp.path().join("rollout-same.jsonl");
        fs::write(&path, b"same file").expect("source file is written");
        let opened = File::open(&path).expect("source file is opened");
        let opened_metadata = opened.metadata().expect("opened metadata is available");
        let current_metadata = fs::metadata(&path).expect("path metadata is available");

        assert!(
            same_opened_file(&opened, &path, &opened_metadata, &current_metadata)
                .expect("identity comparison succeeds")
        );
    }

    /// 验证路径改指向另一个同长度文件时仍被句柄身份复查拒绝。
    #[test]
    fn same_opened_file_rejects_different_same_length_file() {
        let temp = tempdir().expect("temporary source root is available");
        let opened_path = temp.path().join("rollout-opened.jsonl");
        let current_path = temp.path().join("rollout-current.jsonl");
        fs::write(&opened_path, b"first file").expect("opened source is written");
        fs::write(&current_path, b"other file").expect("current source is written");
        let opened = File::open(&opened_path).expect("source file is opened");
        let opened_metadata = opened.metadata().expect("opened metadata is available");
        let current_metadata = fs::metadata(&current_path).expect("path metadata is available");

        assert!(
            !same_opened_file(&opened, &current_path, &opened_metadata, &current_metadata,)
                .expect("identity comparison succeeds")
        );
    }

    /// 验证短首行后的 BufReader 预读也会扣减共享字节预算，而不只计算返回行长度。
    #[test]
    fn rollout_probe_budget_charges_actual_buffered_read() {
        let temp = tempdir().expect("temporary source root is available");
        let first_path = temp.path().join("rollout-first.jsonl");
        let first_line = concat!(
            "{\"timestamp\":\"2026-07-31T00:00:00Z\",\"type\":\"session_meta\",",
            "\"payload\":{\"id\":\"session-first\"}}\n"
        );
        fs::write(&first_path, format!("{first_line}{}", "x".repeat(20_000)))
            .expect("first rollout is written");
        let initial_bytes = 10_000_u64;
        let mut budget = RolloutProbeBudget::new(2, initial_bytes);

        assert!(matches!(
            inspect_rollout_file(&first_path, &CancellationToken::new(), &mut budget),
            RolloutFileInspection::Matched(_)
        ));
        assert!(
            budget.remaining_bytes
                < initial_bytes
                    .saturating_sub(u64::try_from(first_line.len()).expect("line length fits"))
        );

        let second_path = temp.path().join("rollout-second.jsonl");
        let padding = "x".repeat(
            usize::try_from(budget.remaining_bytes)
                .expect("test budget fits memory")
                .saturating_add(100),
        );
        fs::write(
            &second_path,
            format!(
                "{{\"timestamp\":\"2026-07-31T00:00:00Z\",\"type\":\"session_meta\",\
                 \"payload\":{{\"id\":\"session-second\",\"padding\":\"{padding}\"}}}}\n"
            ),
        )
        .expect("second rollout is written");

        assert!(matches!(
            inspect_rollout_file(&second_path, &CancellationToken::new(), &mut budget),
            RolloutFileInspection::BudgetExhausted
        ));
    }
}
