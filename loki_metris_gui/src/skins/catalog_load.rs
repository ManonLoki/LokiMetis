// 本文件是 catalog.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：复用已校验清单生成描述，以及按引用加载待注入皮肤。

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

/// 按引用定位内置或用户皮肤目录，校验清单并生成注入脚本、展示描述与外观要求。
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
