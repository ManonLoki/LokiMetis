//! 旧版皮肤到纯主题的配色迁移与写出。

use super::*;

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

/// 由旧版皮肤 ID 派生转换后主题的 ID，追加 `-theme` 并保持 64 字符上限。
pub(super) fn converted_theme_id(base_id: &str) -> String {
    let suffix = "-theme";
    let stem_length = 64_usize.saturating_sub(suffix.len());
    format!("{}{suffix}", &base_id[..base_id.len().min(stem_length)])
}

/// 由旧版皮肤名称派生转换后主题的名称，追加“（主题版）”并保持 80 字符上限。
pub(super) fn converted_theme_name(base_name: &str) -> String {
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
pub(super) fn normalize_theme_color(value: &str) -> Option<String> {
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

/// 取旧版配色中指定模式的调色板；未按 light/dark 分组的旧配色视为单一调色板。
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

/// 取一行 CSS 声明中以 `--skin-` 开头的变量名。
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

/// 在暂存目录写出由旧版皮肤转换的纯主题：模板清单、迁移后的双模式变量与原图片，返回需要回退默认值的配色角色说明。
pub(super) fn write_converted_theme(
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
