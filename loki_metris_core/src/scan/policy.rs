//! 与扫描范围和平台边界相关的共享策略。

use std::path::{Path, PathBuf};

/// Windows 每个本地卷根下应直接跳过的系统/厂商目录名。
#[cfg(target_os = "windows")]
const WINDOWS_VOLUME_SKIP_DIRS: &[&str] = &[
    "Windows",
    "Windows.old",
    "Program Files",
    "Program Files (x86)",
    "ProgramData",
    "PerfLogs",
    "Recovery",
    "System Volume Information",
    "$Recycle.Bin",
    "$WinREAgent",
    "Config.Msi",
    "MSOCache",
    "Documents and Settings",
    "Boot",
    "EFI",
    "Intel",
    "AMD",
    "NVIDIA",
    "NVIDIA Corporation",
];

/// 任意深度遇到即跳过的目录名（不含用户 Library / Applications 这类可能含数据的名称）。
fn full_discovery_skip_directory_names() -> &'static [&'static str] {
    #[cfg(target_os = "windows")]
    {
        WINDOWS_VOLUME_SKIP_DIRS
    }
    #[cfg(target_os = "macos")]
    {
        &[
            ".Spotlight-V100",
            ".DocumentRevisions-V100",
            ".fseventsd",
            ".TemporaryItems",
            ".Trashes",
            ".Trash",
            ".vol",
            "lost+found",
            "cores",
        ]
    }
    #[cfg(target_os = "linux")]
    {
        &[
            "proc",
            "sys",
            "dev",
            "run",
            "lost+found",
            ".Trash",
            ".Trashes",
        ]
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        &["lost+found", ".Trash", ".Trashes"]
    }
}

/// macOS 根卷上几乎不可能存放会话数据的固定绝对路径。
#[cfg(target_os = "macos")]
fn macos_absolute_skip_roots() -> &'static [&'static str] {
    &[
        "/System",
        "/Library",
        "/Applications",
        "/usr",
        "/bin",
        "/sbin",
        "/private",
        "/dev",
        "/cores",
        "/opt",
        "/Network",
        "/Volumes",
        "/.Spotlight-V100",
        "/.DocumentRevisions-V100",
        "/.fseventsd",
        "/.TemporaryItems",
        "/.vol",
        "/lost+found",
    ]
}

/// 目录名是否属于全盘发现必须跳过的系统/垃圾目录。
pub fn is_full_discovery_skip_directory_name(name: &str) -> bool {
    full_discovery_skip_directory_names()
        .iter()
        .any(|skip| name.eq_ignore_ascii_case(skip))
}

/// 路径末段是否命中全盘跳过目录名。
pub fn path_has_full_discovery_skip_directory_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_full_discovery_skip_directory_name)
}

/// 判断主动发现路径是否位于排除范围；允许单独排队的本地卷穿过其挂载容器。
pub fn is_discovery_path_excluded(
    path: &Path,
    traversal_root: &Path,
    excluded_roots: &[PathBuf],
) -> bool {
    if path_has_full_discovery_skip_directory_name(path) {
        return true;
    }
    excluded_roots
        .iter()
        .any(|excluded| path.starts_with(excluded) && !traversal_root.starts_with(excluded))
}

/// 在全设备发现前先应用的平台保守排除策略（不含按卷展开的相对目录）。
pub fn append_platform_discovery_excludes(excluded_roots: Vec<PathBuf>) -> Vec<PathBuf> {
    append_platform_discovery_excludes_for_volumes(excluded_roots, &[])
}

/// 在平台固定排除之外，按每个本地卷根展开“不可能存放会话数据”的相对目录。
pub fn append_platform_discovery_excludes_for_volumes(
    mut excluded_roots: Vec<PathBuf>,
    volume_roots: &[PathBuf],
) -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        excluded_roots.extend(
            macos_absolute_skip_roots()
                .iter()
                .map(|path| PathBuf::from(*path)),
        );
        // 外置卷根上也跳过同名系统元数据目录。
        for root in volume_roots {
            for name in full_discovery_skip_directory_names() {
                excluded_roots.push(root.join(name));
            }
            for name in [
                "System",
                "Library",
                "Applications",
                "usr",
                "bin",
                "sbin",
                "private",
                "opt",
            ] {
                excluded_roots.push(root.join(name));
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        for root in volume_roots {
            for name in WINDOWS_VOLUME_SKIP_DIRS {
                excluded_roots.push(root.join(name));
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        excluded_roots.extend([
            PathBuf::from("/proc"),
            PathBuf::from("/sys"),
            PathBuf::from("/dev"),
            PathBuf::from("/run"),
            PathBuf::from("/net"),
            PathBuf::from("/lost+found"),
        ]);
        for root in volume_roots {
            for name in full_discovery_skip_directory_names() {
                excluded_roots.push(root.join(name));
            }
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = volume_roots;
        excluded_roots.push(PathBuf::from("/net"));
    }

    excluded_roots.sort();
    excluded_roots.dedup();
    excluded_roots
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "windows")]
    use super::append_platform_discovery_excludes_for_volumes;
    #[cfg(target_os = "macos")]
    use super::is_discovery_path_excluded;
    use super::{
        append_platform_discovery_excludes, full_discovery_skip_directory_names,
        is_full_discovery_skip_directory_name,
    };
    #[cfg(target_os = "macos")]
    use std::path::Path;
    use std::path::PathBuf;

    #[cfg(target_os = "macos")]
    #[test]
    /// 验证 macOS 完全扫描包含固定系统绝对排除目录。
    fn macos_includes_system_absolute_excludes() {
        let excluded = append_platform_discovery_excludes(vec![PathBuf::from("/tmp")]);

        assert!(excluded.contains(&PathBuf::from("/tmp")));
        assert!(excluded.contains(&PathBuf::from("/Network")));
        assert!(excluded.contains(&PathBuf::from("/Volumes")));
        assert!(excluded.contains(&PathBuf::from("/System")));
        assert!(excluded.contains(&PathBuf::from("/Library")));
        assert!(excluded.contains(&PathBuf::from("/Applications")));
        assert!(excluded.contains(&PathBuf::from("/private")));
        let unique_count = excluded
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        assert_eq!(unique_count, excluded.len());
    }

    #[cfg(target_os = "macos")]
    #[test]
    /// 验证系统 Library 排除不会误伤用户 Library 路径。
    fn macos_user_library_is_not_prefix_excluded_by_system_library() {
        let excluded = append_platform_discovery_excludes(Vec::new());
        let user_library = PathBuf::from("/Users/demo/Library");
        assert!(!is_discovery_path_excluded(
            &user_library,
            Path::new("/"),
            &excluded
        ));
        assert!(is_discovery_path_excluded(
            Path::new("/Library"),
            Path::new("/"),
            &excluded
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    /// 验证 Linux 扫描包含已知伪文件系统与临时目录排除。
    fn linux_includes_known_excludes() {
        let excluded = append_platform_discovery_excludes(vec![PathBuf::from("/tmp")]);

        assert!(excluded.contains(&PathBuf::from("/tmp")));
        assert!(excluded.contains(&PathBuf::from("/proc")));
        assert!(excluded.contains(&PathBuf::from("/sys")));
        let unique_count = excluded
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        assert_eq!(unique_count, excluded.len());
    }

    #[cfg(target_os = "windows")]
    #[test]
    /// 验证 Windows 扫描为每个卷展开固定跳过目录。
    fn windows_expands_volume_skip_dirs() {
        let excluded = append_platform_discovery_excludes_for_volumes(
            vec![PathBuf::from(r"Z:\network")],
            &[PathBuf::from(r"C:\")],
        );

        assert!(excluded.contains(&PathBuf::from(r"Z:\network")));
        assert!(excluded.contains(&PathBuf::from(r"C:\Windows")));
        assert!(excluded.contains(&PathBuf::from(r"C:\Program Files")));
        assert!(excluded.contains(&PathBuf::from(r"C:\Program Files (x86)")));
        assert!(excluded.contains(&PathBuf::from(r"C:\$Recycle.Bin")));
        assert!(is_full_discovery_skip_directory_name("Program Files"));
        assert!(is_full_discovery_skip_directory_name("windows"));
        assert!(!is_full_discovery_skip_directory_name("Users"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    /// 验证没有枚举到卷时保留调用者明确提供的扫描输入。
    fn windows_without_volumes_keeps_input_only() {
        let excluded = append_platform_discovery_excludes(vec![PathBuf::from(r"C:")]);
        assert_eq!(excluded, vec![PathBuf::from(r"C:")]);
    }

    #[test]
    /// 验证跨平台跳过目录名称按不区分大小写匹配。
    fn skip_directory_names_are_case_insensitive() {
        let platform_name = full_discovery_skip_directory_names()
            .first()
            .expect("every supported platform has a skip-directory policy");
        assert!(is_full_discovery_skip_directory_name(
            &platform_name.to_ascii_uppercase()
        ));
    }
}
