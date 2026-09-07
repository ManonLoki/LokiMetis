/// 旧皮肤配色角色到 v3 变量的映射，第三项是回退提示中使用的中文名称。
const CONVERTED_COLOR_ROLES: [(&str, &str, &str); 8] = [
    ("background", "--skin-bg", "背景"),
    ("panel", "--skin-panel", "面板"),
    ("panelAlt", "--skin-panel-alt", "次级面板"),
    ("accent", "--skin-accent", "强调色"),
    ("accentAlt", "--skin-accent-alt", "次强调色"),
    ("text", "--skin-text", "正文"),
    ("muted", "--skin-muted", "弱文本"),
    ("line", "--skin-line", "描边"),
];
const CONVERTED_BACKGROUND_FILE: &str = "background.png";
const CONVERTED_CSS_HEADER: &str = "/*\n * 由旧版兼容皮肤转换生成的变量表（schemaVersion 3）。\n * 只迁移了配色角色与背景图片；原皮肤的自由 CSS 与注入脚本不会被转换。\n * 只能修改现有变量的值，不要增加选择器、普通 CSS 属性或 @ 规则。\n */\n";

/// 执行换皮宿主内部的 `converted_theme_id` 步骤。
fn converted_theme_id(base_id: &str) -> String {
    let suffix = "-theme";
    let stem_length = 64_usize.saturating_sub(suffix.len());
    format!("{}{suffix}", &base_id[..base_id.len().min(stem_length)])
}

/// 执行换皮宿主内部的 `converted_theme_name` 步骤。
fn converted_theme_name(base_name: &str) -> String {
    let suffix = "（主题版）";
    let stem_length = 80_usize.saturating_sub(suffix.chars().count());
    let stem = base_name
        .trim()
        .chars()
        .take(stem_length)
        .collect::<String>();
    format!("{stem}{suffix}")
}

/// v3 变量表只接受 `#RRGGBB` 与 `#RRGGBBAA`，因此三位、四位简写在此展开，
/// 其他 CSS 颜色写法一律视为无法迁移。
fn normalize_theme_color(value: &str) -> Option<String> {
    let hex = value.trim().strip_prefix('#')?;
    if !hex.chars().all(|char| char.is_ascii_hexdigit()) {
        return None;
    }
    let expanded = match hex.len() {
        3 | 4 => hex
            .chars()
            .flat_map(|char| [char, char])
            .collect::<String>(),
        6 | 8 => hex.to_owned(),
        _ => return None,
    };
    Some(format!("#{}", expanded.to_ascii_uppercase()))
}

/// 执行换皮宿主内部的 `legacy_palette` 步骤。
fn legacy_palette(
    colors: Option<&serde_json::Value>,
    mode: ColorMode,
) -> Option<&serde_json::Map<String, serde_json::Value>> {
    let object = colors?.as_object()?;
    if object.contains_key("light") || object.contains_key("dark") {
        return object.get(mode.as_str())?.as_object();
    }
    Some(object)
}

/// 返回浅色、深色两套已归一的变量取值，以及无法迁移、需要回退到模板默认值的角色说明。
fn convert_legacy_palettes(
    manifest: &LegacyManifest,
) -> (
    HashMap<&'static str, String>,
    HashMap<&'static str, String>,
    Vec<String>,
) {
    let mut light = HashMap::new();
    let mut dark = HashMap::new();
    let mut fallbacks = Vec::new();
    for (mode, target) in [(ColorMode::Light, &mut light), (ColorMode::Dark, &mut dark)] {
        let palette = legacy_palette(manifest.colors.as_ref(), mode);
        let mode_label = match mode {
            ColorMode::Light => "浅色",
            ColorMode::Dark => "深色",
        };
        for (key, property, label) in CONVERTED_COLOR_ROLES {
            let converted = palette
                .and_then(|palette| palette.get(key))
                .and_then(serde_json::Value::as_str)
                .and_then(normalize_theme_color);
            match converted {
                Some(value) => {
                    target.insert(property, value);
                }
                None => fallbacks.push(format!("{mode_label}模式的{label}")),
            }
        }
    }
    (light, dark, fallbacks)
}

/// 执行换皮宿主内部的 `css_declaration_property` 步骤。
fn css_declaration_property(line: &str) -> Option<&str> {
    let separator = line.find(':')?;
    let property = line[..separator].trim();
    property.starts_with("--skin-").then_some(property)
}

/// 以应用内主题模板为骨架逐行改写，因此非颜色的视觉参数始终继承模板默认值，
/// 输出也必然停留在受限变量白名单内。
fn render_converted_theme_css(
    light: &HashMap<&'static str, String>,
    dark: &HashMap<&'static str, String>,
) -> String {
    let body = THEME_TEMPLATE_CSS
        .split_once("*/")
        .map(|(_, rest)| rest.trim_start_matches(['\r', '\n']))
        .unwrap_or(THEME_TEMPLATE_CSS);
    let mut output = String::from(CONVERTED_CSS_HEADER);
    let mut mode = None;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.ends_with('{') {
            mode = if trimmed.contains(r#"data-dream-shell="dark""#) {
                Some(ColorMode::Dark)
            } else if trimmed.contains(r#"data-dream-shell="light""#) {
                Some(ColorMode::Light)
            } else {
                None
            };
        } else if trimmed == "}" {
            mode = None;
        } else if let Some(property) = css_declaration_property(trimmed) {
            let indent = &line[..line.len() - line.trim_start().len()];
            let replacement = match mode {
                None if matches!(property, "--skin-preview-image" | "--skin-background-image") => {
                    Some(format!("url(\"{CONVERTED_BACKGROUND_FILE}\")"))
                }
                None => None,
                Some(ColorMode::Light) => light.get(property).cloned(),
                Some(ColorMode::Dark) => dark.get(property).cloned(),
            };
            if let Some(value) = replacement {
                output.push_str(&format!("{indent}{property}: {value};\n"));
                continue;
            }
        }
        output.push_str(line);
        output.push('\n');
    }
    output
}

/// 执行换皮宿主内部的 `write_converted_theme` 步骤。
fn write_converted_theme(
    staging: &Path,
    source: &Path,
    legacy: &LegacyManifest,
    id: &str,
    name: &str,
) -> Result<Vec<String>, AppError> {
    let (light, dark, fallback_roles) = convert_legacy_palettes(legacy);
    let mut manifest: PureThemeCssManifest = serde_json::from_str(THEME_TEMPLATE_MANIFEST)
        .map_err(|_| AppError::new("skin.convert_failed", "应用内主题模板清单无效。"))?;
    manifest.id = id.to_owned();
    manifest.name = name.to_owned();
    manifest.author = legacy.author.trim().to_owned();
    manifest.description = match legacy.description.trim() {
        description if !description.is_empty() && description.chars().count() <= 500 => {
            description.to_owned()
        }
        _ => format!("由旧版兼容皮肤「{}」转换而来的纯主题。", legacy.name.trim()),
    };
    manifest._comment = Some(format!(
        "这是由旧版兼容皮肤「{}」转换生成的 CSS 变量主题。转换只迁移了配色与背景图片，原皮肤的自由 CSS 与注入脚本没有转换。请不要修改 id；颜色、图片和有限视觉参数统一在 theme.css 中修改。",
        legacy.name.trim()
    ));
    let manifest = serde_json::to_vec_pretty(&manifest)
        .map_err(|_| AppError::new("skin.convert_failed", "无法生成转换后的主题清单。"))?;
    std::fs::write(staging.join("theme.json"), manifest)
        .map_err(|_| AppError::new("skin.convert_failed", "无法写入转换后的主题清单。"))?;
    std::fs::write(
        staging.join("theme.css"),
        render_converted_theme_css(&light, &dark),
    )
    .map_err(|_| AppError::new("skin.convert_failed", "无法写入转换后的主题变量表。"))?;
    std::fs::copy(
        source.join("qq2007-sky.png"),
        staging.join(CONVERTED_BACKGROUND_FILE),
    )
    .map_err(|_| AppError::new("skin.convert_failed", "无法复制旧皮肤的背景图片。"))?;
    Ok(fallback_roles)
}

/// 执行换皮宿主内部的 `export_file_names` 步骤。
fn export_file_names(directory: &Path, manifest: &SkinManifest) -> Result<Vec<String>, AppError> {
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

/// 执行换皮宿主内部的 `build_skin_archive` 步骤。
fn build_skin_archive(directory: &Path, file_names: &[String]) -> Result<Vec<u8>, AppError> {
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

/// 执行换皮宿主内部的 `save_export_archive` 步骤。
pub fn save_export_archive(destination: &Path, archive: &[u8]) -> Result<(), AppError> {
    std::fs::write(destination, archive)
        .map_err(|_| AppError::new("skin.export_failed", "无法保存皮肤导出压缩包。"))
}

/// 执行换皮宿主内部的 `extract_zip` 步骤。
fn extract_zip(source: &Path, destination: &Path, cancelled: &AtomicBool) -> Result<u64, AppError> {
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

/// 执行换皮宿主内部的 `synthesize_legacy_manifest_if_missing` 步骤。
fn synthesize_legacy_manifest_if_missing(source: &Path, directory: &Path) -> Result<(), AppError> {
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
        description: format!("由LokiMetis从旧版皮肤包「{name}」生成的兼容清单。"),
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

/// 执行换皮宿主内部的 `inferred_legacy_id` 步骤。
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
