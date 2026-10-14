// 本文件收纳：皮肤包清单与外观契约的数据结构、页面探针结构，以及监视任务与在途操作的句柄守卫。
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

/// 持续监视宿主新页面并补注入的后台任务句柄，含取消信号、CDP handler 中止句柄与回收 owner。
struct WatchTask {
    host: SkinHostKind,
    cancel: watch::Sender<bool>,
    join: Option<JoinHandle<Result<usize, AppError>>>,
    handler_abort: Option<AbortHandle>,
    endpoint: CdpEndpoint,
    owner: StdWeak<StdMutex<WatchTaskReaper>>,
}

/// 保存已经离开运行态、但尚未完成终态确认或端点清理的监视任务。
struct RetainedWatchTask {
    host: SkinHostKind,
    endpoint: CdpEndpoint,
    /// `None` 表示任务已经确认终止，只剩端点清理需要在后续操作重试。
    join: Option<JoinHandle<Result<usize, AppError>>>,
    /// watcher 终态前需要一并中止的 CDP handler。
    handler_abort: Option<AbortHandle>,
    /// 正常生命周期仍存在时把取消中的句柄归还原 service owner。
    owner: StdWeak<StdMutex<WatchTaskReaper>>,
}

#[derive(Default)]
/// 由 `SkinService` 持有的监视任务回收器；同步锁只保护短暂的句柄移交。
struct WatchTaskReaper {
    retained: Vec<RetainedWatchTask>,
    /// 已构造且任务 future 尚未到终态的 watcher 取消发送端。
    active: HashMap<u64, watch::Sender<bool>>,
    /// 下一个 active watcher 登记 ID。
    next_registration_id: u64,
    shutting_down: bool,
    /// `SkinService::drop` 已开始，后续迟到句柄必须交给进程级 owner。
    service_dropped: bool,
    /// 首次应用退出事件确定的总截止；重复退出事件不得重新获得完整预算。
    shutdown_deadline: Option<tokio::time::Instant>,
}

/// watcher future 持有的 active 登记守卫；终态或 abort 都会从 service owner 注销。
struct ActiveWatchRegistration {
    registration_id: Option<u64>,
    owner: Arc<StdMutex<WatchTaskReaper>>,
}

/// 持有一个 CDP handler 任务；丢弃或显式中止时把任务交给进程级回收，避免句柄丢失。
struct HandlerTaskGuard(Option<JoinHandle<()>>);

impl HandlerTaskGuard {
    #[cfg(test)]
    /// 为隔离测试登记容量并接管手工构造的 handler。
    fn new(task: JoinHandle<()>) -> Self {
        reserve_handler_task_capacity()
            .unwrap_or_else(|error| panic!("handler fixture capacity unavailable: {}", error.code))
            .bind(task)
    }

    /// 中止 handler 任务并移交进程级 owner 确认终态；重复调用无副作用。
    fn abort(&mut self) {
        if let Some(task) = self.0.take() {
            retain_aborted_handler_task(task);
        }
    }

    /// 中止旧 handler，并从另一个 guard 原子接管替代任务。
    fn replace(&mut self, mut replacement: Self) {
        self.abort();
        self.0 = replacement.0.take();
    }

    /// 已结束任务不能被登记为运行中的 watcher。
    fn is_finished(&self) -> bool {
        self.0.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// 为 watcher owner 复制独立取消能力，同时保留原 JoinHandle 所有权。
    fn abort_handle(&self) -> Option<AbortHandle> {
        self.0
            .as_ref()
            .filter(|task| !task.is_finished())
            .map(JoinHandle::abort_handle)
    }
}

impl Drop for HandlerTaskGuard {
    /// 守卫释放时中止仍在运行的 handler。
    fn drop(&mut self) {
        self.abort();
    }
}

/// 当前在途的宿主操作：操作代次与取消信号。
struct CodexOperation {
    id: u64,
    cancel: watch::Sender<bool>,
}

/// 在途宿主操作的作用域守卫；结束时只清除仍属于自己的操作登记。
struct CodexOperationGuard<'a> {
    id: u64,
    active: &'a StdMutex<Option<CodexOperation>>,
}

impl Drop for CodexOperationGuard<'_> {
    /// 仅当登记的仍是本操作时清除，避免误清后续操作。
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

/// 在宿主变更期间把运行代次保持为奇数，结束（包括错误返回）后恢复为新的偶数代次。
struct HostRuntimeMutationGuard<'a> {
    generation: &'a AtomicU64,
}

impl Drop for HostRuntimeMutationGuard<'_> {
    /// 结束宿主变更并使所有变更前或变更中的异步探针失效。
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - manifests_runtime_types.rs：实例运行态、导入暂存批次、目录缓存指纹与注入汇总报告。
include!("manifests_runtime_types.rs");
