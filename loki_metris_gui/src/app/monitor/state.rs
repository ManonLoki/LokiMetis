//! 监控编辑态只拥有未提交草稿、原生输入与异步回执，不替代磁盘权威记录。

use crate::monitor::image_playback::MonitorImageView;
use crate::monitor::{image_decode::MonitorImageSource, images::MonitorGallerySnapshot};
use gpui_kit::{Entity, Subscription, Task, component::input::TextareaState};
use loki_metris_core::{AiProfileDraft, AiTool, HookBehavior, HookError, ImageFormat};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

/// 每个挂载位置拥有独立播放时钟，资源本身继续共享。
type ImageViews = Rc<RefCell<HashMap<String, (u64, Entity<MonitorImageView>)>>>;

/// 已实现子页；未接通的功能不渲染占位操作。
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Route {
    /// 工作台。
    #[default]
    Workbench,
    /// 形象配置管理。
    Management,
    /// 图库。
    Images,
    /// Hook 配置。
    Hooks,
}

/// 各选择器独立保留格式筛选，切换 Agent 时与原版卸载面板一致重置。
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Category {
    /// 全部格式。
    #[default]
    All,
    /// 仅指定格式。
    Format(ImageFormat),
}
impl Category {
    /// 固定筛选顺序：全部在前，其后沿用 core 的格式顺序。
    pub const ALL: [Self; ImageFormat::ALL.len() + 1] = {
        let mut all = [Self::All; ImageFormat::ALL.len() + 1];
        let mut index = 0;
        while index < ImageFormat::ALL.len() {
            all[index + 1] = Self::Format(ImageFormat::ALL[index]);
            index += 1;
        }
        all
    };
    /// 格式由 core 识别，文件扩展名不决定筛选结果。
    pub fn accepts(self, format: ImageFormat) -> bool {
        self == Self::All || self == Self::Format(format)
    }
}

/// 原生图库去掉领域兼容 data URL，保留真实字节与可修复错误。
#[derive(Clone)]
pub(super) struct ImageEntry {
    /// 图片 ID。
    pub id: String,
    /// 原始文件名。
    pub filename: String,
    /// 图片格式。
    pub format: ImageFormat,
    /// 可播放的图片源；资源缺失或损坏时为可修复错误。
    pub source: Result<MonitorImageSource, HookError>,
    /// 各展示位置的播放视图。
    pub views: ImageViews,
    /// 本条目在各位置的驻留与可见性记录。
    pub residency: Rc<ImageResidency>,
}

/// 每个图像可同时用于图库与多个选择器，完整帧结束后统一判断是否仍可见。
#[derive(Default)]
pub(super) struct ImageResidency {
    /// 已向驻留登记器请求解码。
    pub requested: Cell<bool>,
    /// 已安排在帧结束后清理不可见位置，避免重复安排。
    pub cleanup_pending: Cell<bool>,
    /// 按位置 ID 记录 (最近登记的帧代次, 本帧是否可见)。
    pub placements: RefCell<HashMap<String, (u64, bool)>>,
}
impl ImageResidency {
    /// 当前壳层登记位置并保留上帧可见性，避免 render/prepaint 之间抖动驱逐。
    pub fn register(&self, id: String, epoch: u64) {
        self.placements
            .borrow_mut()
            .entry(id)
            .and_modify(|value| value.0 = epoch)
            .or_insert((epoch, false));
    }
    /// 全部子元素 prepaint 完成后读取，不能按回调顺序单独判断共享资源。
    pub fn any_visible(&self) -> bool {
        self.placements
            .borrow()
            .values()
            .any(|(_, visible)| *visible)
    }
}

/// 同一 Agent 的输入实体跨页面保持，避免重绘覆盖 IME 和未提交文案。
pub(super) struct Editor {
    /// 该 Agent 的形象配置草稿。
    pub draft: AiProfileDraft,
    /// 草稿修订号；每次编辑递增。
    pub revision: u64,
    /// 四个文案输入框，依次对应四种行为；首次进入时创建。
    pub inputs: Option<[Entity<TextareaState>; 4]>,
    /// 下一帧需要用草稿重写输入框内容。
    pub reset_inputs: bool,
    /// 输入变化的订阅。
    pub subscriptions: Vec<Subscription>,
}
impl Editor {
    /// 初始化只接受真实档案读取结果。
    fn new(draft: AiProfileDraft) -> Self {
        Self {
            draft,
            revision: 0,
            inputs: None,
            reset_inputs: true,
            subscriptions: Vec::new(),
        }
    }
}

/// 成功提示只保留展示参数，避免跨页保留的回执冻结旧语言译文。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum CatalogSuccess {
    /// 上传了若干图片。
    ImagesUploaded { count: usize },
    /// 在选择器中上传并选中一张图片。
    UploadedAndSelected { filename: String },
}

/// 页面保留查询、保存与导入任务直到回执，页面切换不撤销已接受写入。
#[derive(Default)]
pub(super) struct Catalog {
    /// 图片帧代次；每帧开始时递增，帧结束时未在本代次登记的位置被暂停或释放。
    pub frame_epoch: Cell<u64>,
    /// 各 Agent 的形象配置编辑器。
    pub editors: Vec<Editor>,
    /// 配置草稿至少成功读取过一次。
    pub profiles_loaded: bool,
    /// 配置草稿读取在途。
    pub profiles_loading: bool,
    /// 配置草稿读取失败的原因。
    pub profiles_error: Option<HookError>,
    /// 当前图库条目。
    pub images: Vec<ImageEntry>,
    /// 已被新快照替换、待下一帧释放像素的旧条目。
    pub retired_images: Vec<ImageEntry>,
    /// 图库至少成功读取过一次。
    pub images_loaded: bool,
    /// 图库读取在途。
    pub images_loading: bool,
    /// 图库读取失败的原因。
    pub images_error: Option<HookError>,
    /// 当前编辑的 Agent。
    pub selected: Option<AiTool>,
    /// 图库页的格式筛选。
    pub category: Category,
    /// 打开中的图片选择器所属行为。
    pub picker: Option<HookBehavior>,
    /// 选择器内的格式筛选。
    pub picker_category: Category,
    /// 最近一次图库或配置操作的反馈。
    pub feedback: Option<Result<CatalogSuccess, HookError>>,
    /// 刚保存成功的 Agent，用于在对应行显示已保存提示。
    pub saved_tool: Option<AiTool>,
    /// 配置保存在途。
    pub saving: bool,
    /// 图片上传在途。
    pub uploading: bool,
    /// 正在删除的图片 ID。
    pub deleting: Option<String>,
    /// 配置读取代次；旧读取结果被丢弃。
    pub profile_query: u64,
    /// 图库读取代次；旧读取结果被丢弃。
    pub image_query: u64,
    /// 配置读取任务。
    pub profile_task: Option<Task<()>>,
    /// 图库读取任务。
    pub image_task: Option<Task<()>>,
    /// 配置保存任务。
    pub save_task: Option<Task<()>>,
    /// 上传或删除任务。
    pub mutation_task: Option<Task<()>>,
}
impl Catalog {
    /// 保留私有编辑器状态，初始读入按公开目录顺序建立。
    pub fn accept_profiles(&mut self, drafts: Vec<AiProfileDraft>) {
        self.editors = drafts.into_iter().map(Editor::new).collect();
        self.profiles_loaded = true;
        self.profiles_error = None;
    }
    /// 快照转换不做磁盘读取、格式解码或 Base64 解析。
    pub fn accept_images(&mut self, mut snapshot: MonitorGallerySnapshot) {
        let mut old_images = self
            .images
            .drain(..)
            .map(|image| (image.id.clone(), image))
            .collect::<HashMap<_, _>>();
        self.images = snapshot
            .gallery
            .images
            .into_iter()
            .map(|image| {
                let source = snapshot
                    .bytes
                    .remove(&image.id)
                    .map(|bytes| MonitorImageSource {
                        id: image.id.clone(),
                        bytes,
                        format: image.format,
                    })
                    .ok_or_else(|| HookError::new("error.monitor.imagesReadFailed"));
                ImageEntry {
                    id: image.id,
                    filename: image.filename,
                    format: image.format,
                    source,
                    views: Default::default(),
                    residency: Default::default(),
                }
            })
            .chain(snapshot.unavailable.into_iter().map(|image| ImageEntry {
                id: image.id,
                filename: image.filename,
                format: image.format,
                source: Err(image.error),
                views: Default::default(),
                residency: Default::default(),
            }))
            .collect();
        for image in &mut self.images {
            if let Some(old) = old_images.remove(&image.id) {
                if image.source.is_ok() == old.source.is_ok() {
                    image.views = old.views;
                    image.residency = old.residency;
                } else {
                    self.retired_images.push(old);
                }
            }
        }
        self.retired_images.extend(old_images.into_values());
        self.images_loaded = true;
        self.images_error = None;
    }
    /// UI 工作流按工具定位草稿，不把公开子集序号当持久身份。
    pub fn editor_mut(&mut self, tool: AiTool) -> Option<&mut Editor> {
        self.editors
            .iter_mut()
            .find(|editor| editor.draft.tool == tool)
    }
    /// 修改清除之前的保存状态，递增代数以保护在途保存之后的新输入。
    pub fn changed(&mut self, tool: AiTool) {
        if let Some(editor) = self.editor_mut(tool) {
            editor.revision = editor.revision.wrapping_add(1);
        }
        self.feedback = None;
        self.saved_tool = None;
    }
    /// 格式计数包含坏资源条目，使用户仍可按原索引找到并清理它。
    pub fn count(&self, category: Category) -> usize {
        self.images
            .iter()
            .filter(|image| category.accepts(image.format))
            .count()
    }
    /// 选择器只统计可读取资源，坏条目仍在图库中保留以便修复。
    pub fn picker_count(&self, category: Category) -> usize {
        self.images
            .iter()
            .filter(|image| image.source.is_ok() && category.accepts(image.format))
            .count()
    }
}
