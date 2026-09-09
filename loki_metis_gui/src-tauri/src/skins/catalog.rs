/// 执行换皮宿主内部的 `catalog_cache_error` 步骤。
fn catalog_cache_error() -> AppError {
    AppError::new("skin.library_unavailable", "皮肤资源缓存不可用。")
}

/// 执行换皮宿主内部的 `import_state_error` 步骤。
fn import_state_error() -> AppError {
    AppError::new("skin.import_failed", "皮肤安装暂存状态不可用。")
}

/// 执行换皮宿主内部的 `catalog_fingerprint` 步骤。
fn catalog_fingerprint(
    builtin_root: &Path,
    user_root: &Path,
) -> Result<Vec<CatalogDirectoryFingerprint>, AppError> {
    let mut fingerprint = catalog_root_fingerprint(builtin_root, SkinSource::Builtin)?;
    fingerprint.extend(catalog_root_fingerprint(user_root, SkinSource::User)?);
    fingerprint.sort_by(|left, right| {
        (left.source == SkinSource::User)
            .cmp(&(right.source == SkinSource::User))
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(fingerprint)
}

/// 执行换皮宿主内部的 `catalog_root_fingerprint` 步骤。
fn catalog_root_fingerprint(
    root: &Path,
    source: SkinSource,
) -> Result<Vec<CatalogDirectoryFingerprint>, AppError> {
    let message = match source {
        SkinSource::Builtin => "无法读取应用内置皮肤资源。",
        SkinSource::User => "无法读取用户皮肤资源库。",
    };
    let entries = std::fs::read_dir(root).map_err(|_| {
        AppError::new(
            if source == SkinSource::Builtin {
                "skin.builtin_unavailable"
            } else {
                "skin.library_unavailable"
            },
            message,
        )
    })?;
    let mut directories = Vec::new();
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        if !name.to_str().is_some_and(is_valid_skin_id) {
            continue;
        }
        let path = entry.path();
        let modified = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        let (children_readable, mut children) = match std::fs::read_dir(&path) {
            Ok(entries) => {
                let children = entries
                    .flatten()
                    .map(|child| {
                        let name = child.file_name();
                        match std::fs::symlink_metadata(child.path()) {
                            Ok(metadata) => {
                                let file_type = metadata.file_type();
                                let kind = if file_type.is_symlink() {
                                    2
                                } else if file_type.is_file() {
                                    0
                                } else if file_type.is_dir() {
                                    1
                                } else {
                                    3
                                };
                                CatalogChildFingerprint {
                                    name,
                                    kind,
                                    length: metadata.len(),
                                    modified: metadata.modified().ok(),
                                }
                            }
                            Err(_) => CatalogChildFingerprint {
                                name,
                                kind: u8::MAX,
                                length: 0,
                                modified: None,
                            },
                        }
                    })
                    .collect::<Vec<_>>();
                (true, children)
            }
            Err(_) => (false, Vec::new()),
        };
        children.sort();
        directories.push(CatalogDirectoryFingerprint {
            source,
            name,
            modified,
            children_readable,
            children,
        });
    }
    directories.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(directories)
}

/// 执行换皮宿主内部的 `directory_latest_modified` 步骤。
fn directory_latest_modified(path: &Path) -> Option<SystemTime> {
    let directory_modified = std::fs::symlink_metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok());
    std::fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            std::fs::symlink_metadata(entry.path())
                .ok()
                .and_then(|metadata| metadata.modified().ok())
        })
        .fold(directory_modified, |latest, modified| {
            Some(latest.map_or(modified, |current| current.max(modified)))
        })
}

/// 执行换皮宿主内部的 `scan_catalog_root` 步骤。
fn scan_catalog_root(
    root: &Path,
    source: SkinSource,
    skins: &mut Vec<CatalogSkin>,
) -> Result<(), AppError> {
    let message = match source {
        SkinSource::Builtin => "无法读取应用内置皮肤资源。",
        SkinSource::User => "无法读取用户皮肤资源库。",
    };
    let entries = std::fs::read_dir(root).map_err(|_| {
        AppError::new(
            if source == SkinSource::Builtin {
                "skin.builtin_unavailable"
            } else {
                "skin.library_unavailable"
            },
            message,
        )
    })?;
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !is_valid_skin_id(&id) {
            continue;
        }
        let path = entry.path();
        match load_descriptor(&path, &id, source) {
            Ok(descriptor) => skins.push(CatalogSkin {
                descriptor,
                modified: directory_latest_modified(&path),
            }),
            Err(error) => tracing::warn!("跳过无效皮肤资源，来源={source:?}，错误码={}", error.code),
        }
    }
    Ok(())
}

/// 执行换皮宿主内部的 `path_is_occupied` 步骤。
fn path_is_occupied(path: &Path) -> Result<bool, AppError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(AppError::new(
            "skin.library_unavailable",
            "暂时无法检查用户皮肤资源库，请稍后重试。",
        )),
    }
}

impl PendingImportBatch {
    /// 执行换皮宿主内部的 `response` 步骤。
    fn response(&self) -> PreparedSkinImportBatch {
        PreparedSkinImportBatch {
            token: self.token.clone(),
            items: self
                .items
                .iter()
                .map(|item| PreparedSkinImportItem {
                    item_id: item.item_id.clone(),
                    archive_name: item.archive_name.clone(),
                    skin: item.candidate.clone(),
                })
                .collect(),
            total_files: self.items.len() + self.skipped.len(),
            skipped: self.skipped.clone(),
        }
    }
}

/// 执行换皮宿主内部的 `prepare_import_batch_in_worker` 步骤。
fn prepare_import_batch_in_worker(
    user_root: &Path,
    sources: Vec<PathBuf>,
    token: String,
    cancelled: &AtomicBool,
    progress: &Arc<dyn Fn(SkinImportPreparationEvent) + Send + Sync>,
) -> Result<PendingImportBatch, AppError> {
    ensure_import_not_cancelled(cancelled)?;
    std::fs::create_dir_all(user_root)
        .map_err(|_| AppError::new("skin.library_unavailable", "无法准备用户皮肤资源库。"))?;
    let staging_root = user_root.join(format!(".import-{token}"));
    std::fs::create_dir(&staging_root)
        .map_err(|_| AppError::new("skin.import_failed", "无法创建皮肤导入暂存目录。"))?;
    let mut items = Vec::new();
    let mut skipped = Vec::new();
    let mut reserved_ids = HashSet::new();
    let mut batch_bytes = 0_u64;
    for (index, source) in sources.iter().enumerate() {
        if let Err(error) = ensure_import_not_cancelled(cancelled) {
            let _ = std::fs::remove_dir_all(&staging_root);
            return Err(error);
        }
        let archive_name = source
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("未知文件.zip")
            .to_owned();
        let staging = staging_root.join(format!("item-{index}"));
        let prepared_item = (|| {
            if !source
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("zip"))
            {
                return Err(AppError::new("skin.import_invalid", "文件不是 ZIP 格式。"));
            }
            std::fs::create_dir(&staging)
                .map_err(|_| AppError::new("skin.import_failed", "无法创建皮肤导入暂存目录。"))?;
            let extracted_bytes = extract_zip(source, &staging, cancelled)?;
            batch_bytes = checked_batch_import_bytes(batch_bytes, extracted_bytes)?;
            synthesize_legacy_manifest_if_missing(source, &staging)?;
            let manifest = read_manifest(&staging)?;
            validate_skin_id(manifest.id())?;
            validate_manifest(&staging, &manifest)?;
            let (install_id, install_name) = available_import_identity_with_reserved(
                user_root,
                manifest.id(),
                manifest.name().trim(),
                &reserved_ids,
            )?;
            rewrite_import_identity(&staging, &install_id, &install_name)?;
            let candidate = load_descriptor(&staging, &install_id, SkinSource::User)?;
            Ok(PendingImportItem {
                item_id: format!("item-{index}"),
                archive_name: archive_name.clone(),
                staging: staging.clone(),
                candidate,
            })
        })();
        if let Err(error) = ensure_import_not_cancelled(cancelled) {
            let _ = std::fs::remove_dir_all(&staging_root);
            return Err(error);
        }
        match prepared_item {
            Ok(item) => {
                reserved_ids.insert(item.candidate.id.clone());
                progress(SkinImportPreparationEvent::ItemReady {
                    token: token.clone(),
                    item: PreparedSkinImportItem {
                        item_id: item.item_id.clone(),
                        archive_name: item.archive_name.clone(),
                        skin: item.candidate.clone(),
                    },
                });
                items.push(item);
            }
            Err(error) => {
                if staging.exists() {
                    let _ = std::fs::remove_dir_all(&staging);
                }
                let skipped_item = SkippedSkinImport {
                    archive_name,
                    code: error.code,
                    message: error.message,
                    details: error.details,
                };
                progress(SkinImportPreparationEvent::ItemSkipped {
                    token: token.clone(),
                    item: skipped_item.clone(),
                });
                skipped.push(skipped_item);
            }
        }
    }
    if let Err(error) = ensure_import_not_cancelled(cancelled) {
        let _ = std::fs::remove_dir_all(&staging_root);
        return Err(error);
    }
    if items.is_empty() {
        let _ = std::fs::remove_dir_all(&staging_root);
        if skipped.len() == 1 {
            let item = skipped.remove(0);
            return Err(AppError::with_details(
                item.code,
                item.message,
                item.details,
            ));
        }
        let details = skipped
            .iter()
            .map(|item| format!("{}：{}", item.archive_name, item.message))
            .collect();
        return Err(AppError::with_details(
            "skin.import_batch_empty",
            "所选文件中没有可导入的皮肤包。",
            details,
        ));
    }
    Ok(PendingImportBatch {
        token,
        staging_root,
        items,
        skipped,
    })
}

/// 执行换皮宿主内部的 `next_import_token` 步骤。
fn next_import_token() -> String {
    let suffix = IMPORT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{}-{suffix}", std::process::id())
}

/// 执行换皮宿主内部的 `ensure_import_not_cancelled` 步骤。
fn ensure_import_not_cancelled(cancelled: &AtomicBool) -> Result<(), AppError> {
    if cancelled.load(Ordering::Acquire) {
        Err(import_cancelled_error())
    } else {
        Ok(())
    }
}

/// 执行换皮宿主内部的 `import_cancelled_error` 步骤。
fn import_cancelled_error() -> AppError {
    AppError::new("skin.import_cancelled", "已取消这批主题/皮肤预检。")
}

/// 执行换皮宿主内部的 `checked_batch_import_bytes` 步骤。
fn checked_batch_import_bytes(current: u64, next: u64) -> Result<u64, AppError> {
    let total = current.saturating_add(next);
    if total > MAX_IMPORT_BATCH_BYTES {
        return Err(AppError::new(
            "skin.import_batch_too_large",
            "这批皮肤包解压后的总体积超过 6.4 GiB。",
        ));
    }
    Ok(total)
}

/// 执行换皮宿主内部的 `available_import_identity` 步骤。
fn available_import_identity(
    user_root: &Path,
    base_id: &str,
    base_name: &str,
) -> Result<(String, String), AppError> {
    available_import_identity_with_reserved(user_root, base_id, base_name, &HashSet::new())
}

/// 执行换皮宿主内部的 `available_import_identity_with_reserved` 步骤。
fn available_import_identity_with_reserved(
    user_root: &Path,
    base_id: &str,
    base_name: &str,
    reserved_ids: &HashSet<String>,
) -> Result<(String, String), AppError> {
    if !reserved_ids.contains(base_id) && !path_is_occupied(&user_root.join(base_id))? {
        return Ok((base_id.to_owned(), base_name.to_owned()));
    }

    for index in 1..=1_000_000_u32 {
        let id_suffix = format!("_{index}");
        let id_stem_length = 64_usize.saturating_sub(id_suffix.len());
        let id_stem = &base_id[..base_id.len().min(id_stem_length)];
        let candidate_id = format!("{id_stem}{id_suffix}");
        if reserved_ids.contains(&candidate_id) || path_is_occupied(&user_root.join(&candidate_id))?
        {
            continue;
        }

        let name_suffix = format!("({index})");
        let name_stem_length = 80_usize.saturating_sub(name_suffix.chars().count());
        let name_stem = base_name.chars().take(name_stem_length).collect::<String>();
        return Ok((candidate_id, format!("{name_stem}{name_suffix}")));
    }

    Err(AppError::new(
        "skin.import_failed",
        "这个皮肤已经安装了太多份，请先整理用户皮肤后再试。",
    ))
}

/// 执行换皮宿主内部的 `rewrite_import_identity` 步骤。
fn rewrite_import_identity(directory: &Path, id: &str, name: &str) -> Result<(), AppError> {
    let path = directory.join("theme.json");
    let text = read_text(&path)?;
    let mut value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| AppError::new("skin.assets_invalid", "皮肤主题配置不是有效 JSON。"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| AppError::new("skin.assets_invalid", "皮肤主题配置的内容结构无效。"))?;
    object.insert("id".into(), serde_json::Value::String(id.to_owned()));
    object.insert("name".into(), serde_json::Value::String(name.to_owned()));
    let updated = serde_json::to_vec_pretty(&value)
        .map_err(|_| AppError::new("skin.import_failed", "无法准备皮肤安装信息。"))?;
    std::fs::write(path, updated)
        .map_err(|_| AppError::new("skin.import_failed", "无法准备皮肤安装信息。"))
}

/// 执行换皮宿主内部的 `load_descriptor` 步骤。
fn load_descriptor(
    directory: &Path,
    id: &str,
    source: SkinSource,
) -> Result<SkinDescriptor, AppError> {
    let manifest = read_manifest(directory)?;
    validate_manifest(directory, &manifest)?;
    load_descriptor_from_manifest(directory, id, source, &manifest)
}

/// 与 `load_descriptor` 相同，但复用调用方已解析并校验过的 manifest，避免重复读取磁盘。
fn load_descriptor_from_manifest(
    directory: &Path,
    id: &str,
    source: SkinSource,
    manifest: &SkinManifest,
) -> Result<SkinDescriptor, AppError> {
    if manifest.id() != id {
        return Err(AppError::new(
            "skin.assets_invalid",
            "皮肤目录标识与 theme.json 不一致。",
        ));
    }
    let preview_file = match &manifest {
        SkinManifest::Legacy(manifest) => manifest.image.clone(),
        SkinManifest::ThemeCss(manifest) => {
            read_theme_css_config(directory, manifest.appearance.as_ref())?.preview
        }
    };
    let preview_data_url =
        image_data_url(&directory.join(&preview_file), image_mime(&preview_file)?)?;
    Ok(SkinDescriptor {
        id: id.into(),
        source,
        package_type: manifest.package_type(),
        name: manifest.name().trim().to_owned(),
        author: manifest.author().trim().to_owned(),
        version: SKIN_VERSION.into(),
        preview_data_url,
        supported_color_modes: manifest.supported_color_modes(),
    })
}

/// 执行换皮宿主内部的 `load_skin` 步骤。
fn load_skin(
    builtin_root: &Path,
    user_root: &Path,
    skin: &SkinReference,
    explicitly_trusted: bool,
) -> Result<LoadedSkin, AppError> {
    let directory = match skin.source {
        SkinSource::Builtin => builtin_root.join(&skin.id),
        SkinSource::User => user_root.join(&skin.id),
    };
    if !directory.is_dir() {
        return Err(AppError::new("skin.not_found", "所选皮肤资源不存在。"));
    }
    let manifest = read_manifest(&directory)?;
    validate_manifest(&directory, &manifest)?;
    let appearance_requirements = match &manifest {
        SkinManifest::ThemeCss(manifest) => manifest
            .appearance
            .as_ref()
            .and_then(|appearance| appearance.requirements.clone()),
        SkinManifest::Legacy(_) => None,
    };
    let descriptor = load_descriptor_from_manifest(&directory, &skin.id, skin.source, &manifest)?;
    let payload = append_runtime_skin_marker(
        build_payload_from_manifest(&directory, &manifest, explicitly_trusted)?,
        &descriptor,
    )?;
    Ok(LoadedSkin {
        descriptor,
        payload: Arc::from(payload),
        appearance_requirements,
    })
}
