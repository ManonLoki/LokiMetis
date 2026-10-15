#[path = "../gpui-patch-support/generator.rs"]
mod generator;

#[path = "../gpui-patch-support/sha256.rs"]
mod fixture_sha256;

use generator::{Patch, Spec};
use std::{collections::{BTreeMap, BTreeSet}, fs, path::Path};

const FONT_FIXTURE_MANIFEST_SHA256: &str = "9fd0d42f6f6fcd62a12d6968f37d16d76b04aa048d1dbfe01814181526b8f81b";

/// 字体夹具仅接受真实目录和有限大小的普通文件，拒绝父目录或叶子链接。
fn font_fixture_files(root: &Path, directory: &Path, found: &mut BTreeMap<String, Vec<u8>>) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.is_dir() {
            font_fixture_files(root, &path, found)?;
        } else if metadata.is_file() {
            if metadata.len() > 200_500 { return Err("font fixture exceeds fixed maximum size".into()); }
            let name = path.strip_prefix(root).map_err(|error| error.to_string())?
                .to_str().ok_or("non-UTF8 font fixture path")?.replace('\\', "/");
            found.insert(name, fs::read(&path).map_err(|error| error.to_string())?);
        } else {
            return Err(format!("font fixture links or special files are forbidden: {}", path.display()));
        }
    }
    Ok(())
}

/// 固定清单覆盖且仅覆盖三个官方字体及两份原样许可，不允许额外或变更字节。
fn verified_font_fixtures(manifest: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let list_path = manifest.join("test-fixtures-content.sha256");
    if !fs::symlink_metadata(&list_path).map_err(|error| error.to_string())?.is_file() {
        return Err("font fixture manifest must be a regular file".into());
    }
    let list = fs::read_to_string(list_path).map_err(|error| error.to_string())?;
    if fixture_sha256::hex(list.as_bytes()) != FONT_FIXTURE_MANIFEST_SHA256 {
        return Err("font fixture manifest drift".into());
    }
    let root = manifest.join("test-fixtures");
    if !fs::symlink_metadata(&root).map_err(|error| error.to_string())?.is_dir() {
        return Err("font fixture root must be a real directory".into());
    }
    let mut actual = BTreeMap::new();
    font_fixture_files(&root, &root, &mut actual)?;
    let mut expected = BTreeSet::new();
    for line in list.lines() {
        let (digest, name) = line.split_once("  ").ok_or("malformed font fixture SHA line")?;
        if digest.len() != 64 || !digest.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            || name.is_empty() || name.contains('\\') || name.contains(':')
            || name.split('/').any(|part| part.is_empty() || part == "." || part == "..") {
            return Err("unsafe font fixture manifest entry".into());
        }
        if !expected.insert(name.to_owned()) { return Err(format!("duplicate font fixture: {name}")); }
        let bytes = actual.get(name).ok_or_else(|| format!("missing font fixture: {name}"))?;
        if fixture_sha256::hex(bytes) != digest { return Err(format!("font fixture byte drift: {name}")); }
    }
    if expected.len() != 5 || actual.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err("font fixture file set differs from fixed five-file manifest".into());
    }
    Ok(actual)
}

/// 校验不可变官方原包和原样测试字体，再生成仅修改声明片段的完整 WGPU 编译树。
fn main() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    println!("cargo:rerun-if-changed={}", manifest.join("test-fixtures-content.sha256").display());
    println!("cargo:rerun-if-changed={}", manifest.join("test-fixtures").display());
    verified_font_fixtures(manifest).unwrap_or_else(|error| panic!("WGPU font fixture verification failed: {error}"));
    let spec = Spec {
        entry: "src/gpui_wgpu.rs",
        root_prefix: include_str!("patches/root-prefix.before"),
        source_manifest_sha256: "64f38e80c93d932e660b2b437d6591d74996851cd37e7e0e216b0e0608ef340b",
        patches: &[
            Patch {
                name: "polychrome-layout",
                path: "src/shaders.wgsl",
                before: include_str!("patches/polychrome-layout.before"),
                after: include_str!("patches/polychrome-layout.after"),
            },
            Patch {
                name: "polychrome-distance",
                path: "src/shaders.wgsl",
                before: include_str!("patches/polychrome-distance.before"),
                after: include_str!("patches/polychrome-distance.after"),
            },
            Patch {
                name: "webgl-polychrome-decoder",
                path: "src/shaders_webgl.wgsl",
                before: include_str!("patches/webgl-polychrome-decoder.before"),
                after: include_str!("patches/webgl-polychrome-decoder.after"),
            },
            Patch {
                name: "polychrome-word-stride",
                path: "src/wgpu_renderer.rs",
                before: include_str!("patches/polychrome-word-stride.before"),
                after: include_str!("patches/polychrome-word-stride.after"),
            },
            Patch {
                name: "shader-test-naga-import",
                path: "src/wgpu_renderer.rs",
                before: include_str!("patches/shader-test-naga-import.before"),
                after: include_str!("patches/shader-test-naga-import.after"),
            },
            Patch {
                name: "test-font-regular-family",
                path: "src/cosmic_text_system.rs",
                before: include_str!("patches/test-font-regular-family.before"),
                after: include_str!("patches/test-font-regular-family.after"),
            },
            Patch {
                name: "test-font-bold-family",
                path: "src/cosmic_text_system.rs",
                before: include_str!("patches/test-font-bold-family.before"),
                after: include_str!("patches/test-font-bold-family.after"),
            },
            Patch {
                name: "test-font-ibm-constant",
                path: "src/cosmic_text_system.rs",
                before: include_str!("patches/test-font-ibm-constant.before"),
                after: include_str!("patches/test-font-ibm-constant.after"),
            },
            Patch {
                name: "test-font-lilex-constant",
                path: "src/cosmic_text_system.rs",
                before: include_str!("patches/test-font-lilex-constant.before"),
                after: include_str!("patches/test-font-lilex-constant.after"),
            },
        ],
        generated_hashes: &[
            ("src/cosmic_text_system.rs", "79305228ecd9588a20bae8c4b3680391ba870657da5ac8e6854ce58a8a458535"),
            ("src/gpui_wgpu.rs", "b51033ac07d187c89c4079f208d3a557c6fff309b1cd403927ce8974e795afa0"),
            ("src/shaders.wgsl", "03d4c3ee901b800fe0beb775abfc191820affdec17bc38759129f3254c7c2e48"),
            ("src/shaders_webgl.wgsl", "d48a0ef2bd567c342f91f9b45ec4483de23e0f9ded11a4a232b3b46f9a43e6c9"),
            ("src/wgpu_renderer.rs", "d37fa295545b2ed128030f94ce428be168a05781fea78ece8e51d7f274f916eb"),
        ],
    };
    generator::generate(manifest, &spec);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, path::PathBuf, sync::atomic::{AtomicUsize, Ordering}};

    /// 独占临时副本只服务于真实校验入口的故障注入，不改项目内字体或原包。
    struct Fixture(PathBuf);
    impl Fixture {
        /// 复制已登记的实际字体及清单，避免使用镜像哈希或模拟字体绕过字节合同。
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = env::temp_dir().join(format!("loki-wgpu-font-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir(&path).unwrap();
            let source = Path::new(env!("CARGO_MANIFEST_DIR"));
            fs::copy(source.join("test-fixtures-content.sha256"), path.join("test-fixtures-content.sha256")).unwrap();
            let mut originals = BTreeMap::new();
            let source_root = source.join("test-fixtures");
            font_fixture_files(&source_root, &source_root, &mut originals).unwrap();
            for (name, bytes) in originals {
                let target = path.join("test-fixtures").join(name);
                fs::create_dir_all(target.parent().unwrap()).unwrap();
                fs::write(target, bytes).unwrap();
            }
            Self(path)
        }

        /// 以夹具相对路径定位实际故障文件。
        fn file(&self, name: &str) -> PathBuf { self.0.join("test-fixtures").join(name) }
    }
    impl Drop for Fixture {
        /// 测试临时副本随作用域释放；不会删除被链接的官方原文件。
        fn drop(&mut self) { fs::remove_dir_all(&self.0).unwrap(); }
    }

    /// 实际迁入的五份官方字节及许可共同通过生产构建使用的校验函数。
    #[test]
    fn verifies_exact_official_font_and_license_bytes() {
        let actual = verified_font_fixtures(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        assert_eq!(actual.len(), 5);
        assert_eq!(actual.values().map(Vec::len).sum::<usize>(), 596_542);
    }

    /// 任一缺失字体都不能靠剩余清单或生成模板继续构建。
    #[test]
    fn rejects_missing_font() {
        let fixture = Fixture::new();
        fs::remove_file(fixture.file("assets/fonts/lilex/Lilex-Regular.ttf")).unwrap();
        assert!(verified_font_fixtures(&fixture.0).unwrap_err().contains("missing font fixture"));
    }

    /// 同长度修改字体或许可均触发真实 SHA 校验，许可不会被当作可选文档。
    #[test]
    fn rejects_modified_font_and_license() {
        for name in ["assets/fonts/lilex/Lilex-Bold.ttf", "assets/fonts/ibm-plex-sans/license.txt"] {
            let fixture = Fixture::new();
            let path = fixture.file(name);
            let mut bytes = fs::read(&path).unwrap();
            bytes[0] ^= 1;
            fs::write(path, bytes).unwrap();
            assert!(verified_font_fixtures(&fixture.0).unwrap_err().contains("font fixture byte drift"));
        }
    }

    /// 额外夹具及篡改后的清单都无法扩张这一次封闭五文件登记。
    #[test]
    fn rejects_extra_file_and_manifest_drift() {
        let fixture = Fixture::new();
        fs::write(fixture.file("assets/fonts/lilex/extra.ttf"), b"extra").unwrap();
        assert!(verified_font_fixtures(&fixture.0).unwrap_err().contains("file set"));
        fs::remove_file(fixture.file("assets/fonts/lilex/extra.ttf")).unwrap();
        fs::write(fixture.0.join("test-fixtures-content.sha256"), b"changed\n").unwrap();
        assert!(verified_font_fixtures(&fixture.0).unwrap_err().contains("manifest drift"));
    }

    /// 即便链接目标字节正确，叶子文件、字体父目录及清单链接也必须拒绝。
    #[cfg(unix)]
    #[test]
    fn rejects_leaf_parent_and_manifest_symlinks() {
        use std::os::unix::fs::symlink;
        let source = Path::new(env!("CARGO_MANIFEST_DIR"));
        for name in ["assets/fonts/lilex/Lilex-Regular.ttf", "assets/fonts/lilex"] {
            let fixture = Fixture::new();
            let path = fixture.file(name);
            if path.is_dir() { fs::remove_dir_all(&path).unwrap(); } else { fs::remove_file(&path).unwrap(); }
            symlink(source.join("test-fixtures").join(name), path).unwrap();
            assert!(verified_font_fixtures(&fixture.0).unwrap_err().contains("links"));
        }
        let fixture = Fixture::new();
        let path = fixture.0.join("test-fixtures-content.sha256");
        fs::remove_file(&path).unwrap();
        symlink(source.join("test-fixtures-content.sha256"), path).unwrap();
        assert!(verified_font_fixtures(&fixture.0).unwrap_err().contains("regular file"));
    }
}
