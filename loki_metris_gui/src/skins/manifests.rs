// 本文件收纳皮肤包序列化清单、外观契约与宿主页面探针的数据契约。
// （本文件由主文件 include! 并入，与主文件共用同一个模块作用域。）

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// 旧版皮肤包清单：好友卡片图片与可选配色，作为需要显式信任的可执行皮肤加载。
struct LegacyManifest {
    /// 清单格式版本。
    schema_version: u64,
    /// 皮肤 ID。
    id: String,
    /// 展示名。
    name: String,
    /// 说明，可为空。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    description: String,
    /// 作者；缺失时为未知作者。
    #[serde(default = "unknown_author")]
    author: String,
    /// 主图文件名。
    image: String,
    /// 好友卡片图片。
    friend_cards: FriendCards,
    /// 可选配色，原样保留供转换使用。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    colors: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// 旧版皮肤的好友卡片图片，分别用于资料页与列表。
struct FriendCards {
    /// 资料页图片文件名。
    profile_image: String,
    /// 列表图片文件名。
    list_image: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// 纯主题 CSS 皮肤包清单：只含样式与外观要求，不携带可执行脚本。
struct PureThemeCssManifest {
    /// 清单中的 `$comment` 说明，忽略。
    #[serde(rename = "$comment", default)]
    _comment: Option<String>,
    /// 清单格式版本。
    schema_version: u64,
    /// 包类型，必须是主题 CSS。
    #[serde(rename = "type")]
    package_type: String,
    /// 皮肤 ID。
    id: String,
    /// 展示名。
    name: String,
    /// 说明。
    description: String,
    /// 作者。
    author: String,
    /// 外观契约。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    appearance: Option<ThemeAppearance>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// 主题声明的外观契约：支持的明暗模式及可选的逐模式宿主设置要求。
struct ThemeAppearance {
    /// 支持的明暗模式。
    supported_color_modes: Vec<ColorMode>,
    /// 逐模式宿主设置要求。
    #[serde(default)]
    requirements: Option<AppearanceRequirements>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// 按浅色、深色分别声明的宿主外观要求。
struct AppearanceRequirements {
    /// 浅色模式要求。
    #[serde(default)]
    light: Option<AppearanceRequirement>,
    /// 深色模式要求。
    #[serde(default)]
    dark: Option<AppearanceRequirement>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// 单一明暗模式下皮肤期望的宿主外观设置，字段缺失表示不作要求。
struct AppearanceRequirement {
    /// 代码主题 ID。
    #[serde(default)]
    code_theme_id: Option<String>,
    /// 强调色。
    #[serde(default)]
    accent: Option<String>,
    /// 表面色。
    #[serde(default)]
    surface: Option<String>,
    /// 文字色。
    #[serde(default)]
    ink: Option<String>,
    /// 对比度。
    #[serde(default)]
    contrast: Option<u8>,
    /// 是否要求不透明窗口。
    #[serde(default)]
    opaque_windows: Option<bool>,
    /// 界面字体。
    #[serde(default)]
    ui_font: Option<String>,
    /// 代码字体。
    #[serde(default)]
    code_font: Option<String>,
    /// 语义色要求。
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
    /// 差异新增色。
    #[serde(default)]
    diff_added: Option<String>,
    /// 差异删除色。
    #[serde(default)]
    diff_removed: Option<String>,
    /// 技能标记色。
    #[serde(default)]
    skill: Option<String>,
}

#[derive(Debug)]
/// 主题 CSS 包解析后的资源：预览图、背景图、逐模式背景与样式正文。
struct ThemeCssConfig {
    /// 预览图 data URL。
    preview: String,
    /// 默认背景图 data URL。
    background: String,
    /// 逐明暗模式的背景图。
    mode_backgrounds: HashMap<ColorMode, String>,
    /// 样式正文。
    css_text: String,
}

/// 两种受支持的皮肤包清单。
enum SkinManifest {
    /// 旧版可执行皮肤。
    Legacy(Box<LegacyManifest>),
    /// 纯主题 CSS 皮肤。
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
    /// 宿主实际明暗模式。
    effective_mode: ColorMode,
    /// 宿主生效的外观设置。
    #[serde(default)]
    effective: serde_json::Value,
    /// 外观配置可以读取。
    appearance_readable: bool,
    /// 读到的外观配置原文。
    #[serde(default)]
    appearance: serde_json::Value,
}

#[derive(Debug, Deserialize)]
/// 页面身份探针：是否为 Codex 或 WorkBuddy 页面及其 URL。
struct PageProbe {
    /// 是 Codex 页面。
    #[serde(default)]
    codex: bool,
    /// 是 WorkBuddy 页面。
    #[serde(default, rename = "workBuddy")]
    work_buddy: bool,
    /// 页面 URL。
    url: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
/// 单个页面注入后回报的兼容规则应用情况。
struct CompatibilityPageReport {
    /// 兼容脚本版本。
    version: String,
    /// 已应用的规则。
    #[serde(default)]
    applied_rules: Vec<String>,
    /// 无法应用的规则。
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
    /// 连接已在运行的宿主。
    Existing,
    /// 由本应用刚启动的宿主。
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
    /// 展示描述。
    descriptor: SkinDescriptor,
    /// 注入脚本正文。
    payload: Arc<str>,
    /// 外观要求；未声明时为 None。
    appearance_requirements: Option<AppearanceRequirements>,
}

#[derive(Clone)]
/// 安装前预检使用的外观策略：支持的模式与逐模式要求。
struct AppearancePolicy {
    /// 支持的明暗模式。
    supported_color_modes: Vec<ColorMode>,
    /// 逐模式要求。
    requirements: Option<AppearanceRequirements>,
}
