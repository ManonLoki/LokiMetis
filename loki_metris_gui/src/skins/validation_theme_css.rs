// 本文件是 validation.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：逐模式 CSS 与变量取值校验、资源文件检查、图片格式与签名校验，以及新主题 ID 与模板写入。

/// 校验单一颜色模式的 CSS 文件：只允许对应模式的唯一根选择器、白名单变量各声明一次且取值合法；返回其中声明的背景图片文件名。
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

/// 模式 CSS 允许声明的固定变量白名单。
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

/// 按变量类别校验取值：颜色为十六进制，背景尺寸只允许 cover/contain，其余为限定范围的像素或百分比。
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

/// 生成指出具体文件与问题的模式 CSS 契约错误。
fn mode_css_error(file: &str, problem: &str) -> AppError {
    AppError::with_details(
        "skin.assets_invalid",
        format!("{file} 不符合模式 CSS 契约。"),
        vec![problem.to_owned()],
    )
}

/// 去除 `/* */` 注释；注释未闭合时报语法错误。
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

/// 解析 `url("…")` 或 `url('…')` 形式的本地图片引用，并校验文件名安全。
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

/// 值是否为闭区间内的整数像素（`Npx`）。
fn is_css_px(value: &str, min: u16, max: u16) -> bool {
    value
        .strip_suffix("px")
        .and_then(|value| value.parse::<u16>().ok())
        .is_some_and(|value| (min..=max).contains(&value))
}

/// 值是否为闭区间内的整数百分比（`N%`）。
fn is_css_percent(value: &str, min: u16, max: u16) -> bool {
    value
        .strip_suffix('%')
        .and_then(|value| value.parse::<u16>().ok())
        .is_some_and(|value| (min..=max).contains(&value))
}

/// theme.css 结构不符合固定选择器与变量契约时的错误。
fn theme_css_syntax_error() -> AppError {
    AppError::new(
        "skin.assets_invalid",
        "theme.css 只能包含规定的三个主题选择器和固定 CSS 变量，且每项只能声明一次。",
    )
}

/// theme.css 变量取值非法时的错误。
fn theme_css_value_error() -> AppError {
    AppError::new(
        "skin.assets_invalid",
        "theme.css 包含无效的颜色、图片、尺寸、位置或透明度值。",
    )
}

/// 要求皮肤目录中的资源文件存在、是非空普通文件且不超过大小上限。
fn validate_resource_file(directory: &Path, file: &str, limit: u64) -> Result<(), AppError> {
    if let Some(problem) = resource_file_problem(directory, file, limit) {
        return Err(import_resource_error(vec![problem]));
    }
    Ok(())
}

/// 把资源文件名转换为面向用户的用途说明。
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

/// 检查单个资源文件，返回缺失、不可读、非普通文件、空文件或过大的用户可读问题。
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

/// 按图片或文本上限检查旧版皮肤的全部必需资源，汇总所有问题。
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

/// 皮肤包资源不完整的导入错误，携带逐项问题。
fn import_resource_error(details: Vec<String>) -> AppError {
    AppError::with_details(
        "skin.assets_invalid",
        "这个皮肤包的文件还不完整，请补齐后重新导入。",
        details,
    )
}

/// 颜色是否为 `#RRGGBB` 或 `#RRGGBBAA` 十六进制形式。
fn is_valid_theme_color(value: &str) -> bool {
    matches!(value.len(), 7 | 9)
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// 主题图片必须是根目录内的安全文件名且扩展名为 PNG 或 JPEG。
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

/// 文件名扩展名是否为受支持的图片格式。
fn is_image_file(file_name: &str) -> bool {
    image_mime(file_name).is_ok()
}

/// 读取图片字节，确认魔数与扩展名声明的格式一致。
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

/// 按扩展名（不区分大小写）返回图片 MIME，只接受 PNG 与 JPEG。
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

/// 规范化主题名称或作者文本，长度不合法时给出带字段名的错误。
fn validate_theme_creator_text(value: &str, label: &str) -> Result<String, AppError> {
    loki_metris_core::normalize_skin_creator_text(value).map_err(|_| {
        AppError::new(
            "skin.create_invalid",
            format!("{label}必须是 1 到 80 个字符。"),
        )
    })
}

/// 生成用户皮肤库中尚未占用的新主题 ID，有限次重试后失败。
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

/// 在新主题目录写入内置模板：清单（填入 ID、名称、作者）、变量表与占位图片。
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
