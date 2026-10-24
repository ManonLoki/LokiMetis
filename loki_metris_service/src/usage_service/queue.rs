//! 有界请求邮箱：活动请求合作取消，只保留最新待处理意图。

use loki_metris_host::local_index::CancellationToken;
use loki_metris_core::SourceClientKind;
use std::{future::Future, sync::{Arc, Condvar, Mutex}, time::Duration};
use tokio::sync::{Notify, oneshot};

/// 请求携带纯值；异步结果通过一次性通道返回所属界面。
#[derive(Clone, Copy)]
pub(super) struct Intent {
    /// 要扫描的 Agent；None 表示全部。
    pub client: Option<SourceClientKind>,
    /// 扫描的触发来源。
    pub origin: loki_metris_core::ScanStartOrigin,
}

/// 独立请求没有隐式 detach 句柄；丢弃前台接收端不会生成额外扫描。
struct Pending<I, O> {
    /// 请求输入。
    intent: I,
    /// 结果回执发送端。
    reply: oneshot::Sender<Result<O, &'static str>>,
}

/// 一把锁线性化提交、领取、取消及退出，避免取消落到下一次请求。
struct State<I, O> {
    /// 最新的待办请求；新提交会取代它。
    pending: Option<Pending<I, O>>,
    /// 在途请求的取消句柄。
    active: Option<CancellationToken>,
    /// 已禁止新提交。
    stopped: bool,
    /// worker 已退出，不再执行任何请求。
    finished: bool,
}

/// 唯一 worker 拥有执行顺序；待处理槽的容量永远是一个。
pub(super) struct Queue<I, O> {
    /// 队列状态。
    state: Mutex<State<I, O>>,
    /// 有新请求时唤醒 worker。
    wake: Notify,
    /// worker 退出时唤醒等待者。
    drained: Condvar,
}

impl<I, O> Default for Queue<I, O> {
    /// 泛型邮箱默认不要求请求和响应本身可构造默认业务值。
    fn default() -> Self {
        Self { state: Mutex::new(State { pending: None, active: None, stopped: false, finished: false }),
            wake: Notify::new(), drained: Condvar::new() }
    }
}

impl<I, O> Queue<I, O> {
    /// 覆盖旧待处理意图并请求活动扫描合作停下。
    pub fn submit(&self, intent: I) -> Result<oneshot::Receiver<Result<O, &'static str>>, &'static str> {
        self.submit_inner(intent, true)
    }

    /// writer 新意图不取消当前扫描，保持显式批次的完整收尾。
    pub fn submit_without_cancelling(&self, intent: I) -> Result<oneshot::Receiver<Result<O, &'static str>>, &'static str> {
        self.submit_inner(intent, false)
    }

    /// 周期检查与提交在同一把锁内完成，不覆盖已排队的显式意图。
    pub fn try_submit_if_idle(&self, intent: I) -> Result<bool, &'static str> {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.stopped { return Err("usage_service_stopped") }
        if state.pending.is_some() || state.active.is_some() { return Ok(false) }
        let (reply, _receiver) = oneshot::channel();
        state.pending = Some(Pending { intent, reply });
        self.wake.notify_one();
        Ok(true)
    }

    /// 同一同步边界处理不同邮箱的取消策略。
    fn submit_inner(&self, intent: I, cancel_active: bool) -> Result<oneshot::Receiver<Result<O, &'static str>>, &'static str> {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.stopped { return Err("usage_service_stopped") }
        if cancel_active && let Some(active) = &state.active { active.cancel(); }
        let (reply, receiver) = oneshot::channel();
        state.pending = Some(Pending { intent, reply });
        self.wake.notify_one();
        Ok(receiver)
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

    /// 正常取消仍等待业务收尾；每次请求的 panic 不会关闭整个 worker。
    pub async fn run<F, Fut>(self: Arc<Self>, mut load: F)
    where F: FnMut(I, CancellationToken) -> Fut,
        Fut: Future<Output = Result<O, &'static str>>,
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
            let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| load(pending.intent, token))) {
                Ok(future) => guarded(future).await,
                Err(_) => Err("usage_worker_failed"),
            };
            let _ = pending.reply.send(result);
            self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).active = None;
        }
    }
}

/// 仅捕获一次业务 future 的 poll，保留取消收尾与队列的所有权。
async fn guarded<O>(future: impl Future<Output = Result<O, &'static str>>) -> Result<O, &'static str> {
    let mut future = std::pin::pin!(future);
    std::future::poll_fn(move |cx| {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| future.as_mut().poll(cx))) {
            Ok(poll) => poll,
            Err(_) => std::task::Poll::Ready(Err("usage_worker_failed")),
        }
    }).await
}

/// 正常返回或 panic 都释放退出等待者；panic 仍由运行时记录。
struct Exit<I, O>(Arc<Queue<I, O>>);
impl<I, O> Drop for Exit<I, O> {
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
        let queue: Arc<Queue<Intent, ()>> = Arc::new(Queue::default());
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
        let request = |client| Intent { client: Some(client), origin: loki_metris_core::ScanStartOrigin::ExplicitUser };
        let first = queue.submit(request(SourceClientKind::Codex)).unwrap();
        assert_eq!(start_receiver.recv_timeout(Duration::from_secs(2)).unwrap(), Some(SourceClientKind::Codex));
        let obsolete = queue.submit(request(SourceClientKind::ClaudeCode)).unwrap();
        let latest = queue.submit(request(SourceClientKind::Grok)).unwrap();
        assert!(runtime.block_on(obsolete).is_err());
        release.send(()).unwrap();
        assert_eq!(runtime.block_on(first).unwrap().err(), Some("test_result"));
        assert_eq!(runtime.block_on(latest).unwrap().err(), Some("test_result"));
        assert_eq!(start_receiver.recv_timeout(Duration::from_secs(2)).unwrap(), Some(SourceClientKind::Grok));
        assert!(queue.stop(Duration::from_secs(2)));
        assert!(queue.submit(request(SourceClientKind::Codex)).is_err());
    }

    /// 退出等待活动请求的取消收尾，而不是直接终止异步任务。
    #[test]
    fn shutdown_waits_for_cooperative_cleanup() {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let queue: Arc<Queue<Intent, ()>> = Arc::new(Queue::default());
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
        let _receiver = queue.submit(Intent { client: None, origin: loki_metris_core::ScanStartOrigin::ExplicitUser }).unwrap();
        start_receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(queue.stop(Duration::from_secs(2)));
        assert!(cleaned.load(std::sync::atomic::Ordering::Acquire));
    }
    /// 单次业务 panic 返回失败，后续请求仍由同一个 worker 正常处理。
    #[test]
    fn panic_isolated_and_periodic_never_replaces_explicit_pending() {
        let runtime=tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let queue:Arc<Queue<u8,u8>>=Arc::new(Queue::default());
        let first=queue.submit_without_cancelling(1).unwrap();
        assert!(!queue.try_submit_if_idle(3).unwrap());
        let worker=queue.clone();
        runtime.spawn(async move {worker.run(|value,_|async move {if value==1 {panic!("test isolated panic")};Ok(value)}).await;});
        assert_eq!(runtime.block_on(first).unwrap(),Err("usage_worker_failed"));
        let next=queue.submit(2).unwrap();
        assert_eq!(runtime.block_on(next).unwrap(),Ok(2));
        assert!(queue.stop(Duration::from_secs(2)));
    }

}
