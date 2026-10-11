// 本文件是 validation.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：执行换皮宿主内部的 `parse_mode_theme_css` 步骤。；执行换皮宿主内部的 `is_allowed_mode_property` 步骤。；执行换皮宿主内部的 `is_valid_mode_property_value` 步骤。

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
    loki_metris_core::normalize_skin_creator_text(value).map_err(|_| {
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
