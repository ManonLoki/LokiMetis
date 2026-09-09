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
    #[serde(default)]
    codex: bool,
    #[serde(default, rename = "workBuddy")]
    work_buddy: bool,
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

    /// WorkBuddy 只接受其本机 `file:` 渲染页面与稳定标题、根节点组合。
    fn is_verified_workbuddy(&self) -> bool {
        self.work_buddy && self.url.starts_with("file://")
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

    /// 只移交仍存活的 handler；已结束任务不能被登记为运行中的 watcher。
    fn take(&mut self) -> Option<JoinHandle<()>> {
        if self.0.as_ref().is_some_and(JoinHandle::is_finished) {
            self.0.take();
            return None;
        }
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
    last_targets: HashMap<SkinHostKind, String>,
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
    transaction_targets: BTreeSet<String>,
    compatibility_applied_rules: BTreeSet<String>,
    compatibility_skipped_rules: BTreeSet<String>,
}

#[derive(Clone)]
/// 保存一次初始注入的随机所有权标记与已登记页面；外层取消 future 后仍可回滚。
struct InjectionTransaction {
    id: Arc<str>,
    targets: Arc<StdMutex<BTreeSet<String>>>,
}

impl InjectionTransaction {
    /// 创建尚未登记页面的注入事务，并保存不可变所有权标记。
    fn new(id: &str) -> Self {
        Self {
            id: Arc::from(id),
            targets: Arc::new(StdMutex::new(BTreeSet::new())),
        }
    }

    /// 返回用于页面 DOM marker 的本次事务标识。
    fn id(&self) -> &str {
        &self.id
    }

    /// 必须在写 DOM marker 前登记；锁中毒时保留集合并继续完成补偿路径。
    fn track(&self, target_id: String) {
        self.targets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(target_id);
    }

    /// 取得已登记页面的快照，供提交或补偿阶段稳定遍历。
    fn tracked_targets(&self) -> BTreeSet<String> {
        self.targets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
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
