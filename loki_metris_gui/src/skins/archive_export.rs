//! 皮肤归档导出：选择已验证包资源并写入 ZIP。

use super::*;

/// 列出导出皮肤时需要打包的文件：旧版为固定运行文件，主题为清单、变量表、逐模式样式与引用的图片。
pub(super) fn export_file_names(directory: &Path, manifest: &SkinManifest) -> Result<Vec<String>, AppError> {
    Ok(match manifest {
        SkinManifest::Legacy(_) => LEGACY_REQUIRED_FILES
            .into_iter()
            .map(str::to_owned)
            .collect(),
        SkinManifest::ThemeCss(manifest) => {
            let config = read_theme_css_config(directory, manifest.appearance.as_ref())?;
            let mut files = vec!["theme.json".to_owned(), "theme.css".to_owned()];
            if let Some(appearance) = &manifest.appearance {
                files.extend(
                    appearance
                        .supported_color_modes
                        .iter()
                        .map(|mode| format!("theme.{}.css", mode.as_str())),
                );
            }
            for file in std::iter::once(config.preview)
                .chain(std::iter::once(config.background))
                .chain(config.mode_backgrounds.into_values())
            {
                if !files.contains(&file) {
                    files.push(file);
                }
            }
            files
        }
    })
}

/// 把皮肤目录中的指定文件打包为内存中的 ZIP。
pub(super) fn build_skin_archive(directory: &Path, file_names: &[String]) -> Result<Vec<u8>, AppError> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    for file_name in file_names {
        let bytes = std::fs::read(directory.join(file_name))
            .map_err(|_| AppError::new("skin.export_failed", "无法读取待导出的皮肤资源。"))?;
        writer
            .start_file(file_name, zip::write::SimpleFileOptions::default())
            .map_err(|_| AppError::new("skin.export_failed", "无法创建皮肤导出压缩包。"))?;
        writer
            .write_all(&bytes)
            .map_err(|_| AppError::new("skin.export_failed", "无法写入皮肤导出资源。"))?;
    }
    writer
        .finish()
        .map(Cursor::into_inner)
        .map_err(|_| AppError::new("skin.export_failed", "无法完成皮肤导出压缩包。"))
}
