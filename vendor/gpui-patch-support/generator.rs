//! 原样来源和人工补丁分离；构建只能修改 OUT_DIR 内的完整编译镜像。

use std::{collections::{BTreeMap, BTreeSet}, env, fs, path::{Component, Path, PathBuf}};

#[path = "sha256.rs"]
mod sha256;

/// 按 `目录/名称.before|after` 约定登记补丁，名称同时用于文件与错误信息；路径相对调用处源文件。
#[allow(unused_macros, reason = "部分后端不使用补丁登记宏")]
macro_rules! patch {
    ($dir:literal, $name:literal, $path:expr) => {
        $crate::generator::Patch {
            name: $name,
            path: $path,
            before: include_str!(concat!($dir, $name, ".before")),
            after: include_str!(concat!($dir, $name, ".after")),
        }
    };
}

/// 每个维护片段必须在指定原文件中恰好出现一次，不接受模糊匹配。
#[derive(Clone, Copy)]
pub struct Patch {
    pub name: &'static str,
    pub path: &'static str,
    pub before: &'static str,
    pub after: &'static str,
}

/// 清单摘要和生成摘要固定在维护代码中，来源与补丁任意漂移均失败关闭。
pub struct Spec<'a> {
    pub entry: &'static str,
    pub root_prefix: &'static str,
    pub source_manifest_sha256: &'static str,
    pub patches: &'a [Patch],
    pub generated_hashes: &'static [(&'static str, &'static str)],
}

/// 只允许普通相对路径，拒绝父目录、绝对路径及 Windows 路径分隔符。
fn relative(value: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    if value.is_empty() || value.contains('\\') || value.contains(':')
        || path.components().any(|part| !matches!(part, Component::Normal(_))) {
        return Err(format!("unsafe source path: {value}"));
    }
    Ok(path.to_owned())
}

/// 读取实际完整文件集，链接和特殊文件不能混入原包登记范围。
fn files(root: &Path, directory: &Path, found: &mut BTreeMap<String, Vec<u8>>) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        let kind = fs::symlink_metadata(&path).map_err(|error| error.to_string())?.file_type();
        if kind.is_dir() {
            files(root, &path, found)?;
        } else if kind.is_file() {
            let name = path.strip_prefix(root).map_err(|error| error.to_string())?
                .to_str().ok_or("non-UTF8 source path")?.replace('\\', "/");
            relative(&name)?;
            found.insert(name, fs::read(&path).map_err(|error| error.to_string())?);
        } else {
            return Err(format!("upstream links or special files are forbidden: {}", path.display()));
        }
    }
    Ok(())
}

/// 对清单本身及其每个原文件做 SHA-256 校验，并拒绝清单外的额外内容。
fn verified_sources(manifest: &Path, spec: &Spec) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let list = fs::read_to_string(manifest.join("source-content.sha256")).map_err(|error| error.to_string())?;
    if sha256::hex(list.as_bytes()) != spec.source_manifest_sha256 {
        return Err("source-content.sha256 drift; verify source provenance before updating".into());
    }
    let upstream = manifest.join("upstream");
    let metadata = fs::symlink_metadata(&upstream).map_err(|error| error.to_string())?;
    if !metadata.is_dir() { return Err("upstream root must be a real directory".into()); }
    let mut actual = BTreeMap::new();
    files(&upstream, &upstream, &mut actual)?;
    let mut expected = BTreeSet::new();
    for line in list.lines() {
        let (digest, name) = line.split_once("  ").ok_or("malformed source SHA line")?;
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("malformed source SHA digest".into());
        }
        relative(name)?;
        if !expected.insert(name.to_owned()) { return Err(format!("duplicate source: {name}")); }
        let bytes = actual.get(name).ok_or_else(|| format!("missing upstream source: {name}"))?;
        if sha256::hex(bytes) != digest { return Err(format!("upstream byte drift: {name}")); }
    }
    if actual.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err("upstream file set differs from fixed source manifest".into());
    }
    Ok(actual)
}

/// 校验后在内存中应用补丁，只允许固定前缀及完整唯一片段。
fn patched_sources(manifest: &Path, spec: &Spec) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut sources = verified_sources(manifest, spec)?;
    let mut changed = BTreeSet::new();
    if !spec.root_prefix.is_empty() {
        let entry = sources.get_mut(spec.entry).ok_or("missing crate entry")?;
        let original = std::str::from_utf8(entry).map_err(|error| error.to_string())?;
        *entry = original.strip_prefix(spec.root_prefix).ok_or("crate root attribute prefix drift")?.as_bytes().to_vec();
    }
    changed.insert(spec.entry);
    for patch in spec.patches {
        relative(patch.path)?;
        if patch.before.is_empty() { return Err(format!("empty original fragment: {}", patch.name)); }
        let bytes = sources.get_mut(patch.path).ok_or_else(|| format!("missing patch target: {}", patch.path))?;
        let original = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
        let matches = original.match_indices(patch.before).count();
        if matches != 1 { return Err(format!("{} requires one exact fragment, found {matches}", patch.name)); }
        *bytes = original.replacen(patch.before, patch.after, 1).into_bytes();
        changed.insert(patch.path);
    }
    let mut checked = BTreeSet::new();
    for &(name, digest) in spec.generated_hashes {
        relative(name)?;
        if !checked.insert(name) { return Err(format!("duplicate generated digest: {name}")); }
        if sha256::hex(sources.get(name).ok_or("missing generated digest target")?) != digest {
            return Err(format!("reviewed generated output drift: {name}"));
        }
    }
    if changed != checked { return Err("generated digests must cover exactly every changed file and crate entry".into()); }
    Ok(sources)
}

/// 输出目录由 Cargo 给出；整个上游树按原路径复制，不手工改写模块或资源路径。
fn generate_into(manifest: &Path, output: &Path, spec: &Spec) -> Result<PathBuf, String> {
    let sources = patched_sources(manifest, spec)?;
    let destination = output.join("gpui-patched");
    if destination.exists() {
        if !fs::symlink_metadata(&destination).map_err(|error| error.to_string())?.is_dir() {
            return Err("generated root must be a real directory".into());
        }
        fs::remove_dir_all(&destination).map_err(|error| error.to_string())?;
    }
    fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
    for (name, bytes) in sources {
        let path = destination.join(relative(&name)?);
        fs::create_dir_all(path.parent().ok_or("missing output parent")?).map_err(|error| error.to_string())?;
        fs::write(path, bytes).map_err(|error| error.to_string())?;
    }
    Ok(destination)
}

/// 构建脚本唯一入口；记录真正的输入后生成，任何来源或补丁错误均终止编译。
pub fn generate(manifest: &Path, spec: &Spec) -> PathBuf {
    println!("cargo:rerun-if-changed={}", manifest.join("upstream").display());
    println!("cargo:rerun-if-changed={}", manifest.join("source-content.sha256").display());
    println!("cargo:rerun-if-changed={}", manifest.join("patches").display());
    println!("cargo:rerun-if-changed={}", manifest.join("../gpui-patch-support").display());
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo build output"));
    generate_into(manifest, &output, spec).unwrap_or_else(|error| panic!("GPUI source generation failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// 仅创建独占临时夹具，测试结束自动释放，不依赖 Cargo 或第三方临时目录包。
    struct Fixture(PathBuf);
    impl Fixture {
        /// 原始片段、清单及预期输出固定，复现构建前后的三层完整性校验。
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = env::temp_dir().join(format!("loki-gpui-generator-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir_all(path.join("upstream/src")).unwrap();
            fs::write(path.join("upstream/src/lib.rs"), "#![allow(dead_code)]\nneedle\n").unwrap();
            fs::write(path.join("source-content.sha256"), format!("{}  src/lib.rs\n", sha256::hex(b"#![allow(dead_code)]\nneedle\n"))).unwrap();
            Self(path)
        }
        /// 为测试提供独立固定值；泄露几个短字符串只持续到测试进程退出。
        fn spec(&self, before: &'static str, after: &'static str) -> Spec<'static> {
            let digest = sha256::hex(&fs::read(self.0.join("source-content.sha256")).unwrap());
            Spec {
                entry: "src/lib.rs", root_prefix: "#![allow(dead_code)]\n",
                source_manifest_sha256: Box::leak(digest.into_boxed_str()),
                patches: Box::leak(vec![Patch { name: "test", path: "src/lib.rs", before, after }].into_boxed_slice()),
                generated_hashes: Box::leak(vec![("src/lib.rs", Box::leak(sha256::hex(b"value\n").into_boxed_str()) as &'static str)].into_boxed_slice()),
            }
        }
    }
    impl Drop for Fixture { fn drop(&mut self) { fs::remove_dir_all(&self.0).unwrap(); } }

    /// 镜像含补丁而原包字节保持不变，证明写入只发生在独立输出树。
    #[test]
    fn generates_without_mutating_upstream() {
        let fixture = Fixture::new();
        let spec = fixture.spec("needle", "value");
        let output = generate_into(&fixture.0, &fixture.0.join("out"), &spec).unwrap();
        assert_eq!(fs::read(output.join("src/lib.rs")).unwrap(), b"value\n");
        assert_eq!(fs::read(fixture.0.join("upstream/src/lib.rs")).unwrap(), b"#![allow(dead_code)]\nneedle\n");
    }

    /// 输入字节或清单外文件漂移均先于补丁应用失败。
    #[test]
    fn rejects_source_and_file_set_drift() {
        let fixture = Fixture::new();
        let spec = fixture.spec("needle", "value");
        fs::write(fixture.0.join("upstream/extra"), b"extra").unwrap();
        assert!(patched_sources(&fixture.0, &spec).unwrap_err().contains("file set"));
        fs::remove_file(fixture.0.join("upstream/extra")).unwrap();
        fs::write(fixture.0.join("upstream/src/lib.rs"), b"changed").unwrap();
        assert!(patched_sources(&fixture.0, &spec).unwrap_err().contains("byte drift"));
    }

    /// 原片段缺失、空片段和补丁结果变化均无法通过固定输出摘要。
    #[test]
    fn rejects_patch_and_output_drift() {
        let fixture = Fixture::new();
        for (before, after, message) in [("missing", "value", "exact fragment"), ("", "value", "empty original"), ("needle", "wrong", "output drift")] {
            assert!(patched_sources(&fixture.0, &fixture.spec(before, after)).unwrap_err().contains(message));
        }
    }

    /// 任意绝对或父目录路径都不能跳出 immutable upstream 与生成目录。
    #[test]
    fn rejects_unsafe_paths() {
        for name in ["", "/root", "../root", "src/../root", "C:/root", "src\\root"] { assert!(relative(name).is_err()); }
        assert_eq!(relative("src/window.rs").unwrap(), Path::new("src/window.rs"));
    }
}
