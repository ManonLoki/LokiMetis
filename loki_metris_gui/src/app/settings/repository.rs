//! GitHub按钮独立拥有系统打开回执，页面表面不代理出站动作。

use super::super::Shell;
use gpui_kit::{Context, Task};

/// 根实体保留一次系统请求与失败状态，切换页面不取消已经接受的打开。
#[derive(Default)]
pub(in crate::app) struct RepositoryState {
    pub pending: bool,
    pub failed: bool,
    task: Option<Task<()>>,
}

impl Shell {
    /// 固定历史源码入口由原生API在后台打开，按钮本身拥有此动作。
    pub(super) fn open_repository(&mut self, cx: &mut Context<Self>) {
        self.open_repository_using(crate::native::links::open_repository, cx);
    }

    /// 请求期间防止重复，下一次请求清除旧错误；仅系统返回接受才报告成功。
    fn open_repository_using(
        &mut self,
        open: impl FnOnce() -> bool + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.repository.pending {
            return;
        }
        self.repository.pending = true;
        self.repository.failed = false;
        let work = cx.background_executor().spawn(async move { open() });
        self.repository.task = Some(cx.spawn(async move |entity, cx| {
            let opened = work.await;
            let _ = entity.update(cx, |this, cx| {
                this.repository.pending = false;
                this.repository.failed = !opened;
                cx.notify();
            });
        }));
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;
    /// 失败回执可见且下次请求清除错误；父级渲染不自动打开浏览器。
    #[gpui_kit::test]
    fn repository_failure_retries_from_root_entity(cx: &mut TestAppContext) {
        let (_, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
        shell.update(cx, |this, cx| this.open_repository_using(|| false, cx));
        cx.run_until_parked();
        assert!(shell.read_with(cx, |this, _| this.repository.failed
            && !this.repository.pending));
        shell.update(cx, |this, cx| {
            this.open_repository_using(|| true, cx);
            assert!(!this.repository.failed);
        });
        cx.run_until_parked();
        assert!(shell.read_with(cx, |this, _| !this.repository.failed
            && !this.repository.pending));
    }
}
