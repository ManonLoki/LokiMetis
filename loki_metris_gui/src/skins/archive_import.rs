//! 皮肤归档导入：有界安全解压与缺失旧版清单的兼容补全。

use super::*;

/// 安全解压导入 ZIP：限制条目数与解压体积，拒绝路径穿越与链接，识别唯一的皮肤根目录并只解出该目录内容；返回解压字节数。
pub(super) fn extract_zip(source: &Path, destination: &Path, cancelled: &AtomicBool) -> Result<u64, AppError> {
    ensure_import_not_cancelled(cancelled)?;
    let file = std::fs::File::open(source)
        .map_err(|_| AppError::new("skin.import_invalid", "无法读取所选 ZIP 文件。"))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|_| AppError::new("skin.import_invalid", "皮肤压缩包不是有效 ZIP。"))?;
    if archive.len() > MAX_IMPORT_ENTRIES {
        return Err(AppError::new(
            "skin.import_too_large",
            "皮肤压缩包文件数量过多。",
        ));
    }

    let mut manifest_roots = Vec::<Option<PathBuf>>::new();
    let mut file_roots = BTreeSet::<Option<PathBuf>>::new();
    for index in 0..archive.len() {
        ensure_import_not_cancelled(cancelled)?;
        let entry = archive
            .by_index(index)
            .map_err(|_| AppError::new("skin.import_invalid", "无法读取皮肤压缩包条目。"))?;
        let path = entry
            .enclosed_name()
            .ok_or_else(|| AppError::new("skin.import_invalid", "皮肤压缩包包含越界路径。"))?;
        let components: Vec<_> = path.components().collect();
        if !entry.is_dir() {
            match components.as_slice() {
                [Component::Normal(_)] => {
                    file_roots.insert(None);
                }
                [Component::Normal(root), ..] => {
                    file_roots.insert(Some(PathBuf::from(root)));
                }
                _ => {}
            }
        }
        if components.as_slice() == [Component::Normal("theme.json".as_ref())] {
            manifest_roots.push(None);
        } else if components.len() == 2 && components[1] == Component::Normal("theme.json".as_ref())
        {
            let Component::Normal(root) = components[0] else {
                continue;
            };
            manifest_roots.push(Some(PathBuf::from(root)));
        }
    }
    if manifest_roots.len() > 1 {
        return Err(AppError::new(
            "skin.import_invalid",
            "这个 ZIP 中有多个 theme.json，无法判断应该使用哪一个。",
        ));
    }
    let root = if manifest_roots.len() == 1 {
        manifest_roots.pop().flatten()
    } else if file_roots.len() == 1 {
        file_roots.pop_first().flatten()
    } else {
        return Err(AppError::with_details(
            "skin.import_invalid",
            "没有找到可识别的皮肤文件结构。",
            vec![
                "请把皮肤文件直接放在 ZIP 根目录，或统一放在一个文件夹中。".into(),
                "没有 theme.json 的旧皮肤仍需包含五个固定运行文件。".into(),
            ],
        ));
    };
    let mut total_bytes = 0_u64;
    for index in 0..archive.len() {
        ensure_import_not_cancelled(cancelled)?;
        let mut entry = archive
            .by_index(index)
            .map_err(|_| AppError::new("skin.import_invalid", "无法读取皮肤压缩包条目。"))?;
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| AppError::new("skin.import_invalid", "皮肤压缩包包含越界路径。"))?;
        let relative = match &root {
            Some(prefix) => match enclosed.strip_prefix(prefix) {
                Ok(value) => value,
                Err(_) => continue,
            },
            None => enclosed.as_path(),
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(AppError::new(
                "skin.import_invalid",
                "皮肤压缩包不能包含符号链接。",
            ));
        }
        if entry.size() > MAX_IMAGE_BYTES {
            return Err(AppError::new(
                "skin.import_too_large",
                "皮肤资源单个文件过大。",
            ));
        }
        total_bytes = total_bytes.saturating_add(entry.size());
        if total_bytes > MAX_IMPORT_BYTES {
            return Err(AppError::new(
                "skin.import_too_large",
                "皮肤资源总体积过大。",
            ));
        }
        let target = destination.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&target)
                .map_err(|_| AppError::new("skin.import_failed", "无法创建皮肤资源目录。"))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|_| AppError::new("skin.import_failed", "无法创建皮肤资源目录。"))?;
        }
        let mut output = std::fs::File::create(target)
            .map_err(|_| AppError::new("skin.import_failed", "无法写入皮肤资源文件。"))?;
        let copied = std::io::copy(&mut entry.by_ref().take(MAX_IMAGE_BYTES + 1), &mut output)
            .map_err(|_| AppError::new("skin.import_failed", "无法解压皮肤资源文件。"))?;
        output
            .flush()
            .map_err(|_| AppError::new("skin.import_failed", "无法保存皮肤资源文件。"))?;
        if copied > MAX_IMAGE_BYTES {
            return Err(AppError::new(
                "skin.import_too_large",
                "皮肤资源单个文件过大。",
            ));
        }
    }
    Ok(total_bytes)
}

/// 没有清单的旧版皮肤包在五个运行文件齐全时生成兼容清单；有 theme.css 却缺清单或文件不全时报具体问题。
pub(super) fn synthesize_legacy_manifest_if_missing(source: &Path, directory: &Path) -> Result<(), AppError> {
    let manifest_path = directory.join("theme.json");
    if manifest_path.exists() {
        return Ok(());
    }

    if directory.join("theme.css").exists() {
        return Err(import_resource_error(vec![resource_file_problem(
            directory,
            "theme.json",
            MAX_TEXT_BYTES,
        )
        .unwrap_or_else(|| "缺少皮肤说明文件「theme.json」。".into())]));
    }

    let legacy_files_present = LEGACY_RUNTIME_FILES
        .iter()
        .filter(|file| directory.join(file).is_file())
        .count();
    if legacy_files_present == 0 {
        return Err(AppError::with_details(
            "skin.import_invalid",
            "这个 ZIP 中没有找到可识别的主题或旧版皮肤。",
            vec![
                "新版主题需要皮肤说明文件「theme.json」和主题变量文件「theme.css」。".into(),
                "无清单旧皮肤需要五个固定运行文件。".into(),
            ],
        ));
    }

    let problems = legacy_resource_problems(directory, &LEGACY_RUNTIME_FILES);
    if !problems.is_empty() {
        return Err(import_resource_error(problems));
    }

    let source_name = source
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("导入的旧版皮肤");
    let name: String = source_name.chars().take(80).collect();
    let manifest = LegacyManifest {
        schema_version: 1,
        id: inferred_legacy_id(source_name),
        description: format!("由LokiMetris从旧版皮肤包「{name}」生成的兼容清单。"),
        name,
        author: unknown_author(),
        image: "qq2007-sky.png".into(),
        friend_cards: FriendCards {
            profile_image: "avatar.png".into(),
            list_image: "qqshow.jpg".into(),
        },
        colors: None,
    };
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|_| {
        AppError::new(
            "skin.import_failed",
            "旧版皮肤文件已识别，但无法生成兼容说明文件。",
        )
    })?;
    std::fs::write(manifest_path, bytes).map_err(|_| {
        AppError::new(
            "skin.import_failed",
            "旧版皮肤文件已识别，但无法保存兼容说明文件。",
        )
    })
}

/// 由归档名推导旧版皮肤 ID：取 ASCII 字母数字组成短标识，再拼接名称的 FNV-1a 哈希保证唯一。
fn inferred_legacy_id(name: &str) -> String {
    let mut slug = String::new();
    let mut separator_pending = false;
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            if separator_pending && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character.to_ascii_lowercase());
            separator_pending = false;
        } else if matches!(character, '-' | '_' | ' ' | '.') {
            separator_pending = true;
        }
        if slug.len() >= 32 {
            break;
        }
    }
    let slug = slug.trim_matches('-');
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    if slug.is_empty() {
        format!("legacy-{hash:016x}")
    } else {
        format!("legacy-{slug}-{hash:016x}")
    }
}
