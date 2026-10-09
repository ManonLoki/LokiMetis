//! 通知开关由应用级 owner 持有；授权回调只传值，落盘回执才更新已确认状态。

#[cfg(target_os = "macos")]
mod macos;
#[cfg(test)]
mod tests;

use crate::preferences::{PreferencePatch, Writer};
use futures::{
    FutureExt as _,
    future::{Either, select},
};
use gpui_kit::{App, Task};
use std::{
    cell::RefCell,
    future::Future,
    rc::Rc,
    time::{Duration, Instant},
};

/// 平台权限与保存失败保留稳定类别；设置页统一提示，失败保持原有本地开关。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Denied,
    Unavailable,
    Timeout,
    Save,
}

/// 本地偏好是展示权威；启动不查询或申请权限，也不隐式重写已经保存的值。
#[derive(Clone, Debug, Default)]
pub struct State {
    pub enabled: bool,
    pub busy: bool,
    pub desired: Option<bool>,
    pub error: Option<Error>,
}

/// 唯一前台任务拥有操作代次；退出先失效，再排空已经接受的偏好写入。
struct Owner {
    state: State,
    generation: u64,
    closing: bool,
    task: Option<Task<()>>,
}

/// 克隆仅共享应用级状态，不依赖任何页面 Entity 的生命周期。
#[derive(Clone)]
pub struct Service {
    owner: Rc<RefCell<Owner>>,
    writer: Writer,
}

impl Service {
    /// 初始化只接收已读取的持久值；没有系统通知或授权副作用。
    pub fn new(enabled: bool, writer: Writer) -> Self {
        Self {
            owner: Rc::new(RefCell::new(Owner {
                state: State {
                    enabled,
                    ..Default::default()
                },
                generation: 0,
                closing: false,
                task: None,
            })),
            writer,
        }
    }

    /// 渲染只复制本地快照，不触发系统查询。
    pub fn state(&self) -> State {
        self.owner.borrow().state.clone()
    }

    /// 只有实际开关事件可以授权；关闭与等值操作都不请求系统权限。
    pub fn set(&self, enabled: bool, cx: &mut App) {
        let executor = cx.background_executor().clone();
        self.set_using(
            enabled,
            move |deadline| async move {
                #[cfg(target_os = "macos")]
                {
                    macos::authorize(executor, deadline).await
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = (executor, deadline);
                    Ok(())
                }
            },
            cx,
        );
    }

    /// 授权阶段有界；保存一旦接受就等待真实回执，不用超时伪装已取消的写入。
    fn set_using<F: Future<Output = Result<(), Error>> + 'static>(
        &self,
        enabled: bool,
        authorize: impl FnOnce(Instant) -> F + 'static,
        cx: &mut App,
    ) {
        let generation = {
            let mut owner = self.owner.borrow_mut();
            if owner.closing || owner.state.busy || owner.state.enabled == enabled {
                return;
            }
            owner.generation += 1;
            owner.state.busy = true;
            owner.state.desired = Some(enabled);
            owner.state.error = None;
            owner.generation
        };
        let executor = cx.background_executor().clone();
        let deadline = executor.now() + Duration::from_secs(10);
        let weak = Rc::downgrade(&self.owner);
        let writer = self.writer.clone();
        let task = cx.spawn(async move |cx| {
            let permission = if executor.now() >= deadline {
                Err(Error::Timeout)
            } else if enabled {
                match select(
                    authorize(deadline).boxed_local(),
                    executor
                        .timer(deadline.saturating_duration_since(executor.now()))
                        .boxed_local(),
                )
                .await
                {
                    Either::Left((result, _)) => result,
                    Either::Right(_) => Err(Error::Timeout),
                }
            } else {
                Ok(())
            };
            let Some(owner) = weak.upgrade() else { return };
            if !current(&owner.borrow(), generation) {
                return;
            }
            let receipt = match permission {
                Ok(()) if executor.now() >= deadline => Err(Error::Timeout),
                Ok(()) => writer
                    .mutate(PreferencePatch::SystemNotification(enabled))
                    .map_err(|_| Error::Save),
                Err(error) => Err(error),
            };
            let result = match receipt {
                Ok(mut receipt) => loop {
                    match receipt.try_recv() {
                        Ok(Ok(PreferencePatch::SystemNotification(actual)))
                            if actual == enabled =>
                        {
                            break Ok(());
                        }
                        Ok(_) | Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                            break Err(Error::Save);
                        }
                        Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
                    }
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(20))
                        .await;
                },
                Err(error) => Err(error),
            };
            let mut state = owner.borrow_mut();
            if !current(&state, generation) {
                return;
            }
            state.state.busy = false;
            state.state.desired = None;
            match result {
                Ok(()) => state.state.enabled = enabled,
                Err(error) => state.state.error = Some(error),
            }
            drop(state);
            cx.update(|cx| {
                if result == Err(Error::Denied) {
                    open_settings(cx);
                }
                cx.refresh_windows();
            });
        });
        self.owner.borrow_mut().task = Some(task);
        cx.refresh_windows();
    }

    /// 退出同步取消权限任务；系统迟到回调最多发送到已断开的 oneshot。
    pub fn stop(&self) {
        let mut owner = self.owner.borrow_mut();
        owner.closing = true;
        owner.generation += 1;
        owner.task.take();
        owner.state.busy = false;
        owner.state.desired = None;
    }
}

/// 授权和回执都必须仍属于当前应用操作，不能在退出后更新或开始写入。
fn current(owner: &Owner, generation: u64) -> bool {
    !owner.closing && owner.generation == generation
}

/// 拒绝时打开当前产品的通知设置，与参考应用一致；关闭不会撤销系统授权。
fn open_settings(cx: &App) {
    #[cfg(all(target_os = "macos",not(test)))]
    cx.open_url("x-apple.systempreferences:com.apple.Notifications-Settings.extension?id=com.loki-metris.app");
    #[cfg(any(not(target_os = "macos"), test))]
    let _ = cx;
}
