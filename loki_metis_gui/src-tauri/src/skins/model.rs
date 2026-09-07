#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 定义换皮宿主 `SkinDescriptor` 使用的内部数据。
pub struct SkinDescriptor {
    pub id: String,
    pub source: SkinSource,
    pub package_type: SkinPackageType,
    pub name: String,
    pub author: String,
    pub version: String,
    pub preview_data_url: String,
    pub supported_color_modes: Vec<ColorMode>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 定义换皮宿主 `AppearanceDifference` 使用的内部数据。
pub struct AppearanceDifference {
    pub field: String,
    pub label: String,
    pub current_value: Option<String>,
    pub expected_value: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 定义换皮宿主 `SkinAppearanceCheck` 使用的内部数据。
pub struct SkinAppearanceCheck {
    pub effective_mode: ColorMode,
    pub supported_color_modes: Vec<ColorMode>,
    pub differences: Vec<AppearanceDifference>,
    pub unreadable: Vec<AppearanceDifference>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
/// 定义换皮宿主 `InstallSkinResult` 使用的内部数据。
pub enum InstallSkinResult {
    Installed { status: SkinStatus },
    NeedsConfirmation { check: SkinAppearanceCheck },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 定义换皮宿主 `SkinCreationPrompt` 使用的内部数据。
pub struct SkinCreationPrompt {
    pub prompt: String,
}

/// 旧皮肤转换结果。`fallback_roles` 列出无法解析、已回退到模板默认值的配色角色。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThemeConversionResult {
    pub theme: SkinDescriptor,
    pub fallback_roles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 定义换皮宿主 `FailedSkinDelete` 使用的内部数据。
pub struct FailedSkinDelete {
    pub skin: SkinReference,
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 定义换皮宿主 `BatchDeleteResult` 使用的内部数据。
pub struct BatchDeleteResult {
    pub deleted: Vec<SkinReference>,
    pub failed: Vec<FailedSkinDelete>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
/// 定义换皮宿主 `PreparedSkinImportItem` 使用的内部数据。
pub struct PreparedSkinImportItem {
    pub item_id: String,
    pub archive_name: String,
    pub skin: SkinDescriptor,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
/// 定义换皮宿主 `SkippedSkinImport` 使用的内部数据。
pub struct SkippedSkinImport {
    pub archive_name: String,
    pub code: &'static str,
    pub message: String,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
/// 定义换皮宿主 `PreparedSkinImportBatch` 使用的内部数据。
pub struct PreparedSkinImportBatch {
    pub token: String,
    pub items: Vec<PreparedSkinImportItem>,
    pub total_files: usize,
    pub skipped: Vec<SkippedSkinImport>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
/// 定义换皮宿主 `FailedSkinImport` 使用的内部数据。
pub struct FailedSkinImport {
    pub item_id: String,
    pub archive_name: String,
    pub skin_name: String,
    pub code: &'static str,
    pub message: String,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
/// 定义换皮宿主 `BatchImportResult` 使用的内部数据。
pub struct BatchImportResult {
    pub installed: Vec<SkinDescriptor>,
    pub failed: Vec<FailedSkinImport>,
    pub skipped_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
/// 定义换皮宿主 `SkinImportPreparationEvent` 使用的内部数据。
pub enum SkinImportPreparationEvent {
    Started {
        token: String,
        total_files: usize,
    },
    ItemReady {
        token: String,
        item: PreparedSkinImportItem,
    },
    ItemSkipped {
        token: String,
        item: SkippedSkinImport,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 定义换皮宿主 `SkinStatus` 使用的内部数据。
pub struct SkinStatus {
    installed: bool,
    skin_id: Option<String>,
    source: Option<SkinSource>,
    package_type: Option<SkinPackageType>,
    skin_name: Option<String>,
    version: String,
    affected_pages: usize,
    compatibility: Option<SkinCompatibilityStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    instance_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 定义换皮宿主 `SkinCompatibilityStatus` 使用的内部数据。
pub struct SkinCompatibilityStatus {
    version: String,
    mode: SkinCompatibilityMode,
    applied_rules: Vec<String>,
    skipped_rules: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 定义换皮宿主 `SkinCompatibilityMode` 使用的内部数据。
pub enum SkinCompatibilityMode {
    Native,
    Adapted,
    Partial,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 定义换皮宿主 `CodexRuntimeState` 使用的内部数据。
pub enum CodexRuntimeState {
    Stopped,
    Ready,
    RunningWithoutCdp,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 定义换皮宿主 `CodexRuntimeStatus` 使用的内部数据。
pub struct CodexRuntimeStatus {
    state: CodexRuntimeState,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 定义换皮宿主 `CodexInstance` 使用的内部数据。
pub struct CodexInstance {
    id: String,
    pid: u32,
    label: String,
    profile: Option<String>,
    state: CodexRuntimeState,
    debug_port: Option<u16>,
    active_skin_name: Option<String>,
    active_skin: Option<SkinReference>,
    account_label: Option<String>,
    avatar_data_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
/// 定义换皮宿主 `AccountProfileProbe` 使用的内部数据。
struct AccountProfileProbe {
    label: Option<String>,
    avatar_data_url: Option<String>,
    active_skin: Option<ActiveSkinProbe>,
}

#[derive(Debug, Clone)]
/// 定义换皮宿主 `AccountProfile` 使用的内部数据。
struct AccountProfile {
    label: Option<String>,
    avatar_data_url: Option<String>,
    active_skin: Option<RecoveredSkinIdentity>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
/// 定义换皮宿主 `ActiveSkinProbe` 使用的内部数据。
struct ActiveSkinProbe {
    version: Option<String>,
    source: Option<SkinSource>,
    id: Option<String>,
    legacy_theme_id: Option<String>,
    legacy_style_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// 定义换皮宿主 `RecoveredSkinIdentity` 使用的内部数据。
enum RecoveredSkinIdentity {
    Exact(SkinReference),
    LegacyId(String),
    LegacyThemeCss(String),
}

#[derive(Debug, Clone)]
/// 定义换皮宿主 `PlatformCodexProcess` 使用的内部数据。
struct PlatformCodexProcess {
    pid: u32,
    executable: PathBuf,
    command_line: String,
}

#[derive(Debug, Clone)]
/// 定义换皮宿主 `ResolvedCodexInstance` 使用的内部数据。
struct ResolvedCodexInstance {
    id: String,
    process: PlatformCodexProcess,
    arguments: Vec<String>,
    debug_port: Option<u16>,
    profile: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// 定义换皮宿主 `CdpEndpoint` 使用的内部数据。
struct CdpEndpoint {
    port: u16,
}

impl CdpEndpoint {
    /// 执行换皮宿主内部的 `new` 步骤。
    fn new(port: u16) -> Self {
        Self { port }
    }

    /// 执行换皮宿主内部的 `url` 步骤。
    fn url(self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Default for CdpEndpoint {
    /// 执行换皮宿主内部的 `default` 步骤。
    fn default() -> Self {
        Self::new(DEFAULT_CDP_PORT)
    }
}

impl CodexRuntimeStatus {
    /// 执行换皮宿主内部的 `new` 步骤。
    fn new(state: CodexRuntimeState) -> Self {
        Self { state }
    }
}

impl SkinStatus {
    /// 执行换皮宿主内部的 `stopped` 步骤。
    fn stopped(affected_pages: usize) -> Self {
        Self {
            installed: false,
            skin_id: None,
            source: None,
            package_type: None,
            skin_name: None,
            version: SKIN_VERSION.into(),
            affected_pages,
            compatibility: None,
            instance_id: None,
        }
    }

    /// 执行换皮宿主内部的 `running` 步骤。
    fn running(
        skin: &SkinDescriptor,
        affected_pages: usize,
        compatibility: Option<SkinCompatibilityStatus>,
    ) -> Self {
        Self {
            installed: true,
            skin_id: Some(skin.id.clone()),
            source: Some(skin.source),
            package_type: Some(skin.package_type),
            skin_name: Some(skin.name.clone()),
            version: skin.version.clone(),
            affected_pages,
            compatibility,
            instance_id: None,
        }
    }

    /// 执行换皮宿主内部的 `for_instance` 步骤。
    fn for_instance(mut self, instance_id: impl Into<String>) -> Self {
        self.instance_id = Some(instance_id.into());
        self
    }
}
