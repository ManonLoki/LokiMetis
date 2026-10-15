// 自有构建期登记：七份原样测试字体/OFL，不能借目录名豁免其它内容。
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path},
};
#[path = "../gpui-patch-support/sha256.rs"]
mod sha256;

/// 清单本身固定；重新下载或重编码字体必须重新审查，不能自动接纳。
const MANIFEST_SHA256: &str = "beb3fbcd84f005d5e3ff86de06dc6c1e249fea1d1f67d0c50c78ad08d75974d7";

/// 只接纳普通相对文件名，禁止清单路径越界和平台路径歧义。
fn safe_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.contains('\\')
        || name.contains(':')
        || name
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
        || Path::new(name)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("unsafe font fixture path".into());
    }
    Ok(())
}

/// 真实根/目录/文件分别核对，拒绝根或父目录被链接替换。
fn collect(root: &Path, path: &Path, files: &mut BTreeSet<String>) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if meta.is_symlink() {
        return Err("font fixture symlink".into());
    }
    if meta.is_dir() {
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            collect(root, &entry.map_err(|e| e.to_string())?.path(), files)?;
        }
    } else if meta.is_file() {
        let name = path
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_str()
            .ok_or("non-UTF8 font fixture")?
            .replace('\\', "/");
        safe_name(&name)?;
        files.insert(name);
    } else {
        return Err("font fixture special file".into());
    }
    Ok(())
}

/// 完整清单 SHA、七个原样文件 SHA 和实际文件集全部匹配才允许构建。
pub fn verify(package: &Path) -> Result<(), String> {
    let root = package.join("test-fixtures");
    let list_path = package.join("test-fixtures.sha256");
    if !fs::symlink_metadata(package)
        .map_err(|e| e.to_string())?
        .is_dir()
    {
        return Err("font package root must be real directory".into());
    }
    if !fs::symlink_metadata(&root)
        .map_err(|e| e.to_string())?
        .is_dir()
    {
        return Err("fixture root must be real directory".into());
    }
    if !fs::symlink_metadata(&list_path)
        .map_err(|e| e.to_string())?
        .is_file()
    {
        return Err("fixture manifest must be regular file".into());
    }
    let list = fs::read_to_string(list_path).map_err(|e| e.to_string())?;
    if sha256::hex(list.as_bytes()) != MANIFEST_SHA256 {
        return Err("font fixture manifest drift".into());
    }
    let mut expected = BTreeSet::new();
    for line in list.lines() {
        let (digest, name) = line.split_once("  ").ok_or("malformed fixture line")?;
        safe_name(name)?;
        if digest.len() != 64
            || !digest.bytes().all(|b| b.is_ascii_hexdigit())
            || !expected.insert(name.to_owned())
        {
            return Err("duplicate/malformed font fixture".into());
        }
    }
    let mut actual = BTreeSet::new();
    collect(&root, &root, &mut actual)?;
    if expected.len() != 7 || actual != expected {
        return Err("font fixture file set drift".into());
    }
    for line in list.lines() {
        let (digest, name) = line.split_once("  ").ok_or("malformed fixture line")?;
        if sha256::hex(&fs::read(root.join(name)).map_err(|e| e.to_string())?) != digest {
            return Err(format!("font fixture byte drift: {name}"));
        }
    }
    Ok(())
}
