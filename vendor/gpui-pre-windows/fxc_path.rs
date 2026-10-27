//! 按 PATH 顺序定位单个 Windows SDK 编译器，保留原生路径编码。

use std::{ffi::OsStr, path::PathBuf};

/// 直接逐项检查 PATH，不把 where.exe 的多行输出误当作一个路径，也不丢失非 ASCII 路径。
pub(crate) fn find_fxc_in_path(search_path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(search_path).map(|directory| directory.join("fxc.exe")).find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 测试只拥有唯一临时目录，销毁时回收自己的合成编译器文件。
    struct Fixture(PathBuf);

    impl Fixture {
        /// 构造不修改真实 PATH 和 SDK 安装的隔离目录。
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!("loki-fxc-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }

        /// 在指定候选目录创建普通文件，内容无需包含可执行代码。
        fn compiler(&self, name: &str) -> PathBuf {
            let directory = self.0.join(name);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join("fxc.exe"), b"fixture").unwrap();
            directory
        }
    }

    impl Drop for Fixture {
        /// 只清理当前夹具创建的精确临时目录。
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }

    /// 多个 SDK 均在 PATH 时只选第一个实际文件，路径中可包含中文和空格。
    #[test]
    fn multiple_sdk_paths_select_one_native_path_in_order() {
        let fixture = Fixture::new();
        let first = fixture.compiler("首选 SDK");
        let second = fixture.compiler("second-sdk");
        let path = std::env::join_paths([&first, &second]).unwrap();
        assert_eq!(find_fxc_in_path(&path), Some(first.join("fxc.exe")));
        let reversed = std::env::join_paths([&second, &first]).unwrap();
        assert_eq!(find_fxc_in_path(&reversed), Some(second.join("fxc.exe")));
    }

    /// 缺失候选和同名目录不能遮蔽后面的真实编译器，全部缺失返回空。
    #[test]
    fn missing_compilers_and_directories_are_skipped() {
        let fixture = Fixture::new();
        let missing = fixture.0.join("missing");
        let directory = fixture.0.join("directory");
        std::fs::create_dir_all(directory.join("fxc.exe")).unwrap();
        let present = fixture.compiler("present");
        assert!(find_fxc_in_path(&std::env::join_paths([&missing, &directory]).unwrap()).is_none());
        assert_eq!(find_fxc_in_path(&std::env::join_paths([&missing, &directory, &present]).unwrap()), Some(present.join("fxc.exe")));
    }
}
