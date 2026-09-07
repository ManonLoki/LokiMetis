/// 执行换皮宿主内部的 `open_in_file_manager` 步骤。
fn open_in_file_manager(directory: &Path) -> Result<(), AppError> {
    #[cfg(target_os = "macos")]
    let mut command = Command::new("/usr/bin/open");
    #[cfg(target_os = "windows")]
    let mut command = Command::new("explorer");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = Command::new("xdg-open");
    command
        .arg(directory)
        .spawn()
        .map(|_| ())
        .map_err(|_| AppError::new("skin.open_failed", "无法在文件管理器中打开皮肤目录。"))
}

/// 执行换皮宿主内部的 `build_payload` 步骤。
#[cfg(test)]
fn build_payload(directory: &Path) -> Result<String, AppError> {
    let manifest = read_manifest(directory)?;
    validate_manifest(directory, &manifest)?;
    build_payload_from_manifest(directory, &manifest)
}

/// 与 `build_payload` 相同，但复用调用方已解析并校验过的 manifest，避免重复读取磁盘。
fn build_payload_from_manifest(directory: &Path, manifest: &SkinManifest) -> Result<String, AppError> {
    let theme = read_text(&directory.join("theme.json"))?;
    let (css, theme_css, injector, art, avatar, friends, theme_assets) = match manifest {
        SkinManifest::Legacy(manifest) => (
            read_text(&directory.join("dream-skin.css"))?,
            String::new(),
            read_text(&directory.join("renderer-inject.js"))?,
            image_data_url(&directory.join(&manifest.image), "image/png")?,
            image_data_url(
                &directory.join(&manifest.friend_cards.profile_image),
                "image/png",
            )?,
            image_data_url(
                &directory.join(&manifest.friend_cards.list_image),
                "image/jpeg",
            )?,
            "{}".to_owned(),
        ),
        SkinManifest::ThemeCss(manifest) => {
            let config = read_theme_css_config(directory, manifest.appearance.as_ref())?;
            let mode_backgrounds = config
                .mode_backgrounds
                .iter()
                .map(|(mode, file)| Ok((mode.as_str(), theme_image_data_url(directory, file)?)))
                .collect::<Result<HashMap<_, _>, AppError>>()?;
            let theme_assets = serde_json::to_string(&serde_json::json!({
                "backgrounds": mode_backgrounds,
            }))
            .map_err(|_| AppError::new("skin.assets_invalid", "无法编码主题模式图片资源。"))?;
            (
                THEME_RUNTIME_CSS.to_owned(),
                config.css_text,
                THEME_RUNTIME_SCRIPT.to_owned(),
                theme_image_data_url(directory, &config.background)?,
                String::new(),
                String::new(),
                theme_assets,
            )
        }
    };
    let replacements = [
        ("__DREAM_SKIN_CSS_JSON__", json_string(&css)?),
        ("__DREAM_SKIN_THEME_CSS_JSON__", json_string(&theme_css)?),
        ("__DREAM_SKIN_ART_JSON__", json_string(&art)?),
        ("__DREAM_SKIN_AVATAR_JSON__", json_string(&avatar)?),
        ("__DREAM_SKIN_FRIENDS_JSON__", json_string(&friends)?),
        ("__DREAM_SKIN_THEME_ASSETS_JSON__", theme_assets),
        ("__DREAM_SKIN_THEME_JSON__", theme),
        ("__DREAM_SKIN_VERSION_JSON__", json_string(SKIN_VERSION)?),
    ];
    let payload = replacements
        .into_iter()
        .fold(injector, |value, (key, replacement)| {
            value.replace(key, &replacement)
        });
    if payload.contains("__DREAM_SKIN_") {
        return Err(AppError::new(
            "skin.assets_invalid",
            "皮肤渲染脚本包含未解析的资源占位符。",
        ));
    }
    Ok(payload)
}

/// 执行换皮宿主内部的 `append_runtime_skin_marker` 步骤。
fn append_runtime_skin_marker(
    payload: String,
    descriptor: &SkinDescriptor,
) -> Result<String, AppError> {
    let marker = serde_json::to_string(&serde_json::json!({
        "source": descriptor.source,
        "id": descriptor.id,
        "name": descriptor.name,
        "packageType": descriptor.package_type,
    }))
    .map_err(|_| AppError::new("skin.assets_invalid", "无法编码皮肤运行状态标记。"))?;
    let version = json_string(SKIN_VERSION)?;
    Ok(format!(
        "{payload}\n;(() => {{ const state = window.__CODEX_DREAM_SKIN_STATE__; if (state && state.version === {version}) state.skin = {marker}; }})()"
    ))
}

/// 执行换皮宿主内部的 `theme_image_data_url` 步骤。
fn theme_image_data_url(directory: &Path, file_name: &str) -> Result<String, AppError> {
    image_data_url(&directory.join(file_name), image_mime(file_name)?)
}

/// 执行换皮宿主内部的 `read_text` 步骤。
fn read_text(path: &Path) -> Result<String, AppError> {
    read_text_with_limit(path, MAX_TEXT_BYTES)
}

/// 执行换皮宿主内部的 `read_text_with_limit` 步骤。
fn read_text_with_limit(path: &Path, limit: u64) -> Result<String, AppError> {
    let metadata = std::fs::metadata(path)
        .map_err(|_| AppError::new("skin.assets_invalid", "无法读取皮肤文本资源。"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > limit {
        return Err(AppError::new(
            "skin.assets_invalid",
            "皮肤文本资源大小无效。",
        ));
    }
    std::fs::read_to_string(path)
        .map_err(|_| AppError::new("skin.assets_invalid", "皮肤文本资源不是有效 UTF-8。"))
}

/// 执行换皮宿主内部的 `image_data_url` 步骤。
fn image_data_url(path: &Path, mime: &str) -> Result<String, AppError> {
    let bytes = std::fs::read(path)
        .map_err(|_| AppError::new("skin.assets_invalid", "无法读取皮肤图片资源。"))?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(AppError::new(
            "skin.assets_invalid",
            "皮肤图片资源大小无效。",
        ));
    }
    let format = match mime {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        _ => {
            return Err(AppError::new(
                "skin.assets_invalid",
                "皮肤预览图片格式不受支持。",
            ));
        }
    };
    Ok(gallery_image_data_url(format, &bytes))
}

/// 执行换皮宿主内部的 `json_string` 步骤。
fn json_string(value: &str) -> Result<String, AppError> {
    serde_json::to_string(value)
        .map_err(|_| AppError::new("skin.assets_invalid", "皮肤文本无法序列化。"))
}

/// 执行换皮宿主内部的 `debug_port_from_command_line` 步骤。
fn debug_port_from_command_line(command_line: &str) -> Option<u16> {
    let tokens = command_line.split_whitespace().collect::<Vec<_>>();
    let mut result = None;
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index].trim_matches(['"', '\'']);
        if token == "--remote-debugging-port" {
            result = tokens
                .get(index + 1)
                .and_then(|value| value.trim_matches(['"', '\'']).parse::<u16>().ok())
                .filter(|port| *port > 0);
            index += 2;
            continue;
        }
        if let Some(value) = token.strip_prefix("--remote-debugging-port=") {
            result = value
                .trim_matches(['"', '\''])
                .parse::<u16>()
                .ok()
                .filter(|port| *port > 0);
        }
        index += 1;
    }
    result
}

/// 执行换皮宿主内部的 `command_line_arguments` 步骤。
fn command_line_arguments(command_line: &str) -> Vec<String> {
    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut backslashes = 0usize;
    for character in command_line.chars() {
        if character == '\\' {
            backslashes += 1;
            continue;
        }
        if character == '"' {
            current.extend(std::iter::repeat_n('\\', backslashes / 2));
            if backslashes.is_multiple_of(2) {
                quoted = !quoted;
            } else {
                current.push('"');
            }
            backslashes = 0;
            continue;
        }
        current.extend(std::iter::repeat_n('\\', backslashes));
        backslashes = 0;
        if character.is_whitespace() && !quoted {
            if !current.is_empty() {
                arguments.push(std::mem::take(&mut current));
            }
        } else {
            current.push(character);
        }
    }
    current.extend(std::iter::repeat_n('\\', backslashes));
    if !current.is_empty() {
        arguments.push(current);
    }
    arguments
}

/// 执行换皮宿主内部的 `reusable_process_arguments` 步骤。
fn reusable_process_arguments(command_line: &str) -> Vec<String> {
    let mut values = command_line_arguments(command_line)
        .into_iter()
        .skip(1)
        .peekable();
    let mut arguments = Vec::new();
    while let Some(value) = values.next() {
        if value == "--remote-debugging-port" || value == "--remote-debugging-address" {
            let _ = values.next();
            continue;
        }
        if value.starts_with("--remote-debugging-port=")
            || value.starts_with("--remote-debugging-address=")
        {
            continue;
        }
        arguments.push(value);
    }
    arguments
}

/// 执行换皮宿主内部的 `is_primary_codex_command_line` 步骤。
fn is_primary_codex_command_line(command_line: &str) -> bool {
    !command_line_arguments(command_line)
        .into_iter()
        .skip(1)
        .any(|argument| {
            argument == "--type"
                || argument.starts_with("--type=")
                || argument == "--utility-sub-type"
                || argument.starts_with("--utility-sub-type=")
        })
}

/// 执行换皮宿主内部的 `user_data_profile` 步骤。
fn user_data_profile(arguments: &[String]) -> Option<String> {
    user_data_directory(arguments).and_then(|value| {
        value
            .to_string_lossy()
            .trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
    })
}

/// 执行换皮宿主内部的 `user_data_directory` 步骤。
fn user_data_directory(arguments: &[String]) -> Option<PathBuf> {
    let mut index = 0;
    while index < arguments.len() {
        let value = if arguments[index] == "--user-data-dir" {
            index += 1;
            arguments.get(index).map(String::as_str)
        } else {
            arguments[index].strip_prefix("--user-data-dir=")
        };
        if let Some(value) = value {
            let value = value
                .trim_matches(['"', '\''])
                .trim_end_matches(['/', '\\']);
            return (!value.is_empty()).then(|| PathBuf::from(value));
        }
        index += 1;
    }
    None
}

/// 执行换皮宿主内部的 `stable_instance_id` 步骤。
fn stable_instance_id(process: &PlatformCodexProcess) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in process
        .executable
        .to_string_lossy()
        .bytes()
        .chain(process.command_line.bytes())
    {
        hash ^= u64::from(byte.to_ascii_lowercase());
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("codex-{}-{hash:016x}", process.pid)
}

/// 执行换皮宿主内部的 `resolved_instance` 步骤。
fn resolved_instance(process: PlatformCodexProcess) -> ResolvedCodexInstance {
    let arguments = reusable_process_arguments(&process.command_line);
    ResolvedCodexInstance {
        id: stable_instance_id(&process),
        debug_port: debug_port_from_command_line(&process.command_line),
        profile: user_data_profile(&arguments),
        process,
        arguments,
    }
}
