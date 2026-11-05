//! 手动检查更新的状态与结果显示；只在用户点击后出站，无后台轮询。

use super::super::Shell;
use gpui_kit::{Context, Task};
use loki_metris_core::update_check::{self, UpdateFinding};

/// 检查结果的三态展示。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) enum UpdateCheckOutcome {
    /// 已是最新版本。
    UpToDate,
    /// 发现新版本，携带最新标签原文。
    Available(String),
    /// 检查失败（网络、解析或服务不可用）。
    Failed,
}

/// 根实体保留一次检查与最近结果；切换页面不取消已经接受的检查。
#[derive(Default)]
pub(in crate::app) struct UpdateCheckState {
    /// 检查在途。
    pub pending: bool,
    /// 最近一次结果。
    pub outcome: Option<UpdateCheckOutcome>,
    /// 检查任务。
    task: Option<Task<()>>,
}

impl Shell {
    /// 用户点击检查更新：服务端有界读取 GitHub 最新发布并与当前构建比较。
    pub(super) fn check_for_update(&mut self, cx: &mut Context<Self>) {
        // 服务未就绪时不出站，直接报告失败
        let fetch = self
            .dashboard
            .service
            .clone()
            .map(|service| move || service.check_latest_release().ok());
        self.check_for_update_using(fetch, env!("CARGO_PKG_VERSION"), cx);
    }

    /// 请求期间防止重复；比较在后台完成，结果回投根实体。
    /// fetch 返回 None 表示服务未就绪或检查不可用。
    fn check_for_update_using(
        &mut self,
        fetch: Option<
            impl FnOnce() -> Option<tokio::sync::oneshot::Receiver<Result<String, &'static str>>>
            + Send
            + 'static,
        >,
        current: &'static str,
        cx: &mut Context<Self>,
    ) {
        if self.update_check.pending {
            return;
        }
        self.update_check.pending = true;
        self.update_check.outcome = None;
        let work = cx.background_executor().spawn(async move {
            // 服务不可用或通道关闭都归为失败
            (|| async {
                let receiver = (fetch?)()?;
                receiver
                    .await
                    .ok()?
                    .ok()
                    .map(|tag| update_check::compare(current, &tag))
            })()
            .await
            .unwrap_or(None)
            .map(|finding| match finding {
                UpdateFinding::UpToDate => UpdateCheckOutcome::UpToDate,
                UpdateFinding::Available(tag) => UpdateCheckOutcome::Available(tag),
            })
            .unwrap_or(UpdateCheckOutcome::Failed)
        });
        self.update_check.task = Some(cx.spawn(async move |entity, cx| {
            let outcome = work.await;
            let _ = entity.update(cx, |this, cx| {
                this.update_check.pending = false;
                this.update_check.outcome = Some(outcome);
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// 结果行的文案键与内容；None 表示尚未检查。
    pub(in crate::app) fn update_check_text(&self) -> Option<(&'static str, String)> {
        match &self.update_check.outcome {
            Some(UpdateCheckOutcome::UpToDate) => {
                Some(("settings.update_check_up_to_date", self.version()))
            }
            Some(UpdateCheckOutcome::Available(tag)) => Some((
                "settings.update_check_available",
                format!("{} {}", tag, loki_metris_core::update_check::RELEASES_PAGE),
            )),
            Some(UpdateCheckOutcome::Failed) => {
                Some(("settings.update_check_failed", String::new()))
            }
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;
    use tokio::sync::oneshot;

    /// 注入回执驱动三态：发现新版本、已是最新与失败回退。
    #[gpui_kit::test]
    fn update_check_states_follow_injected_receipts(cx: &mut TestAppContext) {
        let (_, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
        // 发现新版本
        let (sender, receiver) = oneshot::channel();
        sender.send(Ok("v0.14.0-20261105".into())).unwrap();
        shell.update(cx, |this, cx| {
            let fetch = move || Some(receiver);
            this.check_for_update_using(Some(fetch), "0.13.0", cx);
            assert!(this.update_check.pending);
        });
        cx.run_until_parked();
        shell.update(cx, |this, _| {
            assert_eq!(
                this.update_check.outcome,
                Some(super::UpdateCheckOutcome::Available("v0.14.0-20261105".into()))
            );
            assert!(this.update_check_text().is_some());
        });
        // 已是最新：相同标签
        let (sender, receiver) = oneshot::channel();
        sender.send(Ok("v0.13.0-20261105".into())).unwrap();
        shell.update(cx, |this, cx| {
            let fetch = move || Some(receiver);
            this.check_for_update_using(Some(fetch), "0.13.0", cx);
        });
        cx.run_until_parked();
        shell.update(cx, |this, _| {
            assert_eq!(
                this.update_check.outcome,
                Some(super::UpdateCheckOutcome::UpToDate)
            );
        });
        // 服务未就绪（fetch 为 None）与坏标签都归为失败
        shell.update(cx, |this, cx| {
            let nothing: Option<fn() -> Option<oneshot::Receiver<Result<String, &'static str>>>> =
                None;
            this.check_for_update_using(nothing, "0.13.0", cx);
        });
        cx.run_until_parked();
        shell.update(cx, |this, _| {
            assert_eq!(
                this.update_check.outcome,
                Some(super::UpdateCheckOutcome::Failed)
            );
        });
        let (sender, receiver) = oneshot::channel();
        sender.send(Ok("autumn".into())).unwrap();
        shell.update(cx, |this, cx| {
            let fetch = move || Some(receiver);
            this.check_for_update_using(Some(fetch), "0.13.0", cx);
        });
        cx.run_until_parked();
        shell.update(cx, |this, _| {
            assert_eq!(
                this.update_check.outcome,
                Some(super::UpdateCheckOutcome::Failed)
            );
            // 请求完成后防重复标记复位
            assert!(!this.update_check.pending);
        });
    }
}
