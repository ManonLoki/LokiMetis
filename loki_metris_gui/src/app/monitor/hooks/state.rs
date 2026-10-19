//! Hooks页面保留各工具目录草稿、原生输入和操作回执，不推断安装状态。

use gpui_kit::{Entity, Subscription, Task, component::input::InputState};
use loki_metris_core::{AiTool, HookConfigLocation, HookConfigWriteResult, HookError};

/// 成功只来自实际writer回执，错误直接映射稳定领域码。
pub(crate) enum Feedback {
    /// 写入成功，附写入结果。
    Written(HookConfigWriteResult),
    /// 写入失败。
    Failed(HookError),
}

/// 一个工具的草稿在切换标签后保留，文件路径输入为可复制的只读投影。
pub(crate) struct Editor {
    /// 最近一次读取或保存成功的配置位置。
    pub saved: HookConfigLocation,
    /// 目录输入框的草稿。
    pub draft: String,
    /// 路径输入框；首次进入时创建。
    pub inputs: Option<Vec<Entity<InputState>>>,
    /// 下一帧需要用已保存值重写输入框。
    pub reset: bool,
    /// 目录保存在途。
    pub saving: bool,
    /// 仅标记已接受的目录保存动作，两个按钮各自显示对应 loading。
    pub saving_default: bool,
    /// Hook 配置写入在途。
    pub writing: bool,
    /// 系统目录选择框打开中。
    pub picking: bool,
    /// 输入变化的订阅。
    pub subscriptions: Vec<Subscription>,
    /// 目录保存任务。
    pub save_task: Option<Task<()>>,
    /// 配置写入任务。
    pub write_task: Option<Task<()>>,
    /// 目录选择任务。
    pub pick_task: Option<Task<()>>,
}
impl Editor {
    /// 首次定位是草稿唯一初值；重复查询不会覆盖用户输入。
    fn new(saved: HookConfigLocation) -> Self {
        Self {
            draft: saved.directory.clone(),
            saved,
            inputs: None,
            reset: true,
            saving: false,
            saving_default: false,
            writing: false,
            picking: false,
            subscriptions: Vec::new(),
            save_task: None,
            write_task: None,
            pick_task: None,
        }
    }
    /// 只在提交时忽略外侧空白，与宿主路径校验一致。
    pub fn dirty(&self) -> bool {
        self.draft.trim() != self.saved.directory
    }
}

/// 已接受的操作任务跨标签保留，各工具忙状态互不影响。
#[derive(Default)]
pub(in crate::app::monitor) struct HooksView {
    /// 各 Agent 的 Hook 配置编辑器。
    pub editors: Vec<Editor>,
    /// 当前选中的 Agent。
    pub selected: Option<AiTool>,
    /// 配置位置至少成功读取过一次。
    pub loaded: bool,
    /// 读取在途。
    pub loading: bool,
    /// 读取失败的原因。
    pub error: Option<HookError>,
    /// 最近一次写入的反馈及所属 Agent。
    pub feedback: Option<(AiTool, Feedback)>,
    /// 反馈代次；新反馈或切换时递增，迟到的目录选择不覆盖较新反馈。
    pub feedback_revision: u64,
    /// 读取代次；旧读取结果被丢弃。
    pub query: u64,
    /// 读取任务。
    pub query_task: Option<Task<()>>,
}
impl HooksView {
    /// 保存开始和成功回执都淘汰旧查询，避免交叉写入回执触发的刷新倒退路径。
    pub fn invalidate_query(&mut self) {
        self.query = self.query.wrapping_add(1);
        self.query_task.take();
        self.loading = false;
    }
    /// 权威路径始终更新，保存期间的后续输入保留为新草稿。
    pub fn finish_directory_save(
        &mut self,
        tool: AiTool,
        submitted_draft: &str,
        feedback_revision: u64,
        result: Result<HookConfigLocation, HookError>,
    ) {
        if result.is_ok() {
            self.invalidate_query();
        }
        if let Some(editor) = self.editor_mut(tool) {
            editor.saving = false;
            editor.saving_default = false;
            match result {
                Ok(location) => {
                    if editor.draft == submitted_draft {
                        editor.draft = location.directory.clone();
                        editor.reset = true;
                    }
                    editor.saved = location;
                }
                Err(error) => {
                    self.accept_feedback(feedback_revision, tool, Feedback::Failed(error))
                }
            }
        }
    }
    /// 编辑、标签切换与新操作使旧异步提示失效，但不撤销已经接受的写入。
    pub fn clear_feedback(&mut self) -> u64 {
        self.feedback_revision = self.feedback_revision.wrapping_add(1);
        self.feedback = None;
        self.feedback_revision
    }
    /// 旧操作仍完成自身状态收尾，只有当前观察代数能发布提示。
    pub fn accept_feedback(&mut self, revision: u64, tool: AiTool, feedback: Feedback) {
        if self.feedback_revision == revision {
            self.feedback = Some((tool, feedback));
        }
    }
    /// 新快照更新路径投影；已有草稿与输入实体保持不变。
    pub fn accept_locations(&mut self, locations: Vec<HookConfigLocation>) {
        for location in locations {
            if let Some(editor) = self
                .editors
                .iter_mut()
                .find(|editor| editor.saved.tool == location.tool)
            {
                editor.saved = location;
            } else {
                self.editors.push(Editor::new(location));
            }
        }
        self.loaded = true;
        self.error = None;
    }
    /// 每个回执都按工具定位，不能更新当前另一个标签。
    pub fn editor_mut(&mut self, tool: AiTool) -> Option<&mut Editor> {
        self.editors
            .iter_mut()
            .find(|editor| editor.saved.tool == tool)
    }
}
