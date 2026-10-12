//! 皮肤页面只拥有查询快照、草稿与操作回执，文件事务属于宿主服务。

use crate::{
    skins::{SkinDescriptor, SkinError, SkinHostInstance, SkinStatus},
    usage_service::SkinEntry,
};
use gpui_kit::{Entity, FocusHandle, Subscription, Task, component::input::InputState};
use loki_metris_core::{SkinHostKind, SkinReference};
use std::sync::{Arc, atomic::AtomicU64};

/// 模态保留打开前的焦点与稳定输入实体。
pub(crate) struct Modal {
    pub kind: ModalKind,
    pub focus: FocusHandle,
    pub previous: Option<FocusHandle>,
}
/// 资源确认均携带联合身份，不能把同ID的内置与用户包混为一项。
#[derive(Clone)]
pub(crate) enum ModalKind {
    Create([Entity<InputState>; 2]),
    Delete(Vec<SkinReference>),
    Convert(SkinDescriptor),
    Appearance(crate::skins::SkinAppearanceCheck, PendingApply),
    Restart(PendingApply, RestartMode),
    Import,
}
/// 一次应用意图绑定宿主与资源；确认后重新检查原生实例身份。
#[derive(Clone)]
pub(crate) struct PendingApply {
    pub host: SkinHostKind,
    pub skin: SkinReference,
    pub instance: Option<String>,
    pub instance_label: String,
    pub allow_appearance: bool,
    pub allow_workbuddy_recovery: bool,
    pub allow_third_party_code: bool,
}
/// 单实例重启与Windows整宿主恢复具有不同确认文案和执行入口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RestartMode {
    RestartSelected,
    RecoverWindowsWorkBuddy,
}
/// 仅搜索与宿主选择跨路由保留；异步操作离页后仍有应用级owner。
pub(in crate::app) struct Skins {
    pub host: Option<SkinHostKind>,
    pub entries: Option<Vec<SkinEntry>>,
    pub preview_residencies: super::preview_residency::PreviewResidencies,
    pub catalog_error: Option<SkinError>,
    pub host_error: Option<SkinError>,
    pub instances: Vec<SkinHostInstance>,
    pub status: Option<SkinStatus>,
    pub search: Option<Entity<InputState>>,
    pub search_subscription: Option<Subscription>,
    pub input_subscriptions: Vec<Subscription>,
    pub selected: Vec<SkinReference>,
    pub modal: Option<Modal>,
    pub prompt: Option<String>,
    pub copied: bool,
    pub busy: bool,
    pub loading: bool,
    pub last_read: Option<std::time::Instant>,
    pub generation: u64,
    pub authority: Arc<AtomicU64>,
    pub query: Option<Task<()>>,
    pub mutation: Option<Task<()>>,
    pub feedback: Option<Result<SkinSuccess, SkinError>>,
    pub import: Option<ImportDraft>,
    pub picking: bool,
}
impl Default for Skins {
    /// 未收到真实目录和宿主快照前不开放写操作。
    fn default() -> Self {
        Self {
            host: None,
            entries: None,
            preview_residencies: Default::default(),
            catalog_error: None,
            host_error: None,
            instances: Vec::new(),
            status: None,
            search: None,
            search_subscription: None,
            input_subscriptions: Vec::new(),
            selected: Vec::new(),
            modal: None,
            prompt: None,
            copied: false,
            busy: false,
            loading: false,
            last_read: None,
            generation: 0,
            authority: Arc::new(AtomicU64::new(0)),
            query: None,
            mutation: None,
            feedback: None,
            import: None,
            picking: false,
        }
    }
}

/// 单一UI租约持有预检token；选择改变时只重置所选脚本集合的信任。
pub(crate) struct ImportDraft {
    pub lease: crate::usage_service::SkinImportLease,
    pub batch: Option<crate::skins::PreparedSkinImportBatch>,
    pub selected: Vec<String>,
    pub trusted: bool,
}

/// 成功回执保留实际资源参数，展示时才使用当前语言生成通知。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) enum SkinSuccess {
    Created { name: String },
    Converted { name: String },
    Deleted { count: usize },
    Exported,
    Stopped,
    Applied { name: String },
    Imported { count: usize },
}

/// 资源操作的实际终态，回执不固化当前语言的译文。
pub(super) enum Outcome {
    Created(SkinDescriptor),
    Converted(crate::skins::ThemeConversionResult),
    Deleted(crate::skins::BatchDeleteResult),
    Prompt(String),
    Opened,
    Exported,
    Applied(crate::skins::InstallSkinResult, PendingApply),
    NeedsRestart(PendingApply, RestartMode),
    Stopped,
}
