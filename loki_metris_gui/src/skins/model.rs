// 本文件收纳：换肤对外数据模型（皮肤描述、外观预检、导入与删除结果、运行状态、宿主实例）及内部探针与端点类型。
// （本文件由主文件 include! 并入，与主文件共用同一个模块作用域。）

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 皮肤库中一项可安装皮肤的展示描述：身份、来源与包类型、作者版本、预览图及支持的明暗模式。
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
/// 宿主当前外观与皮肤期望不一致的一项设置，含展示标签、当前值（读不到时为空）与期望值。
pub struct AppearanceDifference {
    pub field: String,
    pub label: String,
    pub current_value: Option<String>,
    pub expected_value: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 安装前的外观预检结果：宿主实际明暗模式、皮肤支持的模式，以及不一致项与无法读取项。
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
/// 安装结果：已注入并返回运行状态，或外观不一致需要用户确认后重试。
pub enum InstallSkinResult {
    Installed { status: SkinStatus },
    NeedsConfirmation { check: SkinAppearanceCheck },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 供用户复制给 AI 生成新皮肤的完整提示词。
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
/// 批量删除中未能删除的一项皮肤，附稳定错误码与脱敏说明。
pub struct FailedSkinDelete {
    pub skin: SkinReference,
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 批量删除的分项结果：已删除的皮肤与失败项分别列出，不因单项失败回滚其它项。
pub struct BatchDeleteResult {
    pub deleted: Vec<SkinReference>,
    pub failed: Vec<FailedSkinDelete>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
/// 导入预检通过、等待用户确认安装的一个归档条目。
pub struct PreparedSkinImportItem {
    pub item_id: String,
    pub archive_name: String,
    pub skin: SkinDescriptor,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
/// 导入预检阶段被跳过的归档，附稳定错误码、说明与校验细节。
pub struct SkippedSkinImport {
    pub archive_name: String,
    pub code: &'static str,
    pub message: String,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
/// 一次导入预检的完整批次：确认安装时凭 token 取回暂存条目，并汇总文件总数与跳过项。
pub struct PreparedSkinImportBatch {
    pub token: String,
    pub items: Vec<PreparedSkinImportItem>,
    pub total_files: usize,
    pub skipped: Vec<SkippedSkinImport>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
/// 确认安装阶段失败的导入条目，保留归档名、皮肤名与校验细节以便提示。
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
/// 确认安装批次的结果：成功安装的皮肤、失败项与预检阶段跳过的数量。
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
/// 导入预检的流式进度事件：开始、单项就绪或单项跳过，均携带所属批次 token。
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
/// 宿主当前换肤运行状态：是否已注入、生效皮肤身份与版本、受影响页面数、兼容规则应用情况及所属实例。
pub struct SkinStatus {
    pub(crate) installed: bool,
    pub(crate) skin_id: Option<String>,
    pub(crate) source: Option<SkinSource>,
    pub(crate) package_type: Option<SkinPackageType>,
    pub(crate) skin_name: Option<String>,
    pub(crate) version: String,
    pub(crate) affected_pages: usize,
    pub(crate) compatibility: Option<SkinCompatibilityStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) instance_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 皮肤对当前宿主版本的兼容情况：适配模式及已应用、已跳过的兼容规则。
pub struct SkinCompatibilityStatus {
    version: String,
    mode: SkinCompatibilityMode,
    applied_rules: Vec<String>,
    skipped_rules: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 皮肤兼容模式：原生匹配、经规则适配，或部分规则无法应用。
pub enum SkinCompatibilityMode {
    Native,
    Adapted,
    Partial,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 换肤宿主进程状态：未运行、已开放调试端口可换肤，或运行中但没有可用 CDP。
pub enum CodexRuntimeState {
    Stopped,
    Ready,
    RunningWithoutCdp,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 宿主运行状态查询结果，只携带运行状态枚举。
pub struct CodexRuntimeStatus {
    pub(crate) state: CodexRuntimeState,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 界面展示的一个宿主实例：进程、配置档、调试端口、当前生效皮肤及账户标签与头像。
pub struct SkinHostInstance {
    pub(crate) id: String,
    pub(crate) pid: u32,
    pub(crate) label: String,
    pub(crate) profile: Option<String>,
    pub(crate) state: CodexRuntimeState,
    pub(crate) debug_port: Option<u16>,
    pub(crate) active_skin_name: Option<String>,
    pub(crate) active_skin: Option<SkinReference>,
    pub(crate) account_label: Option<String>,
    pub(crate) avatar_data_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
/// 从宿主页面读取的账户资料原始探针结果，字段均可能缺失。
struct AccountProfileProbe {
    label: Option<String>,
    avatar_data_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
/// 仅解析页面当前活动皮肤引用，不读取账户资料。
struct ActiveSkinOnlyProbe {
    active_skin: Option<ActiveSkinProbe>,
}

#[derive(Debug, Clone)]
/// 已校验并缓存的实例账户资料，用于实例列表展示。
struct AccountProfile {
    label: Option<String>,
    avatar_data_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
/// 从宿主页面读取的当前生效皮肤标记，兼容新版引用与旧版主题 ID、样式文本。
struct ActiveSkinProbe {
    version: Option<String>,
    source: Option<SkinSource>,
    id: Option<String>,
    legacy_theme_id: Option<String>,
    legacy_style_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// 从页面恢复出的生效皮肤身份：精确引用、旧版主题 ID 或旧版主题样式文本。
enum RecoveredSkinIdentity {
    Exact(SkinReference),
    LegacyId(String),
    LegacyThemeCss(String),
}

#[derive(Debug, Clone)]
/// 已通过身份校验的宿主进程：PID、可执行文件路径与完整命令行。
struct PlatformHostProcess {
    pid: u32,
    executable: PathBuf,
    command_line: String,
}

#[derive(Debug, Clone)]
/// 解析完成的宿主实例：稳定实例 ID、进程、启动参数、调试端口与配置档。
struct ResolvedSkinHostInstance {
    id: String,
    process: PlatformHostProcess,
    arguments: Vec<String>,
    debug_port: Option<u16>,
    profile: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// 本机回环 CDP 调试端点，只保存端口。
struct CdpEndpoint {
    port: u16,
}

impl CdpEndpoint {
    /// 以指定端口构造回环端点。
    fn new(port: u16) -> Self {
        Self { port }
    }

    /// 返回一个宿主专用且互不冲突的默认回环端口。
    fn default_for(host: SkinHostKind) -> Self {
        Self::new(match host {
            SkinHostKind::Codex => DEFAULT_CDP_PORT,
            SkinHostKind::WorkBuddy => WORKBUDDY_DEFAULT_CDP_PORT,
        })
    }

    /// 返回该端点的 HTTP 基础地址。
    fn url(self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Default for CdpEndpoint {
    /// 默认端点使用 Codex 的默认调试端口。
    fn default() -> Self {
        Self::new(DEFAULT_CDP_PORT)
    }
}

impl CodexRuntimeStatus {
    /// 以运行状态构造查询结果。
    fn new(state: CodexRuntimeState) -> Self {
        Self { state }
    }
}

impl SkinStatus {
    /// 未注入皮肤的状态，保留本次清理影响的页面数。
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

    /// 指定皮肤已注入时的运行状态。
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

    /// 把状态绑定到指定宿主实例。
    fn for_instance(mut self, instance_id: impl Into<String>) -> Self {
        self.instance_id = Some(instance_id.into());
        self
    }
}
