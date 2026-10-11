//! Hooks页面保留各工具目录草稿、原生输入和操作回执，不推断安装状态。

use gpui_kit::{Entity, Subscription, Task, component::input::InputState};
use loki_metris_core::{AiTool, HookConfigLocation, HookConfigWriteResult, HookError};

/// 成功只来自实际writer回执，错误直接映射稳定领域码。
pub(crate) enum Feedback {
    Written(HookConfigWriteResult),
    Failed(HookError),
}

/// 一个工具的草稿在切换标签后保留，文件路径输入为可复制的只读投影。
pub(crate) struct Editor {
    pub saved: HookConfigLocation,
    pub draft: String,
    pub inputs: Option<Vec<Entity<InputState>>>,
    pub reset: bool,
    pub saving: bool,
    /// 仅标记已接受的目录保存动作，两个按钮各自显示对应 loading。
    pub saving_default: bool,
    pub writing: bool,
    pub picking: bool,
    pub subscriptions: Vec<Subscription>,
    pub save_task: Option<Task<()>>,
    pub write_task: Option<Task<()>>,
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
    pub editors: Vec<Editor>,
    pub selected: Option<AiTool>,
    pub loaded: bool,
    pub loading: bool,
    pub error: Option<HookError>,
    pub feedback: Option<(AiTool, Feedback)>,
    pub feedback_revision: u64,
    pub query: u64,
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
