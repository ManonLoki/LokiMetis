// 本文件收纳皮肤包序列化清单、外观契约与宿主页面探针的数据契约。
// （本文件由主文件 include! 并入，与主文件共用同一个模块作用域。）

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// 旧版皮肤包清单：好友卡片图片与可选配色，作为需要显式信任的可执行皮肤加载。
struct LegacyManifest {
    schema_version: u64,
    id: String,
    name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    description: String,
    #[serde(default = "unknown_author")]
    author: String,
    image: String,
    friend_cards: FriendCards,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    colors: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// 旧版皮肤的好友卡片图片，分别用于资料页与列表。
struct FriendCards {
    profile_image: String,
    list_image: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// 纯主题 CSS 皮肤包清单：只含样式与外观要求，不携带可执行脚本。
struct PureThemeCssManifest {
    #[serde(rename = "$comment", default)]
    _comment: Option<String>,
    schema_version: u64,
    #[serde(rename = "type")]
    package_type: String,
    id: String,
    name: String,
    description: String,
    author: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    appearance: Option<ThemeAppearance>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// 主题声明的外观契约：支持的明暗模式及可选的逐模式宿主设置要求。
struct ThemeAppearance {
    supported_color_modes: Vec<ColorMode>,
    #[serde(default)]
    requirements: Option<AppearanceRequirements>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// 按浅色、深色分别声明的宿主外观要求。
struct AppearanceRequirements {
    #[serde(default)]
    light: Option<AppearanceRequirement>,
    #[serde(default)]
    dark: Option<AppearanceRequirement>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// 单一明暗模式下皮肤期望的宿主外观设置，字段缺失表示不作要求。
struct AppearanceRequirement {
    #[serde(default)]
    code_theme_id: Option<String>,
    #[serde(default)]
    accent: Option<String>,
    #[serde(default)]
    surface: Option<String>,
    #[serde(default)]
    ink: Option<String>,
    #[serde(default)]
    contrast: Option<u8>,
    #[serde(default)]
    opaque_windows: Option<bool>,
    #[serde(default)]
    ui_font: Option<String>,
    #[serde(default)]
    code_font: Option<String>,
    #[serde(default)]
    semantic_colors: Option<SemanticColorRequirements>,
}

impl AppearanceRequirement {
    /// 是否声明了至少一项外观要求。
    fn has_fields(&self) -> bool {
        self.code_theme_id.is_some()
            || self.accent.is_some()
            || self.surface.is_some()
            || self.ink.is_some()
            || self.contrast.is_some()
            || self.opaque_windows.is_some()
            || self.ui_font.is_some()
            || self.code_font.is_some()
            || self.semantic_colors.as_ref().is_some_and(|colors| {
                colors.diff_added.is_some()
                    || colors.diff_removed.is_some()
                    || colors.skill.is_some()
            })
    }
}

impl AppearanceRequirements {
    /// 任一明暗模式声明了外观要求时为真。
    fn has_fields(&self) -> bool {
        self.light
            .as_ref()
            .is_some_and(AppearanceRequirement::has_fields)
            || self
                .dark
                .as_ref()
                .is_some_and(AppearanceRequirement::has_fields)
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// 期望的语义色：差异新增、差异删除与技能标记颜色。
struct SemanticColorRequirements {
    #[serde(default)]
    diff_added: Option<String>,
    #[serde(default)]
    diff_removed: Option<String>,
    #[serde(default)]
    skill: Option<String>,
}

#[derive(Debug)]
/// 主题 CSS 包解析后的资源：预览图、背景图、逐模式背景与样式正文。
struct ThemeCssConfig {
    preview: String,
    background: String,
    mode_backgrounds: HashMap<ColorMode, String>,
    css_text: String,
}

/// 两种受支持的皮肤包清单。
enum SkinManifest {
    Legacy(Box<LegacyManifest>),
    ThemeCss(Box<PureThemeCssManifest>),
}

impl SkinManifest {
    /// 返回清单声明的皮肤 ID。
    fn id(&self) -> &str {
        match self {
            Self::Legacy(manifest) => &manifest.id,
            Self::ThemeCss(manifest) => &manifest.id,
        }
    }

    /// 返回清单声明的皮肤名称。
    fn name(&self) -> &str {
        match self {
            Self::Legacy(manifest) => &manifest.name,
            Self::ThemeCss(manifest) => &manifest.name,
        }
    }

    /// 返回清单声明的作者。
    fn author(&self) -> &str {
        match self {
            Self::Legacy(manifest) => &manifest.author,
            Self::ThemeCss(manifest) => &manifest.author,
        }
    }

    /// 把清单种类映射为对外包类型。
    fn package_type(&self) -> SkinPackageType {
        match self {
            Self::Legacy(_) => SkinPackageType::LegacySkin,
            Self::ThemeCss(_) => SkinPackageType::Theme,
        }
    }

    /// 返回皮肤支持的明暗模式：旧版皮肤只有配色完整时才支持双模式，主题未声明时默认仅浅色。
    fn supported_color_modes(&self) -> Vec<ColorMode> {
        let mut modes = match self {
            Self::Legacy(manifest) if legacy_has_complete_dual_palettes(manifest) => {
                vec![ColorMode::Light, ColorMode::Dark]
            }
            Self::Legacy(_) => vec![ColorMode::Light],
            Self::ThemeCss(manifest) => manifest
                .appearance
                .as_ref()
                .map(|appearance| appearance.supported_color_modes.clone())
                .unwrap_or_else(|| vec![ColorMode::Light]),
        };
        modes.sort_by_key(|mode| match mode {
            ColorMode::Light => 0,
            ColorMode::Dark => 1,
        });
        modes
    }
}

/// 旧版清单是否为浅色与深色都提供了完整的八项配色。
fn legacy_has_complete_dual_palettes(manifest: &LegacyManifest) -> bool {
    const COLOR_KEYS: [&str; 8] = [
        "background",
        "panel",
        "panelAlt",
        "accent",
        "accentAlt",
        "text",
        "muted",
        "line",
    ];

    let Some(colors) = manifest
        .colors
        .as_ref()
        .and_then(serde_json::Value::as_object)
    else {
        return false;
    };
    ["light", "dark"].into_iter().all(|mode| {
        colors
            .get(mode)
            .and_then(serde_json::Value::as_object)
            .is_some_and(|palette| {
                COLOR_KEYS.iter().all(|key| {
                    palette
                        .get(*key)
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|value| !value.trim().is_empty())
                })
            })
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
/// 页面脚本回报的外观探针：实际明暗模式、生效设置及可读取的外观配置。
struct AppearanceProbe {
    effective_mode: ColorMode,
    #[serde(default)]
    effective: serde_json::Value,
    appearance_readable: bool,
    #[serde(default)]
    appearance: serde_json::Value,
}

#[derive(Debug, Deserialize)]
/// 页面身份探针：是否为 Codex 或 WorkBuddy 页面及其 URL。
struct PageProbe {
    #[serde(default)]
    codex: bool,
    #[serde(default, rename = "workBuddy")]
    work_buddy: bool,
    url: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
/// 单个页面注入后回报的兼容规则应用情况。
struct CompatibilityPageReport {
    version: String,
    #[serde(default)]
    applied_rules: Vec<String>,
    #[serde(default)]
    skipped_rules: Vec<String>,
}

impl PageProbe {
    /// 只有 Codex 主界面固定 URL 才算已验证的 Codex 页面。
    fn is_verified_codex(&self) -> bool {
        self.codex && self.url == "app://-/index.html"
    }

    /// WorkBuddy 只接受其本机 `file:` 渲染页面与稳定标题、根节点组合。
    fn is_verified_workbuddy(&self) -> bool {
        self.work_buddy && self.url.starts_with("file://")
    }
}

#[derive(Clone, Copy)]
/// CDP 连接来源：连接已在运行的宿主，或由本应用刚启动的宿主。
enum ConnectionSource {
    Existing,
    Launched,
}

impl ConnectionSource {
    /// 等待页面就绪的期限；刚启动的宿主需要更长时间加载。
    fn page_ready_timeout(self) -> Duration {
        match self {
            Self::Existing => EXISTING_CODEX_PAGE_READY_TIMEOUT,
            Self::Launched => CODEX_PAGE_READY_TIMEOUT,
        }
    }
}

/// 已加载待注入的皮肤：展示描述、注入脚本正文与外观要求。
struct LoadedSkin {
    descriptor: SkinDescriptor,
    payload: Arc<str>,
    appearance_requirements: Option<AppearanceRequirements>,
}

#[derive(Clone)]
/// 安装前预检使用的外观策略：支持的模式与逐模式要求。
struct AppearancePolicy {
    supported_color_modes: Vec<ColorMode>,
    requirements: Option<AppearanceRequirements>,
}
