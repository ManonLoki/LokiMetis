//! 换肤对外数据模型（皮肤描述、外观预检、导入与删除结果、运行状态、宿主实例）及内部探针与端点类型。

use super::*;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 皮肤库中一项可安装皮肤的展示描述：身份、来源与包类型、作者版本、预览图及支持的明暗模式。
pub struct SkinDescriptor {
    /// 皮肤 ID，在同一来源内唯一。
    pub id: String,
    /// 内置或用户导入。
    pub source: SkinSource,
    /// 包类型，决定加载与信任方式。
    pub package_type: SkinPackageType,
    /// 展示名。
    pub name: String,
    /// 作者。
    pub author: String,
    /// 皮肤包版本。
    pub version: String,
    /// 预览图 data URL。
    pub preview_data_url: String,
    /// 支持的明暗模式。
    pub supported_color_modes: Vec<ColorMode>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 宿主当前外观与皮肤期望不一致的一项设置，含展示标签、当前值（读不到时为空）与期望值。
pub struct AppearanceDifference {
    /// 外观设置键。
    pub field: String,
    /// 展示标签。
    pub label: String,
    /// 宿主当前值；读不到时为 None。
    pub current_value: Option<String>,
    /// 皮肤期望值。
    pub expected_value: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 安装前的外观预检结果：宿主实际明暗模式、皮肤支持的模式，以及不一致项与无法读取项。
pub struct SkinAppearanceCheck {
    /// 宿主实际明暗模式。
    pub effective_mode: ColorMode,
    /// 皮肤支持的模式。
    pub supported_color_modes: Vec<ColorMode>,
    /// 与期望不一致的设置。
    pub differences: Vec<AppearanceDifference>,
    /// 无法读取当前值的设置。
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
    /// 已注入，附运行状态。
    Installed { status: SkinStatus },
    /// 外观不一致，需要用户确认后重试。
    NeedsConfirmation { check: SkinAppearanceCheck },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 供用户复制给 AI 生成新皮肤的完整提示词。
pub struct SkinCreationPrompt {
    /// 提示词正文。
    pub prompt: String,
}

/// 旧皮肤转换结果。`fallback_roles` 列出无法解析、已回退到模板默认值的配色角色。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThemeConversionResult {
    /// 转换生成的主题皮肤。
    pub theme: SkinDescriptor,
    /// 回退到模板默认值的配色角色。
    pub fallback_roles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 批量删除中未能删除的一项皮肤，附稳定错误码与脱敏说明。
pub struct FailedSkinDelete {
    /// 删除失败的皮肤。
    pub skin: SkinReference,
    /// 稳定错误码。
    pub code: &'static str,
    /// 脱敏说明。
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 批量删除的分项结果：已删除的皮肤与失败项分别列出，不因单项失败回滚其它项。
pub struct BatchDeleteResult {
    /// 已删除的皮肤。
    pub deleted: Vec<SkinReference>,
    /// 删除失败的项。
    pub failed: Vec<FailedSkinDelete>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
/// 导入预检通过、等待用户确认安装的一个归档条目。
pub struct PreparedSkinImportItem {
    /// 导入条目 ID。
    pub item_id: String,
    /// 归档文件名。
    pub archive_name: String,
    /// 候选皮肤描述。
    pub skin: SkinDescriptor,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
/// 导入预检阶段被跳过的归档，附稳定错误码、说明与校验细节。
pub struct SkippedSkinImport {
    /// 归档文件名。
    pub archive_name: String,
    /// 稳定错误码。
    pub code: &'static str,
    /// 脱敏说明。
    pub message: String,
    /// 校验细节。
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
/// 一次导入预检的完整批次：确认安装时凭 token 取回暂存条目，并汇总文件总数与跳过项。
pub struct PreparedSkinImportBatch {
    /// 批次令牌，确认安装时凭它取回暂存条目。
    pub token: String,
    /// 预检通过的条目。
    pub items: Vec<PreparedSkinImportItem>,
    /// 参与预检的文件总数。
    pub total_files: usize,
    /// 被跳过的归档。
    pub skipped: Vec<SkippedSkinImport>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
/// 确认安装阶段失败的导入条目，保留归档名、皮肤名与校验细节以便提示。
pub struct FailedSkinImport {
    /// 导入条目 ID。
    pub item_id: String,
    /// 归档文件名。
    pub archive_name: String,
    /// 皮肤展示名。
    pub skin_name: String,
    /// 稳定错误码。
    pub code: &'static str,
    /// 脱敏说明。
    pub message: String,
    /// 校验细节。
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
/// 确认安装批次的结果：成功安装的皮肤、失败项与预检阶段跳过的数量。
pub struct BatchImportResult {
    /// 成功安装的皮肤。
    pub installed: Vec<SkinDescriptor>,
    /// 安装失败的条目。
    pub failed: Vec<FailedSkinImport>,
    /// 预检阶段跳过的数量。
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
    /// 预检开始，附文件总数。
    Started {
        token: String,
        total_files: usize,
    },
    /// 一个归档预检通过。
    ItemReady {
        token: String,
        item: PreparedSkinImportItem,
    },
    /// 一个归档被跳过。
    ItemSkipped {
        token: String,
        item: SkippedSkinImport,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 宿主当前换肤运行状态：是否已注入、生效皮肤身份与版本、受影响页面数、兼容规则应用情况及所属实例。
pub struct SkinStatus {
    /// 宿主页面已注入皮肤。
    pub installed: bool,
    /// 生效皮肤 ID。
    pub skin_id: Option<String>,
    /// 生效皮肤来源（内置或用户导入）。
    pub source: Option<SkinSource>,
    /// 生效皮肤的包类型。
    pub package_type: Option<SkinPackageType>,
    /// 生效皮肤展示名。
    pub skin_name: Option<String>,
    /// 注入页面的皮肤运行时版本（SKIN_VERSION）。
    pub version: String,
    /// 已注入的页面数。
    pub affected_pages: usize,
    /// 兼容规则应用情况；未注入时为 None。
    pub compatibility: Option<SkinCompatibilityStatus>,
    /// 所属实例 ID；未指定实例时省略。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 皮肤对当前宿主版本的兼容情况：适配模式及已应用、已跳过的兼容规则。
pub struct SkinCompatibilityStatus {
    /// 宿主兼容脚本版本。
    pub(super) version: String,
    /// 兼容模式。
    pub(super) mode: SkinCompatibilityMode,
    /// 已应用的兼容规则。
    pub(super) applied_rules: Vec<String>,
    /// 无法应用的兼容规则。
    pub(super) skipped_rules: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 皮肤兼容模式：原生匹配、经规则适配，或部分规则无法应用。
pub enum SkinCompatibilityMode {
    /// 宿主版本原生匹配，无需规则。
    Native,
    /// 已通过兼容规则适配。
    Adapted,
    /// 部分兼容规则无法应用。
    Partial,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 换肤宿主进程状态：未运行、已开放调试端口可换肤，或运行中但没有可用 CDP。
pub enum CodexRuntimeState {
    /// 宿主未运行。
    Stopped,
    /// 已开放调试端口，可以换肤。
    Ready,
    /// 运行中但没有可用的 CDP，需要重启宿主。
    RunningWithoutCdp,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 宿主运行状态查询结果，只携带运行状态枚举。
pub struct CodexRuntimeStatus {
    /// 宿主运行状态。
    pub(crate) state: CodexRuntimeState,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
/// 界面展示的一个宿主实例：进程、配置档、调试端口、当前生效皮肤及账户标签与头像。
pub struct SkinHostInstance {
    /// 稳定实例 ID。
    pub id: String,
    /// 宿主主进程 PID。
    pub pid: u32,
    /// 实例展示名称。
    pub label: String,
    /// 宿主配置档名称；默认配置档为 None。
    pub profile: Option<String>,
    /// 实例运行状态。
    pub state: CodexRuntimeState,
    /// 已开放的 CDP 调试端口；未开放时为 None。
    pub debug_port: Option<u16>,
    /// 当前生效皮肤的展示名。
    pub active_skin_name: Option<String>,
    /// 当前生效皮肤的引用。
    pub active_skin: Option<SkinReference>,
    /// 账户展示名；读不到时为 None。
    pub account_label: Option<String>,
    /// 账户头像的 data URL；读不到时为 None。
    pub avatar_data_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
/// 从宿主页面读取的账户资料原始探针结果，字段均可能缺失。
pub(super) struct AccountProfileProbe {
    /// 账户展示名。
    pub(super) label: Option<String>,
    /// 头像 data URL。
    pub(super) avatar_data_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
/// 仅解析页面当前活动皮肤引用，不读取账户资料。
pub(super) struct ActiveSkinOnlyProbe {
    /// 页面当前生效皮肤；未注入时为 None。
    pub(super) active_skin: Option<ActiveSkinProbe>,
}

#[derive(Debug, Clone)]
/// 已校验并缓存的实例账户资料，用于实例列表展示。
pub(super) struct AccountProfile {
    /// 已校验的账户展示名。
    pub(super) label: Option<String>,
    /// 已校验的头像 data URL。
    pub(super) avatar_data_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
/// 从宿主页面读取的当前生效皮肤标记，兼容新版引用与旧版主题 ID、样式文本。
pub(super) struct ActiveSkinProbe {
    /// 页面中皮肤运行时版本。
    pub(super) version: Option<String>,
    /// 皮肤来源。
    pub(super) source: Option<SkinSource>,
    /// 皮肤 ID。
    pub(super) id: Option<String>,
    /// 旧版主题 ID。
    pub(super) legacy_theme_id: Option<String>,
    /// 旧版主题样式文本。
    pub(super) legacy_style_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// 从页面恢复出的生效皮肤身份：精确引用、旧版主题 ID 或旧版主题样式文本。
pub(super) enum RecoveredSkinIdentity {
    /// 新版精确皮肤引用。
    Exact(SkinReference),
    /// 旧版主题 ID。
    LegacyId(String),
    /// 旧版主题样式文本。
    LegacyThemeCss(String),
}

#[derive(Debug, Clone)]
/// 已通过身份校验的宿主进程：PID、可执行文件路径与完整命令行。
pub(super) struct PlatformHostProcess {
    /// 进程 PID。
    pub(super) pid: u32,
    /// 已校验的可执行文件路径。
    pub(super) executable: PathBuf,
    /// 完整命令行。
    pub(super) command_line: String,
}

#[derive(Debug, Clone)]
/// 解析完成的宿主实例：稳定实例 ID、进程、启动参数、调试端口与配置档。
pub(super) struct ResolvedSkinHostInstance {
    /// 稳定实例 ID。
    pub(super) id: String,
    /// 宿主进程。
    pub(super) process: PlatformHostProcess,
    /// 启动参数。
    pub(super) arguments: Vec<String>,
    /// 已开放的 CDP 调试端口。
    pub(super) debug_port: Option<u16>,
    /// 配置档名称。
    pub(super) profile: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// 本机回环 CDP 调试端点，只保存端口。
pub(super) struct CdpEndpoint {
    /// 回环地址上的调试端口。
    pub(super) port: u16,
}

impl CdpEndpoint {
    /// 以指定端口构造回环端点。
    pub(super) fn new(port: u16) -> Self {
        Self { port }
    }

    /// 返回一个宿主专用且互不冲突的默认回环端口。
    pub(super) fn default_for(host: SkinHostKind) -> Self {
        Self::new(match host {
            SkinHostKind::Codex => DEFAULT_CDP_PORT,
            SkinHostKind::WorkBuddy => WORKBUDDY_DEFAULT_CDP_PORT,
        })
    }

    /// 返回该端点的 HTTP 基础地址。
    pub(super) fn url(self) -> String {
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
    pub(super) fn new(state: CodexRuntimeState) -> Self {
        Self { state }
    }
}

impl SkinStatus {
    /// 未注入皮肤的状态，保留本次清理影响的页面数。
    pub(super) fn stopped(affected_pages: usize) -> Self {
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
    pub(super) fn running(
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
    pub(super) fn for_instance(mut self, instance_id: impl Into<String>) -> Self {
        self.instance_id = Some(instance_id.into());
        self
    }
}
