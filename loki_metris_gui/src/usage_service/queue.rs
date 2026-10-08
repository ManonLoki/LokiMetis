//! 有界请求邮箱：活动请求合作取消，只保留最新待处理意图。

use super::UsageData;
use crate::backend::local_index::CancellationToken;
use loki_metris_core::SourceClientKind;
use std::{future::Future, sync::{Arc, Condvar, Mutex}, time::Duration};
use tokio::sync::{Notify, oneshot};

/// 请求携带纯值；异步结果通过一次性通道返回所属界面。
#[derive(Clone, Copy)]
pub(super) struct Intent { pub client: Option<SourceClientKind>, pub scan: bool }

/// 独立请求没有隐式 detach 句柄；丢弃前台接收端不会生成额外扫描。
struct Pending { intent: Intent, reply: oneshot::Sender<Result<UsageData, &'static str>> }

/// 一把锁线性化提交、领取、取消及退出，避免取消落到下一次请求。
#[derive(Default)]
struct State {
    pending: Option<Pending>,
    active: Option<CancellationToken>,
    stopped: bool,
    finished: bool,
}

/// 唯一 worker 拥有执行顺序；待处理槽的容量永远是一个。
#[derive(Default)]
pub(super) struct Queue { state: Mutex<State>, wake: Notify, drained: Condvar }

impl Queue {
    /// 覆盖旧待处理意图并请求活动扫描合作停下。
    pub fn submit(&self, intent: Intent) -> Result<oneshot::Receiver<Result<UsageData, &'static str>>, &'static str> {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.stopped { return Err("usage_service_stopped") }
        if let Some(active) = &state.active { active.cancel(); }
        let (reply, receiver) = oneshot::channel();
        state.pending = Some(Pending { intent, reply });
        self.wake.notify_one();
        Ok(receiver)
    }

    /// 清空尚未启动的请求；已启动任务继续执行扫描收尾。
    pub fn cancel(&self) {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.pending.take();
        if let Some(active) = &state.active { active.cancel(); }
    }

    /// 禁止新提交后唤醒 worker，等待活动扫描完成取消收尾。
    pub fn stop(&self, timeout: Duration) -> bool {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.stopped = true;
        state.pending.take();
        if let Some(active) = &state.active { active.cancel(); }
        self.wake.notify_one();
        let (state, _) = self.drained.wait_timeout_while(state, timeout, |state| !state.finished)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.finished
    }

    /// worker 在取消后仍 await 业务 future；不会中途丢弃 SQLite 扫描收尾。
    pub async fn run<F, Fut>(self: Arc<Self>, mut load: F)
    where F: FnMut(Intent, CancellationToken) -> Fut,
        Fut: Future<Output = Result<UsageData, &'static str>>,
    {
        let _exit = Exit(self.clone());
        loop {
            let notified = self.wake.notified();
            let work = {
                let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if state.stopped { return }
                state.pending.take().map(|pending| {
                    let token = CancellationToken::new();
                    state.active = Some(token.clone());
                    (pending, token)
                })
            };
            let Some((pending, token)) = work else { notified.await; continue };
            let result = load(pending.intent, token).await;
            let _ = pending.reply.send(result);
            self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).active = None;
        }
    }
}

/// 正常返回或 panic 都释放退出等待者；panic 仍由运行时记录。
struct Exit(Arc<Queue>);
impl Drop for Exit {
    /// worker 已经不再执行任何业务 future。
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.finished = true;
        state.stopped = true;
        state.active = None;
        state.pending.take();
        self.0.drained.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 连续切换只执行活动请求和最后一次意图，取消仍保留业务收尾。
    #[test]
    fn coalesces_pending_requests_and_drains_active_work() {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let queue = Arc::new(Queue::default());
        let (started, start_receiver) = std::sync::mpsc::channel();
        let (release, release_receiver) = oneshot::channel();
        let mut release_receiver = Some(release_receiver);
        let worker = queue.clone();
        runtime.spawn(async move {
            worker.run(move |intent, cancel| {
                let first = release_receiver.take();
                let started = started.clone();
                async move {
                    started.send(intent.client).unwrap();
                    if let Some(first) = first {
                        let _ = first.await;
                        assert!(cancel.is_cancelled());
                    }
                    Err("test_result")
                }
            }).await;
        });
        let request = |client| Intent { client: Some(client), scan: true };
        let first = queue.submit(request(SourceClientKind::Codex)).unwrap();
        assert_eq!(start_receiver.recv_timeout(Duration::from_secs(2)).unwrap(), Some(SourceClientKind::Codex));
        let obsolete = queue.submit(request(SourceClientKind::ClaudeCode)).unwrap();
        let latest = queue.submit(request(SourceClientKind::GrokBuildCli)).unwrap();
        assert!(runtime.block_on(obsolete).is_err());
        release.send(()).unwrap();
        assert_eq!(runtime.block_on(first).unwrap().err(), Some("test_result"));
        assert_eq!(runtime.block_on(latest).unwrap().err(), Some("test_result"));
        assert_eq!(start_receiver.recv_timeout(Duration::from_secs(2)).unwrap(), Some(SourceClientKind::GrokBuildCli));
        assert!(queue.stop(Duration::from_secs(2)));
        assert!(queue.submit(request(SourceClientKind::Codex)).is_err());
    }

    /// 退出等待活动请求的取消收尾，而不是直接终止异步任务。
    #[test]
    fn shutdown_waits_for_cooperative_cleanup() {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let queue = Arc::new(Queue::default());
        let (started, start_receiver) = std::sync::mpsc::channel();
        let cleaned = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = cleaned.clone();
        let worker = queue.clone();
        runtime.spawn(async move {
            worker.run(move |_, cancel| {
                let started = started.clone(); let flag = flag.clone();
                async move {
                    started.send(()).unwrap();
                    while !cancel.is_cancelled() { tokio::time::sleep(Duration::from_millis(1)).await; }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    flag.store(true, std::sync::atomic::Ordering::Release);
                    Err("usage_scan_cancelled")
                }
            }).await;
        });
        let _receiver = queue.submit(Intent { client: None, scan: true }).unwrap();
        start_receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(queue.stop(Duration::from_secs(2)));
        assert!(cleaned.load(std::sync::atomic::Ordering::Acquire));
    }
}
