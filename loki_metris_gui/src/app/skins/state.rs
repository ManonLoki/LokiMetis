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
    /// 弹窗类型与数据。
    pub kind: ModalKind,
    /// 弹窗焦点。
    pub focus: FocusHandle,
    /// 打开前的焦点，关闭时恢复。
    pub previous: Option<FocusHandle>,
}
/// 资源确认均携带联合身份，不能把同ID的内置与用户包混为一项。
#[derive(Clone)]
pub(crate) enum ModalKind {
    /// 新建皮肤，附名称与作者两个输入框。
    Create([Entity<InputState>; 2]),
    /// 确认删除选中的皮肤。
    Delete(Vec<SkinReference>),
    /// 确认把旧皮肤转换为主题。
    Convert(SkinDescriptor),
    /// 外观不一致确认。
    Appearance(crate::skins::SkinAppearanceCheck, PendingApply),
    /// 重启宿主确认。
    Restart(PendingApply, RestartMode),
    /// 导入预检结果。
    Import,
}
/// 一次应用意图绑定宿主与资源；确认后重新检查原生实例身份。
#[derive(Clone)]
pub(crate) struct PendingApply {
    /// 目标宿主。
    pub host: SkinHostKind,
    /// 要应用的皮肤。
    pub skin: SkinReference,
    /// 目标实例 ID；None 表示由服务选择。
    pub instance: Option<String>,
    /// 目标实例的展示名。
    pub instance_label: String,
    /// 用户已确认外观不一致仍继续。
    pub allow_appearance: bool,
    /// 用户已确认允许恢复 Windows WorkBuddy 整个宿主。
    pub allow_workbuddy_recovery: bool,
    /// 用户已确认信任第三方可执行皮肤。
    pub allow_third_party_code: bool,
}
/// 单实例重启与Windows整宿主恢复具有不同确认文案和执行入口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RestartMode {
    /// 只重启选中的实例。
    RestartSelected,
    /// 恢复 Windows 上整个 WorkBuddy 宿主。
    RecoverWindowsWorkBuddy,
}
/// 仅搜索与宿主选择跨路由保留；异步操作离页后仍有应用级owner。
pub(in crate::app) struct Skins {
    /// 当前选中的换肤宿主。
    pub host: Option<SkinHostKind>,
    /// 皮肤目录；尚未读取时为 None。
    pub entries: Option<Vec<SkinEntry>>,
    /// 皮肤预览图的驻留记录。
    pub preview_residencies: super::preview_residency::PreviewResidencies,
    /// 目录读取失败的原因。
    pub catalog_error: Option<SkinError>,
    /// 宿主状态读取失败的原因。
    pub host_error: Option<SkinError>,
    /// 当前宿主的运行实例。
    pub instances: Vec<SkinHostInstance>,
    /// 当前宿主的换肤状态。
    pub status: Option<SkinStatus>,
    /// 搜索输入框；首次进入时创建。
    pub search: Option<Entity<InputState>>,
    /// 搜索输入变化的订阅。
    pub search_subscription: Option<Subscription>,
    /// 弹窗内输入框的订阅，离页时清空。
    pub input_subscriptions: Vec<Subscription>,
    /// 批量操作选中的皮肤。
    pub selected: Vec<SkinReference>,
    /// 打开中的弹窗。
    pub modal: Option<Modal>,
    /// 已生成的 AI 创建提示词。
    pub prompt: Option<String>,
    /// 提示词刚被复制。
    pub copied: bool,
    /// 有改动宿主或资源的操作在途。
    pub busy: bool,
    /// 目录或宿主状态读取在途。
    pub loading: bool,
    /// 最近一次读取的时刻，用于定时刷新。
    pub last_read: Option<std::time::Instant>,
    /// 读取代次；切换宿主或重新读取时递增，旧结果被丢弃。
    pub generation: u64,
    /// 页面授权代次；离页或切换宿主时递增，尚未开始的宿主副作用随之作废。
    pub authority: Arc<AtomicU64>,
    /// 读取任务。
    pub query: Option<Task<()>>,
    /// 改动任务。
    pub mutation: Option<Task<()>>,
    /// 最近一次操作的反馈。
    pub feedback: Option<Result<SkinSuccess, SkinError>>,
    /// 进行中的导入流程。
    pub import: Option<ImportDraft>,
    /// 系统文件选择框打开中。
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
    /// 导入会话租约。
    pub lease: crate::usage_service::SkinImportLease,
    /// 预检批次；预检完成前为 None。
    pub batch: Option<crate::skins::PreparedSkinImportBatch>,
    /// 选中要安装的条目 ID。
    pub selected: Vec<String>,
    /// 用户已确认信任所选的可执行皮肤；选择变化时重置。
    pub trusted: bool,
}

/// 成功回执保留实际资源参数，展示时才使用当前语言生成通知。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) enum SkinSuccess {
    /// 新建了皮肤。
    Created { name: String },
    /// 转换完成。
    Converted { name: String },
    /// 删除了若干皮肤。
    Deleted { count: usize },
    /// 已导出。
    Exported,
    /// 已停止换肤。
    Stopped,
    /// 已应用皮肤。
    Applied { name: String },
    /// 导入了若干皮肤。
    Imported { count: usize },
}

/// 资源操作的实际终态，回执不固化当前语言的译文。
pub(super) enum Outcome {
    /// 新建了皮肤。
    Created(SkinDescriptor),
    /// 旧皮肤转换为主题。
    Converted(crate::skins::ThemeConversionResult),
    /// 批量删除完成。
    Deleted(crate::skins::BatchDeleteResult),
    /// 生成了 AI 创建提示词。
    Prompt(String),
    /// 已在文件管理器中打开目录。
    Opened,
    /// 已导出。
    Exported,
    /// 应用流程返回结果，附原始意图以便确认后重试。
    Applied(crate::skins::InstallSkinResult, PendingApply),
    /// 需要重启宿主才能应用。
    NeedsRestart(PendingApply, RestartMode),
    /// 已停止换肤。
    Stopped,
}
