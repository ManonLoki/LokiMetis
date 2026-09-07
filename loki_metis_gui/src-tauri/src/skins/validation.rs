/// 执行换皮宿主内部的 `read_manifest` 步骤。
fn read_manifest(directory: &Path) -> Result<SkinManifest, AppError> {
    let text = read_text(&directory.join("theme.json"))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| AppError::new("skin.assets_invalid", "皮肤主题配置不是有效 JSON。"))?;
    match value
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
    {
        Some(1) => serde_json::from_value(value)
            .map(SkinManifest::Legacy)
            .map_err(|_| AppError::new("skin.assets_invalid", "旧版皮肤配置字段无效。")),
        Some(2) => Err(AppError::with_details(
            "skin.import_unsupported",
            "这个主题使用的是已经停用的过渡格式。",
            vec!["请将它转换为 schemaVersion 3 的新版主题 ZIP 后再导入。".into()],
        )),
        Some(3) if value.get("type").and_then(serde_json::Value::as_str) == Some("theme") => {
            serde_json::from_value(value)
                .map(Box::new)
                .map(SkinManifest::ThemeCss)
                .map_err(|_| {
                    AppError::new(
                        "skin.assets_invalid",
                        "CSS 变量主题配置字段无效或包含未开放字段。",
                    )
                })
        }
        _ => Err(AppError::new(
            "skin.assets_invalid",
            "皮肤主题配置版本或类型不受支持。",
        )),
    }
}

/// 执行换皮宿主内部的 `validate_manifest` 步骤。
fn validate_manifest(directory: &Path, manifest: &SkinManifest) -> Result<(), AppError> {
    if normalize_skin_creator_text(manifest.name()).is_err()
        || normalize_skin_creator_text(manifest.author()).is_err()
        || !is_valid_skin_id(manifest.id())
    {
        return Err(AppError::new(
            "skin.assets_invalid",
            "皮肤主题配置版本、标识、标题或作者无效。",
        ));
    }
    match manifest {
        SkinManifest::Legacy(manifest) => validate_legacy_manifest(directory, manifest),
        SkinManifest::ThemeCss(manifest) => validate_theme_css_manifest(directory, manifest),
    }
}

/// 执行换皮宿主内部的 `validate_legacy_manifest` 步骤。
fn validate_legacy_manifest(directory: &Path, manifest: &LegacyManifest) -> Result<(), AppError> {
    if manifest.schema_version != 1
        || manifest.image != "qq2007-sky.png"
        || manifest.friend_cards.profile_image != "avatar.png"
        || manifest.friend_cards.list_image != "qqshow.jpg"
    {
        return Err(AppError::new(
            "skin.assets_invalid",
            "旧版皮肤配置引用了资源目录之外或不受支持的图片。",
        ));
    }
    let problems = legacy_resource_problems(directory, &LEGACY_REQUIRED_FILES);
    if !problems.is_empty() {
        return Err(import_resource_error(problems));
    }
    Ok(())
}

/// 执行换皮宿主内部的 `validate_theme_css_manifest` 步骤。
fn validate_theme_css_manifest(
    directory: &Path,
    manifest: &PureThemeCssManifest,
) -> Result<(), AppError> {
    if manifest.schema_version != 3
        || manifest.package_type != "theme"
        || validate_theme_metadata(
            &manifest.id,
            &manifest.name,
            &manifest.author,
            &manifest.description,
            manifest._comment.as_deref(),
        )
        .is_err()
    {
        return Err(AppError::new(
            "skin.assets_invalid",
            "CSS 变量主题的版本、类型、说明或注释无效。",
        ));
    }

    validate_theme_appearance(manifest)?;
    let config = read_theme_css_config(directory, manifest.appearance.as_ref())?;
    let mut expected = HashSet::from([
        "theme.json".to_owned(),
        "theme.css".to_owned(),
        config.preview.clone(),
        config.background.clone(),
    ]);
    expected.extend(config.mode_backgrounds.values().cloned());
    if let Some(appearance) = &manifest.appearance {
        for mode in &appearance.supported_color_modes {
            expected.insert(format!("theme.{}.css", mode.as_str()));
        }
    }
    let images = std::iter::once(&config.preview)
        .chain(std::iter::once(&config.background))
        .chain(config.mode_backgrounds.values())
        .collect::<HashSet<_>>();
    for image in images {
        validate_theme_image_name(image)?;
        validate_resource_file(directory, image, MAX_IMAGE_BYTES)?;
        validate_image_signature(&directory.join(image), image)?;
    }

    let entries = std::fs::read_dir(directory)
        .map_err(|_| AppError::new("skin.assets_invalid", "无法读取 CSS 变量主题目录。"))?;
    let mut actual = HashSet::new();
    for entry in entries {
        let entry = entry
            .map_err(|_| AppError::new("skin.assets_invalid", "无法读取 CSS 变量主题目录。"))?;
        let file_type = entry
            .file_type()
            .map_err(|_| AppError::new("skin.assets_invalid", "无法检查 CSS 变量主题资源类型。"))?;
        let name = entry.file_name().into_string().map_err(|_| {
            AppError::new("skin.assets_invalid", "CSS 变量主题文件名必须是 UTF-8。")
        })?;
        if !file_type.is_file() || file_type.is_symlink() {
            return Err(AppError::new(
                "skin.assets_invalid",
                "CSS 变量主题只能包含清单、变量表和根目录图片。",
            ));
        }
        actual.insert(name);
    }
    if actual != expected {
        let mut missing = expected.difference(&actual).cloned().collect::<Vec<_>>();
        let mut extras = actual.difference(&expected).cloned().collect::<Vec<_>>();
        missing.sort();
        extras.sort();
        let mut details = missing
            .into_iter()
            .map(|file| format!("缺少{}。", resource_display_name(&file)))
            .collect::<Vec<_>>();
        details.extend(
            extras
                .into_iter()
                .map(|file| format!("发现未使用的文件「{file}」，请从主题包中移除。")),
        );
        return Err(AppError::with_details(
            "skin.assets_invalid",
            "这个新版主题包含缺失或未使用的文件。",
            details,
        ));
    }
    Ok(())
}

/// 执行换皮宿主内部的 `validate_theme_appearance` 步骤。
fn validate_theme_appearance(manifest: &PureThemeCssManifest) -> Result<(), AppError> {
    let Some(appearance) = &manifest.appearance else {
        return Ok(());
    };
    if validate_supported_color_modes(&appearance.supported_color_modes).is_err() {
        return Err(AppError::new(
            "skin.assets_invalid",
            "appearance.supportedColorModes 必须是不重复且非空的 light、dark 列表。",
        ));
    }
    if let Some(requirements) = &appearance.requirements {
        for (mode, requirement) in [
            (ColorMode::Light, requirements.light.as_ref()),
            (ColorMode::Dark, requirements.dark.as_ref()),
        ] {
            if let Some(requirement) = requirement {
                if !appearance.supported_color_modes.contains(&mode) {
                    return Err(AppError::new(
                        "skin.assets_invalid",
                        format!(
                            "appearance.requirements.{} 只能用于已声明支持的模式。",
                            mode.as_str()
                        ),
                    ));
                }
                validate_appearance_requirement(requirement, mode)?;
            }
        }
    }
    Ok(())
}

/// 执行换皮宿主内部的 `validate_appearance_requirement` 步骤。
fn validate_appearance_requirement(
    requirement: &AppearanceRequirement,
    mode: ColorMode,
) -> Result<(), AppError> {
    for (field, value) in [
        ("codeThemeId", requirement.code_theme_id.as_ref()),
        ("uiFont", requirement.ui_font.as_ref()),
        ("codeFont", requirement.code_font.as_ref()),
    ] {
        if value.is_some_and(|value| value.trim().is_empty() || value.chars().count() > 160) {
            return Err(AppError::new(
                "skin.assets_invalid",
                format!("appearance.requirements.{}.{field} 无效。", mode.as_str()),
            ));
        }
    }
    let mut colors = vec![
        ("accent", requirement.accent.as_ref()),
        ("surface", requirement.surface.as_ref()),
        ("ink", requirement.ink.as_ref()),
    ];
    if let Some(semantic) = &requirement.semantic_colors {
        colors.extend([
            ("semanticColors.diffAdded", semantic.diff_added.as_ref()),
            ("semanticColors.diffRemoved", semantic.diff_removed.as_ref()),
            ("semanticColors.skill", semantic.skill.as_ref()),
        ]);
    }
    if let Some((field, _)) = colors
        .into_iter()
        .find(|(_, value)| value.is_some_and(|value| !is_valid_requirement_color(value)))
    {
        return Err(AppError::new(
            "skin.assets_invalid",
            format!(
                "appearance.requirements.{}.{field} 必须为 #RRGGBB。",
                mode.as_str()
            ),
        ));
    }
    if requirement.contrast.is_some_and(|value| value > 100) {
        return Err(AppError::new(
            "skin.assets_invalid",
            format!(
                "appearance.requirements.{}.contrast 必须为 0 到 100。",
                mode.as_str()
            ),
        ));
    }
    Ok(())
}

/// 执行换皮宿主内部的 `is_valid_requirement_color` 步骤。
fn is_valid_requirement_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..]
            .chars()
            .all(|character| character.is_ascii_hexdigit())
}

/// 执行换皮宿主内部的 `read_theme_css_config` 步骤。
fn read_theme_css_config(
    directory: &Path,
    appearance: Option<&ThemeAppearance>,
) -> Result<ThemeCssConfig, AppError> {
    validate_resource_file(directory, "theme.css", MAX_THEME_CSS_BYTES)?;
    let css = std::fs::read_to_string(directory.join("theme.css"))
        .map_err(|_| AppError::new("skin.assets_invalid", "主题变量表不是有效 UTF-8 文本。"))?;
    let mut config = parse_theme_css(&css, appearance.is_some())?;
    if let Some(appearance) = appearance {
        for mode in &appearance.supported_color_modes {
            let file = format!("theme.{}.css", mode.as_str());
            validate_resource_file(directory, &file, MAX_THEME_CSS_BYTES)?;
            let overlay = std::fs::read_to_string(directory.join(&file)).map_err(|_| {
                AppError::new(
                    "skin.assets_invalid",
                    format!("{file} 不是有效 UTF-8 文本。"),
                )
            })?;
            if let Some(background) = parse_mode_theme_css(&overlay, *mode, &file)? {
                config.mode_backgrounds.insert(*mode, background);
            }
            config.css_text.push('\n');
            config.css_text.push_str(&overlay);
        }
    }
    Ok(config)
}

/// 执行换皮宿主内部的 `parse_theme_css` 步骤。
fn parse_theme_css(css: &str, explicit_appearance: bool) -> Result<ThemeCssConfig, AppError> {
    const ROOT: &str = "html.codex-dream-skin";
    const DARK: &str = "html.codex-dream-skin[data-dream-shell=\"dark\"]";
    const LIGHT: &str = "html.codex-dream-skin[data-dream-shell=\"light\"]";
    const PALETTE: [&str; 8] = [
        "--skin-bg",
        "--skin-panel",
        "--skin-panel-alt",
        "--skin-accent",
        "--skin-accent-alt",
        "--skin-text",
        "--skin-muted",
        "--skin-line",
    ];
    const ROOT_PROPERTIES: [&str; 15] = [
        "--skin-preview-image",
        "--skin-background-image",
        "--skin-background-size",
        "--skin-background-position-x",
        "--skin-background-position-y",
        "--skin-radius",
        "--skin-blur",
        "--skin-sidebar-opacity",
        "--skin-main-opacity",
        "--skin-header-opacity",
        "--skin-composer-opacity",
        "--skin-card-opacity",
        "--skin-border-width",
        "--skin-shadow-opacity",
        "--skin-texture-opacity",
    ];

    let stripped = strip_css_comments(css)?;
    let mut blocks: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut rest = stripped.as_str();
    while !rest.trim_start().is_empty() {
        rest = rest.trim_start();
        let open = rest.find('{').ok_or_else(theme_css_syntax_error)?;
        let selector = rest[..open].trim();
        if !matches!(selector, ROOT | DARK | LIGHT) {
            return Err(theme_css_syntax_error());
        }
        let after_open = &rest[open + 1..];
        let close = after_open.find('}').ok_or_else(theme_css_syntax_error)?;
        let body = &after_open[..close];
        if body.contains('{') || blocks.contains_key(selector) {
            return Err(theme_css_syntax_error());
        }
        let mut declarations = HashMap::new();
        for raw in body.split(';') {
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            let (property, value) = raw.split_once(':').ok_or_else(theme_css_syntax_error)?;
            let property = property.trim();
            let value = value.trim();
            if !property.starts_with("--skin-")
                || value.is_empty()
                || value.contains("!important")
                || declarations
                    .insert(property.to_owned(), value.to_owned())
                    .is_some()
            {
                return Err(theme_css_syntax_error());
            }
        }
        blocks.insert(selector.to_owned(), declarations);
        rest = &after_open[close + 1..];
    }
    if blocks.len() != if explicit_appearance { 1 } else { 3 } {
        return Err(theme_css_syntax_error());
    }

    let root = blocks.get(ROOT).ok_or_else(theme_css_syntax_error)?;
    let expected_root_count =
        ROOT_PROPERTIES.len() + usize::from(explicit_appearance) * PALETTE.len();
    if root.len() != expected_root_count
        || ROOT_PROPERTIES
            .iter()
            .any(|property| !root.contains_key(*property))
        || (explicit_appearance
            && PALETTE.iter().any(|property| {
                root.get(*property)
                    .is_none_or(|value| !is_valid_theme_color(value))
            }))
    {
        return Err(theme_css_syntax_error());
    }
    let preview = parse_local_css_image(root.get("--skin-preview-image").unwrap())?;
    let background = parse_local_css_image(root.get("--skin-background-image").unwrap())?;
    if !matches!(root["--skin-background-size"].as_str(), "cover" | "contain")
        || !is_css_percent(&root["--skin-background-position-x"], 0, 100)
        || !is_css_percent(&root["--skin-background-position-y"], 0, 100)
        || !is_css_px(&root["--skin-radius"], 0, 32)
        || !is_css_px(&root["--skin-blur"], 0, 40)
        || !is_css_px(&root["--skin-border-width"], 0, 3)
    {
        return Err(theme_css_value_error());
    }
    for property in [
        "--skin-sidebar-opacity",
        "--skin-main-opacity",
        "--skin-header-opacity",
        "--skin-composer-opacity",
        "--skin-card-opacity",
        "--skin-shadow-opacity",
        "--skin-texture-opacity",
    ] {
        if !is_css_percent(&root[property], 0, 100) {
            return Err(theme_css_value_error());
        }
    }
    if !explicit_appearance {
        for selector in [DARK, LIGHT] {
            let palette = blocks.get(selector).ok_or_else(theme_css_syntax_error)?;
            if palette.len() != PALETTE.len()
                || PALETTE.iter().any(|property| {
                    palette
                        .get(*property)
                        .is_none_or(|value| !is_valid_theme_color(value))
                })
            {
                return Err(theme_css_value_error());
            }
        }
    }
    Ok(ThemeCssConfig {
        preview,
        background,
        mode_backgrounds: HashMap::new(),
        css_text: css.to_owned(),
    })
}

/// 执行换皮宿主内部的 `parse_mode_theme_css` 步骤。
fn parse_mode_theme_css(
    css: &str,
    mode: ColorMode,
    file: &str,
) -> Result<Option<String>, AppError> {
    let expected_selector = format!(
        "html.codex-dream-skin[data-dream-shell=\"{}\"]",
        mode.as_str()
    );
    let stripped = strip_css_comments(css)?;
    let source = stripped.trim();
    let open = source
        .find('{')
        .ok_or_else(|| mode_css_error(file, "缺少声明块。"))?;
    let close = source
        .rfind('}')
        .ok_or_else(|| mode_css_error(file, "缺少结束花括号。"))?;
    if source[..open].trim() != expected_selector
        || close <= open
        || !source[close + 1..].trim().is_empty()
        || source[open + 1..close].contains('{')
        || source[open + 1..close].contains('}')
    {
        return Err(mode_css_error(file, "只能使用对应颜色模式的单一根选择器。"));
    }
    let mut seen = HashSet::new();
    let mut background = None;
    for raw in source[open + 1..close].split(';') {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let (property, value) = raw
            .split_once(':')
            .ok_or_else(|| mode_css_error(file, "包含无效声明。"))?;
        let property = property.trim();
        let value = value.trim();
        if !seen.insert(property.to_owned()) || !is_allowed_mode_property(property) {
            return Err(mode_css_error(
                file,
                &format!("变量 {property} 不允许、重复或值超出范围。"),
            ));
        }
        if property == "--skin-background-image" {
            background = Some(parse_local_css_image(value).map_err(|_| {
                mode_css_error(file, "背景图片必须引用主题根目录中的 PNG/JPEG 安全文件名。")
            })?);
        } else if !is_valid_mode_property_value(property, value) {
            return Err(mode_css_error(
                file,
                &format!("变量 {property} 不允许、重复或值超出范围。"),
            ));
        }
    }
    if seen.is_empty() {
        return Err(mode_css_error(file, "至少需要声明一个增量变量。"));
    }
    Ok(background)
}

/// 执行换皮宿主内部的 `is_allowed_mode_property` 步骤。
fn is_allowed_mode_property(property: &str) -> bool {
    matches!(
        property,
        "--skin-bg"
            | "--skin-panel"
            | "--skin-panel-alt"
            | "--skin-accent"
            | "--skin-accent-alt"
            | "--skin-text"
            | "--skin-muted"
            | "--skin-line"
            | "--skin-background-image"
            | "--skin-background-size"
            | "--skin-background-position-x"
            | "--skin-background-position-y"
            | "--skin-radius"
            | "--skin-blur"
            | "--skin-sidebar-opacity"
            | "--skin-main-opacity"
            | "--skin-header-opacity"
            | "--skin-composer-opacity"
            | "--skin-card-opacity"
            | "--skin-border-width"
            | "--skin-shadow-opacity"
            | "--skin-texture-opacity"
    )
}

/// 执行换皮宿主内部的 `is_valid_mode_property_value` 步骤。
fn is_valid_mode_property_value(property: &str, value: &str) -> bool {
    match property {
        "--skin-bg" | "--skin-panel" | "--skin-panel-alt" | "--skin-accent"
        | "--skin-accent-alt" | "--skin-text" | "--skin-muted" | "--skin-line" => {
            is_valid_theme_color(value)
        }
        "--skin-background-size" => matches!(value, "cover" | "contain"),
        "--skin-background-position-x" | "--skin-background-position-y" => {
            is_css_percent(value, 0, 100)
        }
        "--skin-radius" => is_css_px(value, 0, 32),
        "--skin-blur" => is_css_px(value, 0, 40),
        "--skin-border-width" => is_css_px(value, 0, 3),
        _ => is_css_percent(value, 0, 100),
    }
}

/// 执行换皮宿主内部的 `mode_css_error` 步骤。
fn mode_css_error(file: &str, problem: &str) -> AppError {
    AppError::with_details(
        "skin.assets_invalid",
        format!("{file} 不符合模式 CSS 契约。"),
        vec![problem.to_owned()],
    )
}

/// 执行换皮宿主内部的 `strip_css_comments` 步骤。
fn strip_css_comments(css: &str) -> Result<String, AppError> {
    let mut result = String::with_capacity(css.len());
    let mut chars = css.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut closed = false;
            while let Some(comment) = chars.next() {
                if comment == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    closed = true;
                    break;
                }
            }
            if !closed {
                return Err(theme_css_syntax_error());
            }
            result.push(' ');
        } else {
            result.push(ch);
        }
    }
    Ok(result)
}

/// 执行换皮宿主内部的 `parse_local_css_image` 步骤。
fn parse_local_css_image(value: &str) -> Result<String, AppError> {
    let inner = value
        .strip_prefix("url(\"")
        .and_then(|value| value.strip_suffix("\")"))
        .or_else(|| {
            value
                .strip_prefix("url('")
                .and_then(|value| value.strip_suffix("')"))
        })
        .ok_or_else(theme_css_value_error)?;
    validate_theme_image_name(inner)?;
    Ok(inner.to_owned())
}

/// 执行换皮宿主内部的 `is_css_px` 步骤。
fn is_css_px(value: &str, min: u16, max: u16) -> bool {
    value
        .strip_suffix("px")
        .and_then(|value| value.parse::<u16>().ok())
        .is_some_and(|value| (min..=max).contains(&value))
}

/// 执行换皮宿主内部的 `is_css_percent` 步骤。
fn is_css_percent(value: &str, min: u16, max: u16) -> bool {
    value
        .strip_suffix('%')
        .and_then(|value| value.parse::<u16>().ok())
        .is_some_and(|value| (min..=max).contains(&value))
}

/// 执行换皮宿主内部的 `theme_css_syntax_error` 步骤。
fn theme_css_syntax_error() -> AppError {
    AppError::new(
        "skin.assets_invalid",
        "theme.css 只能包含规定的三个主题选择器和固定 CSS 变量，且每项只能声明一次。",
    )
}

/// 执行换皮宿主内部的 `theme_css_value_error` 步骤。
fn theme_css_value_error() -> AppError {
    AppError::new(
        "skin.assets_invalid",
        "theme.css 包含无效的颜色、图片、尺寸、位置或透明度值。",
    )
}

/// 执行换皮宿主内部的 `validate_resource_file` 步骤。
fn validate_resource_file(directory: &Path, file: &str, limit: u64) -> Result<(), AppError> {
    if let Some(problem) = resource_file_problem(directory, file, limit) {
        return Err(import_resource_error(vec![problem]));
    }
    Ok(())
}

/// 执行换皮宿主内部的 `resource_display_name` 步骤。
fn resource_display_name(file: &str) -> String {
    let purpose = match file {
        "theme.json" => "皮肤说明文件",
        "theme.css" => "主题变量文件",
        "dream-skin.css" => "界面样式文件",
        "renderer-inject.js" => "皮肤加载脚本",
        "qq2007-sky.png" => "主预览图片",
        "avatar.png" => "头像图片",
        "qqshow.jpg" => "辅助展示图片",
        _ => "图片资源",
    };
    format!("{purpose}「{file}」")
}

/// 执行换皮宿主内部的 `resource_file_problem` 步骤。
fn resource_file_problem(directory: &Path, file: &str, limit: u64) -> Option<String> {
    let display_name = resource_display_name(file);
    match std::fs::metadata(directory.join(file)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Some(format!("缺少{display_name}。"))
        }
        Err(_) => Some(format!("无法读取{display_name}，请确认文件没有被占用。")),
        Ok(metadata) if !metadata.is_file() => Some(format!("{display_name}必须是普通文件。")),
        Ok(metadata) if metadata.len() == 0 => Some(format!("{display_name}是空文件。")),
        Ok(metadata) if metadata.len() > limit => Some(format!("{display_name}文件过大。")),
        Ok(_) => None,
    }
}

/// 执行换皮宿主内部的 `legacy_resource_problems` 步骤。
fn legacy_resource_problems(directory: &Path, files: &[&str]) -> Vec<String> {
    files
        .iter()
        .filter_map(|file| {
            resource_file_problem(
                directory,
                file,
                if is_image_file(file) {
                    MAX_IMAGE_BYTES
                } else {
                    MAX_TEXT_BYTES
                },
            )
        })
        .collect()
}

/// 执行换皮宿主内部的 `import_resource_error` 步骤。
fn import_resource_error(details: Vec<String>) -> AppError {
    AppError::with_details(
        "skin.assets_invalid",
        "这个皮肤包的文件还不完整，请补齐后重新导入。",
        details,
    )
}

/// 执行换皮宿主内部的 `is_valid_theme_color` 步骤。
fn is_valid_theme_color(value: &str) -> bool {
    matches!(value.len(), 7 | 9)
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// 执行换皮宿主内部的 `validate_theme_image_name` 步骤。
fn validate_theme_image_name(value: &str) -> Result<(), AppError> {
    if validate_core_image_name(value).is_ok() && image_mime(value).is_ok() {
        Ok(())
    } else {
        Err(AppError::new(
            "skin.assets_invalid",
            "纯主题图片必须使用根目录中的 PNG 或 JPEG 安全文件名。",
        ))
    }
}

/// 执行换皮宿主内部的 `is_image_file` 步骤。
fn is_image_file(file_name: &str) -> bool {
    image_mime(file_name).is_ok()
}

/// 执行换皮宿主内部的 `validate_image_signature` 步骤。
fn validate_image_signature(path: &Path, file_name: &str) -> Result<(), AppError> {
    let bytes = std::fs::read(path)
        .map_err(|_| AppError::new("skin.assets_invalid", "无法读取纯主题图片。"))?;
    let expected = match image_mime(file_name)? {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        _ => {
            return Err(AppError::new(
                "skin.assets_invalid",
                "纯主题图片内容与声明格式不一致。",
            ));
        }
    };
    let valid = ImageFormat::from_bytes(&bytes) == Some(expected);
    if valid {
        Ok(())
    } else {
        Err(AppError::new(
            "skin.assets_invalid",
            "纯主题图片内容与声明格式不一致。",
        ))
    }
}

/// 执行换皮宿主内部的 `image_mime` 步骤。
fn image_mime(file_name: &str) -> Result<&'static str, AppError> {
    match Path::new(file_name)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => Ok("image/png"),
        Some("jpg" | "jpeg") => Ok("image/jpeg"),
        _ => Err(AppError::new(
            "skin.assets_invalid",
            "皮肤预览图片格式不受支持。",
        )),
    }
}

/// 执行换皮宿主内部的 `validate_theme_creator_text` 步骤。
fn validate_theme_creator_text(value: &str, label: &str) -> Result<String, AppError> {
    loki_metis_core::normalize_skin_creator_text(value).map_err(|_| {
        AppError::new(
            "skin.create_invalid",
            format!("{label}必须是 1 到 80 个字符。"),
        )
    })
}

/// 执行换皮宿主内部的 `available_theme_id` 步骤。
fn available_theme_id(user_root: &Path) -> Result<String, AppError> {
    for _ in 0..16 {
        let candidate = format!("theme_{}", uuid::Uuid::now_v7());
        if !user_root.join(&candidate).exists() {
            return Ok(candidate);
        }
    }
    Err(AppError::new(
        "skin.create_failed",
        "无法生成唯一的主题标识，请重试。",
    ))
}

/// 执行换皮宿主内部的 `write_theme_scaffold` 步骤。
fn write_theme_scaffold(
    directory: &Path,
    id: &str,
    name: &str,
    author: &str,
) -> Result<(), AppError> {
    let mut manifest: PureThemeCssManifest = serde_json::from_str(THEME_TEMPLATE_MANIFEST)
        .map_err(|_| AppError::new("skin.create_failed", "应用内主题模板清单无效。"))?;
    manifest.id = id.to_owned();
    manifest.name = name.to_owned();
    manifest.description = format!("{name} 的纯主题，可在主题目录中继续编辑。");
    manifest.author = author.to_owned();
    let manifest = serde_json::to_vec_pretty(&manifest)
        .map_err(|_| AppError::new("skin.create_failed", "无法生成新主题清单。"))?;
    std::fs::write(directory.join("theme.json"), manifest)
        .map_err(|_| AppError::new("skin.create_failed", "无法写入新主题清单。"))?;
    std::fs::write(directory.join("theme.css"), THEME_TEMPLATE_CSS)
        .map_err(|_| AppError::new("skin.create_failed", "无法写入新主题变量表。"))?;
    for (file_name, bytes) in THEME_TEMPLATE_IMAGES {
        std::fs::write(directory.join(file_name), bytes)
            .map_err(|_| AppError::new("skin.create_failed", "无法写入新主题占位图片。"))?;
    }
    Ok(())
}
