#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
/// 定义换皮宿主 `LegacyManifest` 使用的内部数据。
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
/// 定义换皮宿主 `FriendCards` 使用的内部数据。
struct FriendCards {
    profile_image: String,
    list_image: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// 定义换皮宿主 `PureThemeCssManifest` 使用的内部数据。
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
/// 定义换皮宿主 `ThemeAppearance` 使用的内部数据。
struct ThemeAppearance {
    supported_color_modes: Vec<ColorMode>,
    #[serde(default)]
    requirements: Option<AppearanceRequirements>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// 定义换皮宿主 `AppearanceRequirements` 使用的内部数据。
struct AppearanceRequirements {
    #[serde(default)]
    light: Option<AppearanceRequirement>,
    #[serde(default)]
    dark: Option<AppearanceRequirement>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// 定义换皮宿主 `AppearanceRequirement` 使用的内部数据。
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
    /// 执行换皮宿主内部的 `has_fields` 步骤。
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
    /// 执行换皮宿主内部的 `has_fields` 步骤。
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
/// 定义换皮宿主 `SemanticColorRequirements` 使用的内部数据。
struct SemanticColorRequirements {
    #[serde(default)]
    diff_added: Option<String>,
    #[serde(default)]
    diff_removed: Option<String>,
    #[serde(default)]
    skill: Option<String>,
}

#[derive(Debug)]
/// 定义换皮宿主 `ThemeCssConfig` 使用的内部数据。
struct ThemeCssConfig {
    preview: String,
    background: String,
    mode_backgrounds: HashMap<ColorMode, String>,
    css_text: String,
}

/// 定义换皮宿主 `SkinManifest` 使用的内部数据。
enum SkinManifest {
    Legacy(LegacyManifest),
    ThemeCss(Box<PureThemeCssManifest>),
}

impl SkinManifest {
    /// 执行换皮宿主内部的 `id` 步骤。
    fn id(&self) -> &str {
        match self {
            Self::Legacy(manifest) => &manifest.id,
            Self::ThemeCss(manifest) => &manifest.id,
        }
    }

    /// 执行换皮宿主内部的 `name` 步骤。
    fn name(&self) -> &str {
        match self {
            Self::Legacy(manifest) => &manifest.name,
            Self::ThemeCss(manifest) => &manifest.name,
        }
    }

    /// 执行换皮宿主内部的 `author` 步骤。
    fn author(&self) -> &str {
        match self {
            Self::Legacy(manifest) => &manifest.author,
            Self::ThemeCss(manifest) => &manifest.author,
        }
    }

    /// 执行换皮宿主内部的 `package_type` 步骤。
    fn package_type(&self) -> SkinPackageType {
        match self {
            Self::Legacy(_) => SkinPackageType::LegacySkin,
            Self::ThemeCss(_) => SkinPackageType::Theme,
        }
    }

    /// 执行换皮宿主内部的 `supported_color_modes` 步骤。
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

/// 执行换皮宿主内部的 `legacy_has_complete_dual_palettes` 步骤。
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
/// 定义换皮宿主 `AppearanceProbe` 使用的内部数据。
struct AppearanceProbe {
    effective_mode: ColorMode,
    #[serde(default)]
    effective: serde_json::Value,
    appearance_readable: bool,
    #[serde(default)]
    appearance: serde_json::Value,
}

#[derive(Debug, Deserialize)]
/// 定义换皮宿主 `PageProbe` 使用的内部数据。
struct PageProbe {
    codex: bool,
    url: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
/// 定义换皮宿主 `CompatibilityPageReport` 使用的内部数据。
struct CompatibilityPageReport {
    version: String,
    #[serde(default)]
    applied_rules: Vec<String>,
    #[serde(default)]
    skipped_rules: Vec<String>,
}

impl PageProbe {
    /// 执行换皮宿主内部的 `is_verified_codex` 步骤。
    fn is_verified_codex(&self) -> bool {
        self.codex && self.url == "app://-/index.html"
    }
}

#[derive(Clone, Copy)]
/// 定义换皮宿主 `ConnectionSource` 使用的内部数据。
enum ConnectionSource {
    Existing,
    Launched,
}

impl ConnectionSource {
    /// 执行换皮宿主内部的 `page_ready_timeout` 步骤。
    fn page_ready_timeout(self) -> Duration {
        match self {
            Self::Existing => EXISTING_CODEX_PAGE_READY_TIMEOUT,
            Self::Launched => CODEX_PAGE_READY_TIMEOUT,
        }
    }
}

/// 定义换皮宿主 `LoadedSkin` 使用的内部数据。
struct LoadedSkin {
    descriptor: SkinDescriptor,
    payload: Arc<str>,
    appearance_requirements: Option<AppearanceRequirements>,
}

#[derive(Clone)]
/// 定义换皮宿主 `AppearancePolicy` 使用的内部数据。
struct AppearancePolicy {
    supported_color_modes: Vec<ColorMode>,
    requirements: Option<AppearanceRequirements>,
}

/// 定义换皮宿主 `WatchTask` 使用的内部数据。
struct WatchTask {
    cancel: watch::Sender<bool>,
    join: JoinHandle<Result<usize, AppError>>,
    handler_abort: AbortHandle,
    endpoint: CdpEndpoint,
}

/// 定义换皮宿主 `HandlerTaskGuard` 使用的内部数据。
struct HandlerTaskGuard(Option<JoinHandle<()>>);

impl HandlerTaskGuard {
    /// 执行换皮宿主内部的 `new` 步骤。
    fn new(task: JoinHandle<()>) -> Self {
        Self(Some(task))
    }

    /// 执行换皮宿主内部的 `abort` 步骤。
    fn abort(&mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
        }
    }

    /// 执行换皮宿主内部的 `replace` 步骤。
    fn replace(&mut self, task: JoinHandle<()>) {
        self.abort();
        self.0 = Some(task);
    }

    /// 执行换皮宿主内部的 `take` 步骤。
    fn take(&mut self) -> Option<JoinHandle<()>> {
        self.0.take()
    }
}

impl Drop for HandlerTaskGuard {
    /// 执行换皮宿主内部的 `drop` 步骤。
    fn drop(&mut self) {
        self.abort();
    }
}

/// 定义换皮宿主 `CodexOperation` 使用的内部数据。
struct CodexOperation {
    id: u64,
    cancel: watch::Sender<bool>,
}

/// 定义换皮宿主 `CodexOperationGuard` 使用的内部数据。
struct CodexOperationGuard<'a> {
    id: u64,
    active: &'a StdMutex<Option<CodexOperation>>,
}

impl Drop for CodexOperationGuard<'_> {
    /// 执行换皮宿主内部的 `drop` 步骤。
    fn drop(&mut self) {
        let Ok(mut active) = self.active.lock() else {
            return;
        };
        if active
            .as_ref()
            .is_some_and(|operation| operation.id == self.id)
        {
            *active = None;
        }
    }
}

/// 定义换皮宿主 `InstanceRuntime` 使用的内部数据。
struct InstanceRuntime {
    active: Option<SkinDescriptor>,
    task: Option<WatchTask>,
    compatibility: Option<SkinCompatibilityStatus>,
}

#[derive(Default)]
/// 定义换皮宿主 `RuntimeState` 使用的内部数据。
struct RuntimeState {
    instances: HashMap<String, InstanceRuntime>,
    last_target: Option<String>,
}

/// 定义换皮宿主 `PendingImportItem` 使用的内部数据。
struct PendingImportItem {
    item_id: String,
    archive_name: String,
    staging: PathBuf,
    candidate: SkinDescriptor,
}

/// 定义换皮宿主 `PendingImportBatch` 使用的内部数据。
struct PendingImportBatch {
    token: String,
    staging_root: PathBuf,
    items: Vec<PendingImportItem>,
    skipped: Vec<SkippedSkinImport>,
}

/// 定义换皮宿主 `ActiveImportPreparation` 使用的内部数据。
struct ActiveImportPreparation {
    token: String,
    cancelled: Arc<AtomicBool>,
}

#[derive(Clone)]
/// 定义换皮宿主 `CachedCatalog` 使用的内部数据。
struct CachedCatalog {
    fingerprint: Vec<CatalogDirectoryFingerprint>,
    skins: Vec<SkinDescriptor>,
}

#[derive(Clone, PartialEq, Eq)]
/// 定义换皮宿主 `CatalogDirectoryFingerprint` 使用的内部数据。
struct CatalogDirectoryFingerprint {
    source: SkinSource,
    name: OsString,
    modified: Option<SystemTime>,
    children_readable: bool,
    children: Vec<CatalogChildFingerprint>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
/// 定义换皮宿主 `CatalogChildFingerprint` 使用的内部数据。
struct CatalogChildFingerprint {
    name: OsString,
    kind: u8,
    length: u64,
    modified: Option<SystemTime>,
}

/// 定义换皮宿主 `CatalogSkin` 使用的内部数据。
struct CatalogSkin {
    descriptor: SkinDescriptor,
    modified: Option<SystemTime>,
}

#[derive(Default)]
/// 定义换皮宿主 `InjectionReport` 使用的内部数据。
struct InjectionReport {
    verified_pages: usize,
    injected_pages: usize,
    failed_pages: usize,
    compatibility_applied_rules: BTreeSet<String>,
    compatibility_skipped_rules: BTreeSet<String>,
}

impl InjectionReport {
    /// 执行换皮宿主内部的 `include_compatibility` 步骤。
    fn include_compatibility(&mut self, report: CompatibilityPageReport) {
        self.compatibility_applied_rules
            .extend(report.applied_rules);
        self.compatibility_skipped_rules
            .extend(report.skipped_rules);
    }

    /// 执行换皮宿主内部的 `compatibility_status` 步骤。
    fn compatibility_status(&self) -> SkinCompatibilityStatus {
        let mode = if self.compatibility_skipped_rules.is_empty() {
            if self.compatibility_applied_rules.is_empty() {
                SkinCompatibilityMode::Native
            } else {
                SkinCompatibilityMode::Adapted
            }
        } else {
            SkinCompatibilityMode::Partial
        };
        SkinCompatibilityStatus {
            version: HOST_COMPATIBILITY_VERSION.into(),
            mode,
            applied_rules: self.compatibility_applied_rules.iter().cloned().collect(),
            skipped_rules: self.compatibility_skipped_rules.iter().cloned().collect(),
        }
    }
}
