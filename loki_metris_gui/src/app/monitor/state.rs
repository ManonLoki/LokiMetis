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
    #[default]
    Workbench,
    Management,
    Images,
    Hooks,
}

/// 各选择器独立保留格式筛选，切换 Agent 时与原版卸载面板一致重置。
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Category {
    #[default]
    All,
    Jpeg,
    Png,
    Gif,
}
impl Category {
    /// 固定筛选顺序。
    pub const ALL: [Self; 4] = [Self::All, Self::Jpeg, Self::Png, Self::Gif];
    /// 格式由 core 识别，文件扩展名不决定筛选结果。
    pub fn accepts(self, format: ImageFormat) -> bool {
        self == Self::All
            || matches!(
                (self, format),
                (Self::Jpeg, ImageFormat::Jpeg)
                    | (Self::Png, ImageFormat::Png)
                    | (Self::Gif, ImageFormat::Gif)
            )
    }
}

/// 原生图库去掉领域兼容 data URL，保留真实字节与可修复错误。
#[derive(Clone)]
pub(super) struct ImageEntry {
    pub id: String,
    pub filename: String,
    pub format: ImageFormat,
    pub source: Result<MonitorImageSource, HookError>,
    pub views: ImageViews,
    pub residency: Rc<ImageResidency>,
}

/// 每个图像可同时用于图库与多个选择器，完整帧结束后统一判断是否仍可见。
#[derive(Default)]
pub(super) struct ImageResidency {
    pub requested: Cell<bool>,
    pub cleanup_pending: Cell<bool>,
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
    pub draft: AiProfileDraft,
    pub revision: u64,
    pub inputs: Option<[Entity<TextareaState>; 4]>,
    pub reset_inputs: bool,
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
    ImagesUploaded { count: usize },
    UploadedAndSelected { filename: String },
}

/// 页面保留查询、保存与导入任务直到回执，页面切换不撤销已接受写入。
#[derive(Default)]
pub(super) struct Catalog {
    pub frame_epoch: Cell<u64>,
    pub editors: Vec<Editor>,
    pub profiles_loaded: bool,
    pub profiles_loading: bool,
    pub profiles_error: Option<HookError>,
    pub images: Vec<ImageEntry>,
    pub retired_images: Vec<ImageEntry>,
    pub images_loaded: bool,
    pub images_loading: bool,
    pub images_error: Option<HookError>,
    pub selected: Option<AiTool>,
    pub category: Category,
    pub picker: Option<HookBehavior>,
    pub picker_category: Category,
    pub feedback: Option<Result<CatalogSuccess, HookError>>,
    pub saved_tool: Option<AiTool>,
    pub saving: bool,
    pub uploading: bool,
    pub deleting: Option<String>,
    pub profile_query: u64,
    pub image_query: u64,
    pub profile_task: Option<Task<()>>,
    pub image_task: Option<Task<()>>,
    pub save_task: Option<Task<()>>,
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
