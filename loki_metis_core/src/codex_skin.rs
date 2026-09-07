//! 定义多宿主换皮在 Tauri、WebView 与操作系统之外仍然成立的领域合同。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 单次导入允许的最大本机压缩包数量。
pub const MAX_SKIN_IMPORT_BATCH_FILES: usize = 100;
/// 单次删除允许的最大用户皮肤数量。
pub const MAX_SKIN_DELETE_BATCH_ITEMS: usize = 1000;
/// 皮肤稳定标识允许的最大 ASCII 字节数。
pub const MAX_SKIN_ID_BYTES: usize = 64;
/// 用户创建主题时名称和作者允许的最大字符数。
pub const MAX_SKIN_CREATOR_TEXT_CHARS: usize = 80;
/// 纯主题说明允许的最大 Unicode 字符数。
pub const MAX_THEME_DESCRIPTION_CHARS: usize = 500;
/// 纯主题可选注释允许的最大 Unicode 字符数。
pub const MAX_THEME_COMMENT_CHARS: usize = 2000;
/// 单份纯主题变量表允许的最大字节数。
pub const MAX_THEME_CSS_BYTES: u64 = 64 * 1024;
/// 单个主题图片允许的最大字节数。
pub const MAX_THEME_IMAGE_BYTES: u64 = 16 * 1024 * 1024;

/// 区分当前产品允许应用本机皮肤的桌面宿主。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum SkinHostKind {
    /// OpenAI Codex 桌面应用。
    Codex,
    /// 腾讯 WorkBuddy 桌面应用。
    WorkBuddy,
}

impl SkinHostKind {
    /// 返回界面与稳定错误共同使用的宿主名称。
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::WorkBuddy => "WorkBuddy",
        }
    }
}

/// 区分只读内置资源与可管理的用户资源。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum SkinSource {
    /// 随 LokiMetis 打包且不可原地修改的资源。
    Builtin,
    /// 位于 LokiMetis 应用数据目录、可由用户管理的资源。
    User,
}

/// 区分历史自由注入皮肤与受限变量纯主题。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SkinPackageType {
    /// 包含兼容 CSS 与运行脚本的历史包。
    LegacySkin,
    /// 只包含受限主题变量和本地图片的纯主题。
    Theme,
}

/// 表示主题可应用的 Codex 浅色或深色外观模式。
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum ColorMode {
    /// 浅色外观。
    Light,
    /// 深色外观。
    Dark,
}

impl ColorMode {
    /// 返回皮肤清单与 CSS 契约使用的稳定小写值。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

/// 以来源和稳定标识精确引用一个皮肤资源。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct SkinReference {
    /// 资源来自内置只读目录还是用户资源库。
    pub source: SkinSource,
    /// 只含小写 ASCII、数字、连字符或下划线的稳定标识。
    pub id: String,
}

/// 表示平台无关的换皮输入违反了稳定领域规则。
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum SkinRuleError {
    /// 皮肤标识为空、越界或含路径/非法字符。
    #[error("invalid skin id")]
    InvalidId,
    /// 创建者输入为空或超过固定字符上限。
    #[error("invalid creator text")]
    InvalidCreatorText,
    /// 批次为空或超过允许数量。
    #[error("invalid batch size")]
    InvalidBatchSize,
    /// 删除请求尝试修改只读内置皮肤。
    #[error("builtin skin is read only")]
    BuiltinReadOnly,
    /// 同一批次重复引用相同用户皮肤。
    #[error("duplicate skin reference")]
    DuplicateReference,
    /// 纯主题名称、作者、说明或注释违反统一元数据边界。
    #[error("invalid theme metadata")]
    InvalidThemeMetadata,
    /// 外观模式为空、重复或超出固定浅色与深色集合。
    #[error("invalid color modes")]
    InvalidColorModes,
    /// 图片引用不是主题根目录中的 PNG 或 JPEG 安全文件名。
    #[error("invalid theme image name")]
    InvalidThemeImageName,
}

/// 判断字符串是否满足皮肤目录与清单共用的稳定标识语法。
pub fn is_valid_skin_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_SKIN_ID_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
}

/// 校验精确皮肤引用，拒绝任何可解释为路径的标识。
pub fn validate_skin_reference(reference: &SkinReference) -> Result<(), SkinRuleError> {
    is_valid_skin_id(&reference.id)
        .then_some(())
        .ok_or(SkinRuleError::InvalidId)
}

/// 修剪并校验用户创建主题时的名称或作者。
pub fn normalize_skin_creator_text(value: &str) -> Result<String, SkinRuleError> {
    let normalized = value.trim();
    if normalized.is_empty() || normalized.chars().count() > MAX_SKIN_CREATOR_TEXT_CHARS {
        return Err(SkinRuleError::InvalidCreatorText);
    }
    Ok(normalized.to_owned())
}

/// 校验导入批次数量，不接触文件系统或路径。
pub fn validate_skin_import_batch_size(count: usize) -> Result<(), SkinRuleError> {
    (1..=MAX_SKIN_IMPORT_BATCH_FILES)
        .contains(&count)
        .then_some(())
        .ok_or(SkinRuleError::InvalidBatchSize)
}

/// 校验删除批次只含不重复的用户皮肤引用。
pub fn validate_skin_delete_batch(skins: &[SkinReference]) -> Result<(), SkinRuleError> {
    if !(1..=MAX_SKIN_DELETE_BATCH_ITEMS).contains(&skins.len()) {
        return Err(SkinRuleError::InvalidBatchSize);
    }
    let mut ids = HashSet::with_capacity(skins.len());
    for skin in skins {
        validate_skin_reference(skin)?;
        if skin.source != SkinSource::User {
            return Err(SkinRuleError::BuiltinReadOnly);
        }
        if !ids.insert(skin.id.as_str()) {
            return Err(SkinRuleError::DuplicateReference);
        }
    }
    Ok(())
}

/// 校验 v3 纯主题中跨平台一致的身份与文本字段。
pub fn validate_theme_metadata(
    id: &str,
    name: &str,
    author: &str,
    description: &str,
    comment: Option<&str>,
) -> Result<(), SkinRuleError> {
    if !is_valid_skin_id(id)
        || normalize_skin_creator_text(name).is_err()
        || normalize_skin_creator_text(author).is_err()
        || description.trim().is_empty()
        || description.chars().count() > MAX_THEME_DESCRIPTION_CHARS
        || comment.is_some_and(|value| {
            value.trim().is_empty() || value.chars().count() > MAX_THEME_COMMENT_CHARS
        })
    {
        return Err(SkinRuleError::InvalidThemeMetadata);
    }
    Ok(())
}

/// 校验外观模式必须是一个或两个不重复的固定模式。
pub fn validate_supported_color_modes(modes: &[ColorMode]) -> Result<(), SkinRuleError> {
    if modes.is_empty()
        || modes.len() > 2
        || modes.iter().copied().collect::<HashSet<_>>().len() != modes.len()
    {
        return Err(SkinRuleError::InvalidColorModes);
    }
    Ok(())
}

/// 校验纯主题图片只使用根目录安全文件名和 PNG/JPEG 扩展名。
pub fn validate_theme_image_name(value: &str) -> Result<(), SkinRuleError> {
    let lower = value.to_ascii_lowercase();
    let valid_extension =
        lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg");
    let safe = !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains('/')
        && !value.contains('\\')
        && !value.contains(':')
        && !value.chars().any(char::is_control)
        && lower != "theme.json"
        && lower != "theme.css"
        && valid_extension;
    safe.then_some(())
        .ok_or(SkinRuleError::InvalidThemeImageName)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证稳定标识只允许安全的小写 ASCII 目录名。
    #[test]
    fn skin_id_rejects_paths_uppercase_and_bound_overflow() {
        assert!(is_valid_skin_id("blue-dream_2"));
        assert!(!is_valid_skin_id("../blue-dream"));
        assert!(!is_valid_skin_id("BlueDream"));
        assert!(!is_valid_skin_id(&"a".repeat(MAX_SKIN_ID_BYTES + 1)));
    }

    /// 验证创建者输入按 Unicode 字符计数并返回修剪值。
    #[test]
    fn creator_text_is_trimmed_and_unicode_bounded() {
        assert_eq!(
            normalize_skin_creator_text("  雾中花园  ").expect("合法名称应通过"),
            "雾中花园"
        );
        assert_eq!(
            normalize_skin_creator_text(&"界".repeat(MAX_SKIN_CREATOR_TEXT_CHARS + 1)),
            Err(SkinRuleError::InvalidCreatorText)
        );
    }

    /// 验证删除批次拒绝内置资源、重复项与空选择。
    #[test]
    fn delete_batch_requires_unique_user_references() {
        let user = SkinReference {
            source: SkinSource::User,
            id: "forest".to_owned(),
        };
        assert_eq!(
            validate_skin_delete_batch(&[]),
            Err(SkinRuleError::InvalidBatchSize)
        );
        assert_eq!(
            validate_skin_delete_batch(&[SkinReference {
                source: SkinSource::Builtin,
                id: "forest".to_owned(),
            }]),
            Err(SkinRuleError::BuiltinReadOnly)
        );
        assert_eq!(
            validate_skin_delete_batch(&[user.clone(), user]),
            Err(SkinRuleError::DuplicateReference)
        );
    }

    /// 验证纯主题元数据、外观模式与图片文件名由 core 统一拒绝非法输入。
    #[test]
    fn theme_contract_rejects_invalid_portable_values() {
        assert!(validate_theme_metadata("forest", "森林", "Loki", "说明", None).is_ok());
        assert_eq!(
            validate_theme_metadata("Forest", "森林", "Loki", "说明", None),
            Err(SkinRuleError::InvalidThemeMetadata)
        );
        assert_eq!(
            validate_supported_color_modes(&[ColorMode::Dark, ColorMode::Dark]),
            Err(SkinRuleError::InvalidColorModes)
        );
        assert!(validate_theme_image_name("background.png").is_ok());
        assert_eq!(
            validate_theme_image_name("../background.png"),
            Err(SkinRuleError::InvalidThemeImageName)
        );
    }

    /// 两个批准宿主必须保持稳定 camelCase wire 值与用户可见名称。
    #[test]
    fn skin_hosts_have_stable_wire_values_and_names() {
        assert_eq!(
            serde_json::to_string(&SkinHostKind::Codex).unwrap(),
            "\"codex\""
        );
        assert_eq!(
            serde_json::to_string(&SkinHostKind::WorkBuddy).unwrap(),
            "\"workBuddy\""
        );
        assert_eq!(SkinHostKind::Codex.display_name(), "Codex");
        assert_eq!(SkinHostKind::WorkBuddy.display_name(), "WorkBuddy");
    }
}
