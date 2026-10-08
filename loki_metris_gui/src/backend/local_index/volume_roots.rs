//! 以只读平台卷信息为主动发现提供本地搜索起点，并保守排除网络或未知卷。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// 汇总可安全遍历的本地卷与必须跳过的挂载点，不包含目录内容。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalVolumeRoots {
    /// 当前平台确认可在本次主动任务中遍历的本地卷根。
    pub search_roots: Vec<PathBuf>,
    /// 网络卷或无法可靠分类卷的挂载点，主遍历不得跨入。
    pub excluded_roots: Vec<PathBuf>,
    /// 因网络卷策略跳过的卷数量。
    pub network_skipped_count: u64,
    /// 因未知、离线或枚举失败跳过的卷数量。
    pub other_skipped_count: u64,
}

/// 表示卷是否满足“当前用户可读且可确认的本地卷”边界。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VolumeClass {
    /// 平台或文件系统类型明确证明是本地卷。
    Local,
    /// 文件系统类型明确表示网络或远程卷。
    Network,
    /// 无法可靠证明本地性，按产品规则跳过。
    Unknown,
}

/// 区分三种目标平台的卷元数据证明能力，供纯函数测试复用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VolumePlatform {
    /// Apple 的枚举结果已由系统 `NSURLVolumeIsLocalKey` 确认为本地。
    Apple,
    /// Windows 的枚举结果已由系统限制为 fixed/removable volume。
    Windows,
    /// Linux 需要再按 mountinfo 文件系统类型做保守分类。
    Linux,
    /// 其他平台没有当前获批的可靠分类实现。
    Other,
}

/// 表示一个精确候选路径是否已被当前平台证明位于本地卷。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalPathStatus {
    /// 路径存在，且文件系统类型明确属于本地持久卷。
    ConfirmedLocal,
    /// 路径位于明确的网络文件系统或网络路径前缀。
    RejectedNetwork,
    /// 路径不存在，可安全按失效候选处理。
    Missing,
    /// 权限、卷类型或平台能力不足，不能读取也不能删除历史索引。
    Indeterminate,
}

// 本文件的安全基调是“宁可保守跳过，也不误扫网络卷”：`whichdisk` 是一个
// 跨平台查询“某路径/挂载点属于哪种文件系统”的库；只要分类结果不能
// 100% 确定是本地卷（包括查询失败、平台不支持判定等任何不确定情况），
// 一律归为 Network 或 Unknown 并排除在扫描范围之外，而不是"默认当作本地"。
/// 在读取候选目录或文件内容前确认其物理卷类型；未知类型按不获准处理。
pub(crate) fn classify_local_path(path: &Path) -> LocalPathStatus {
    if is_obviously_network_path(path) {
        return LocalPathStatus::RejectedNetwork;
    }
    match whichdisk::resolve(path) {
        Ok(location) => match classify_resolved_volume(current_platform(), location.fs_type()) {
            VolumeClass::Local => LocalPathStatus::ConfirmedLocal,
            VolumeClass::Network => LocalPathStatus::RejectedNetwork,
            VolumeClass::Unknown => LocalPathStatus::Indeterminate,
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => LocalPathStatus::Missing,
        Err(_) => LocalPathStatus::Indeterminate,
    }
}

/// 先用纯路径规则拒绝不会触发文件系统访问的 UNC 与常见网络挂载入口。
pub(crate) fn is_obviously_network_path(path: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::path::{Component, Prefix};

        matches!(
            path.components().next(),
            Some(Component::Prefix(prefix))
                if matches!(prefix.kind(), Prefix::UNC(_, _) | Prefix::VerbatimUNC(_, _))
        )
    }
    #[cfg(not(target_os = "windows"))]
    {
        let text = path.to_string_lossy().replace('\\', "/");
        text.starts_with("//")
            || text == "/Network"
            || text.starts_with("/Network/")
            || text == "/net"
            || text.starts_with("/net/")
    }
}

/// 枚举当前平台卷；失败时返回空搜索范围并降低覆盖，不退回无边界根扫描。
pub fn enumerate_local_volume_roots() -> LocalVolumeRoots {
    let mounts = match whichdisk::list() {
        Ok(mounts) => mounts,
        Err(_) => {
            return LocalVolumeRoots {
                other_skipped_count: 1,
                ..LocalVolumeRoots::default()
            };
        }
    };
    let platform = current_platform();
    let mut local_candidates = BTreeSet::new();
    let mut excluded_roots = BTreeSet::new();
    let mut network_skipped_count = 0_u64;
    let mut other_skipped_count = 0_u64;

    for mount in mounts {
        let mount_point = mount.mount_point().to_path_buf();
        match classify_volume(platform, mount.fs_type()) {
            VolumeClass::Local => {
                local_candidates.insert(mount_point);
            }
            VolumeClass::Network => {
                network_skipped_count = network_skipped_count.saturating_add(1);
                excluded_roots.insert(mount_point);
            }
            VolumeClass::Unknown => {
                other_skipped_count = other_skipped_count.saturating_add(1);
                excluded_roots.insert(mount_point);
            }
        }
    }

    let exact_conflicts = local_candidates
        .intersection(&excluded_roots)
        .cloned()
        .collect::<Vec<_>>();
    for conflict in &exact_conflicts {
        local_candidates.remove(conflict);
    }
    other_skipped_count = other_skipped_count
        .saturating_add(u64::try_from(exact_conflicts.len()).unwrap_or(u64::MAX));

    let mut search_roots = BTreeSet::new();
    for mount_point in local_candidates {
        let normalized = normalize_local_mount_point(mount_point.clone());
        if normalized.is_dir() {
            search_roots.insert(normalized);
        } else {
            other_skipped_count = other_skipped_count.saturating_add(1);
            excluded_roots.insert(mount_point);
        }
    }

    finish_volume_roots(
        search_roots,
        excluded_roots,
        network_skipped_count,
        other_skipped_count,
    )
}

/// 生成最终卷边界；成功但没有任何可确认本地卷时必须降低覆盖而非报告完整。
fn finish_volume_roots(
    mut search_roots: BTreeSet<PathBuf>,
    excluded_roots: BTreeSet<PathBuf>,
    network_skipped_count: u64,
    mut other_skipped_count: u64,
) -> LocalVolumeRoots {
    let conflicts = search_roots
        .intersection(&excluded_roots)
        .cloned()
        .collect::<Vec<_>>();
    for conflict in &conflicts {
        search_roots.remove(conflict);
    }
    other_skipped_count =
        other_skipped_count.saturating_add(u64::try_from(conflicts.len()).unwrap_or(u64::MAX));
    if search_roots.is_empty() && network_skipped_count == 0 && other_skipped_count == 0 {
        other_skipped_count = 1;
    }
    LocalVolumeRoots {
        search_roots: search_roots.into_iter().collect(),
        excluded_roots: excluded_roots.into_iter().collect(),
        network_skipped_count,
        other_skipped_count,
    }
}

/// 仅对已确认本地的挂载位置做物理规范化；网络或未知卷不得触发文件系统访问。
fn normalize_local_mount_point(mount_point: PathBuf) -> PathBuf {
    fs::canonicalize(&mount_point).unwrap_or(mount_point)
}

/// 返回编译目标对应的卷分类策略，不读取环境或文件。
fn current_platform() -> VolumePlatform {
    match std::env::consts::OS {
        "macos" => VolumePlatform::Apple,
        "windows" => VolumePlatform::Windows,
        "linux" => VolumePlatform::Linux,
        _ => VolumePlatform::Other,
    }
}

/// 根据平台已提供的证明与文件系统类型判断是否允许遍历。
fn classify_volume(platform: VolumePlatform, fs_type: &str) -> VolumeClass {
    match platform {
        VolumePlatform::Apple | VolumePlatform::Windows => VolumeClass::Local,
        VolumePlatform::Linux => classify_linux_file_system(fs_type),
        VolumePlatform::Other => VolumeClass::Unknown,
    }
}

/// 对精确路径解析出的真实文件系统类型做保守分类，不依赖卷枚举的预过滤。
fn classify_resolved_volume(platform: VolumePlatform, fs_type: &str) -> VolumeClass {
    let normalized = fs_type.to_ascii_lowercase();
    if is_network_file_system(&normalized) {
        return VolumeClass::Network;
    }
    match platform {
        VolumePlatform::Apple => {
            if matches!(
                normalized.as_str(),
                "apfs" | "hfs" | "hfsplus" | "msdos" | "exfat" | "ntfs"
            ) {
                VolumeClass::Local
            } else {
                VolumeClass::Unknown
            }
        }
        VolumePlatform::Windows => {
            if matches!(
                normalized.as_str(),
                "ntfs" | "refs" | "fat" | "fat12" | "fat16" | "fat32" | "exfat"
            ) {
                VolumeClass::Local
            } else {
                VolumeClass::Unknown
            }
        }
        VolumePlatform::Linux => classify_linux_file_system(&normalized),
        VolumePlatform::Other => VolumeClass::Unknown,
    }
}

/// 识别三平台常见远程文件系统；未知 FUSE 类型继续按 Unknown 处理。
fn is_network_file_system(fs_type: &str) -> bool {
    matches!(
        fs_type,
        "nfs"
            | "nfs4"
            | "cifs"
            | "smb"
            | "smb2"
            | "smb3"
            | "smbfs"
            | "sshfs"
            | "fuse.sshfs"
            | "davfs"
            | "webdav"
            | "fuse.davfs"
            | "afpfs"
            | "ceph"
            | "glusterfs"
            | "9p"
            | "afs"
    )
}

/// Linux 只接收明确的本地持久文件系统；未知 FUSE/虚拟类型按无法分类跳过。
fn classify_linux_file_system(fs_type: &str) -> VolumeClass {
    let normalized = fs_type.to_ascii_lowercase();
    if is_network_file_system(&normalized) {
        return VolumeClass::Network;
    }
    if matches!(
        normalized.as_str(),
        "ext2"
            | "ext3"
            | "ext4"
            | "xfs"
            | "btrfs"
            | "f2fs"
            | "zfs"
            | "bcachefs"
            | "jfs"
            | "reiserfs"
            | "vfat"
            | "exfat"
            | "ntfs"
            | "ntfs3"
            | "hfs"
            | "hfsplus"
            | "apfs"
    ) {
        VolumeClass::Local
    } else {
        VolumeClass::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证 Linux 网络类型绝不会成为主动发现起点。
    #[test]
    fn linux_network_file_systems_are_rejected() {
        for fs_type in ["nfs", "nfs4", "cifs", "fuse.sshfs", "9p"] {
            assert_eq!(
                classify_volume(VolumePlatform::Linux, fs_type),
                VolumeClass::Network
            );
        }
    }

    /// 验证 Linux 只接受明确的本地持久文件系统，未知类型保守跳过。
    #[test]
    fn linux_local_and_unknown_file_systems_are_distinguished() {
        for fs_type in ["ext4", "btrfs", "xfs", "exfat", "ntfs3"] {
            assert_eq!(
                classify_volume(VolumePlatform::Linux, fs_type),
                VolumeClass::Local
            );
        }
        for fs_type in ["", "overlay", "fuse.custom", "mysteryfs"] {
            assert_eq!(
                classify_volume(VolumePlatform::Linux, fs_type),
                VolumeClass::Unknown
            );
        }
    }

    /// 验证 Apple 与 Windows 只消费依赖已经通过系统属性筛出的卷。
    #[test]
    fn platform_filtered_apple_and_windows_mounts_are_local() {
        assert_eq!(
            classify_volume(VolumePlatform::Apple, "apfs"),
            VolumeClass::Local
        );
        assert_eq!(
            classify_volume(VolumePlatform::Windows, "NTFS"),
            VolumeClass::Local
        );
        assert_eq!(
            classify_volume(VolumePlatform::Other, "ext4"),
            VolumeClass::Unknown
        );
    }

    /// 验证精确路径解析不会把 Apple/Windows 网络类型误当成枚举预过滤后的本地卷。
    #[test]
    fn resolved_volume_classification_rejects_network_and_unknown_types() {
        for platform in [
            VolumePlatform::Apple,
            VolumePlatform::Windows,
            VolumePlatform::Linux,
        ] {
            assert_eq!(
                classify_resolved_volume(platform, "smbfs"),
                VolumeClass::Network
            );
            assert_eq!(
                classify_resolved_volume(platform, "fuse.custom"),
                VolumeClass::Unknown
            );
        }
        assert_eq!(
            classify_resolved_volume(VolumePlatform::Apple, "apfs"),
            VolumeClass::Local
        );
        assert_eq!(
            classify_resolved_volume(VolumePlatform::Windows, "NTFS"),
            VolumeClass::Local
        );
    }

    /// 验证纯路径预检在访问文件系统前拒绝常见网络挂载入口。
    #[test]
    fn obvious_network_prefixes_are_rejected_without_io() {
        #[cfg(target_os = "windows")]
        assert!(is_obviously_network_path(Path::new(
            r"\\server\share\client-data"
        )));
        #[cfg(not(target_os = "windows"))]
        {
            assert!(is_obviously_network_path(Path::new("/Network/client-data")));
            assert!(is_obviously_network_path(Path::new("/net/client-data")));
            assert!(!is_obviously_network_path(Path::new("/Users/client-data")));
        }
    }

    /// 验证卷 API 成功但返回空集合时降低覆盖，不能把“未搜索”报告为完整。
    #[test]
    fn empty_volume_enumeration_is_not_complete() {
        let roots = finish_volume_roots(BTreeSet::new(), BTreeSet::new(), 0, 0);

        assert!(roots.search_roots.is_empty());
        assert_eq!(roots.other_skipped_count, 1);
    }

    /// 验证重复挂载点只遍历一次，枚举阶段的网络与不可读/未知计数仍完整保留。
    #[test]
    fn duplicate_mounts_are_deduplicated_without_losing_skip_counts() {
        let search_roots = [
            PathBuf::from("/"),
            PathBuf::from("/"),
            PathBuf::from("/Volumes/local"),
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        let excluded_roots = [
            PathBuf::from("/Volumes/network"),
            PathBuf::from("/Volumes/network"),
            PathBuf::from("/Volumes/unreadable"),
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();

        let roots = finish_volume_roots(search_roots, excluded_roots, 1, 1);

        assert_eq!(roots.search_roots.len(), 2);
        assert_eq!(roots.excluded_roots.len(), 2);
        assert_eq!(roots.network_skipped_count, 1);
        assert_eq!(roots.other_skipped_count, 1);
    }

    /// 验证同一规范化挂载点出现本地/排除冲突时失败关闭，不会成为扫描起点。
    #[test]
    fn conflicting_mount_classification_is_excluded() {
        let conflicted = PathBuf::from("/Volumes/conflicted");
        let roots = finish_volume_roots(
            [PathBuf::from("/"), conflicted.clone()]
                .into_iter()
                .collect(),
            [conflicted.clone()].into_iter().collect(),
            0,
            0,
        );

        assert_eq!(roots.search_roots, vec![PathBuf::from("/")]);
        assert_eq!(roots.excluded_roots, vec![conflicted]);
        assert_eq!(roots.other_skipped_count, 1);
    }
}
