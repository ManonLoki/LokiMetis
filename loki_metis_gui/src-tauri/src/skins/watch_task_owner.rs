/// 取得应用生命周期结束后仍需持有 watcher 句柄的进程级最终 owner。
fn process_watch_task_owner() -> &'static StdMutex<Vec<RetainedWatchTask>> {
    static OWNER: std::sync::OnceLock<StdMutex<Vec<RetainedWatchTask>>> =
        std::sync::OnceLock::new();
    OWNER.get_or_init(|| StdMutex::new(Vec::new()))
}

/// 把已中止但尚未确认终态的 watcher 交给进程级 owner，禁止丢弃 JoinHandle。
fn retain_process_watch_task(mut task: RetainedWatchTask) {
    if let Some(handler_abort) = task.handler_abort.take() {
        handler_abort.abort();
    }
    if let Some(join) = task.join.as_ref() {
        join.abort();
    }
    task.owner = StdWeak::new();
    process_watch_task_owner()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(task);
}

/// 优先把取消中的 watcher 归还仍存活的 service；最终析构后转交进程级 owner。
fn retain_abandoned_watch_task(task: RetainedWatchTask) {
    if let Some(handler_abort) = task.handler_abort.as_ref() {
        handler_abort.abort();
    }
    if let Some(join) = task.join.as_ref() {
        join.abort();
    }
    if let Some(owner) = task.owner.upgrade() {
        let mut state = owner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.service_dropped {
            state.retained.push(task);
            return;
        }
    }
    retain_process_watch_task(task);
}

impl WatchTask {
    /// 构造立即绑定 service owner 的 watcher 句柄；生产调用方不得手写未登记任务。
    fn new(
        owner: &Arc<StdMutex<WatchTaskReaper>>,
        host: SkinHostKind,
        cancel: watch::Sender<bool>,
        join: JoinHandle<Result<usize, AppError>>,
        handler_abort: AbortHandle,
        endpoint: CdpEndpoint,
    ) -> Self {
        Self {
            host,
            cancel,
            join: Some(join),
            handler_abort: Some(handler_abort),
            endpoint,
            owner: Arc::downgrade(owner),
        }
    }

    /// 构造无需 active 登记的隔离测试任务，析构仍会先中止底层 future。
    #[cfg(test)]
    fn new_test(
        host: SkinHostKind,
        cancel: watch::Sender<bool>,
        join: JoinHandle<Result<usize, AppError>>,
        handler_abort: AbortHandle,
        endpoint: CdpEndpoint,
    ) -> Self {
        Self {
            host,
            cancel,
            join: Some(join),
            handler_abort: Some(handler_abort),
            endpoint,
            owner: StdWeak::new(),
        }
    }

    /// 返回 watcher future 是否仍未到终态。
    fn is_running(&self) -> bool {
        self.join.as_ref().is_some_and(|join| !join.is_finished())
    }

    /// 发布合作取消并把 JoinHandle、handler 与 endpoint 责任移入取消守卫。
    fn into_retained(mut self) -> RetainedWatchTask {
        let _ = self.cancel.send(true);
        RetainedWatchTask {
            host: self.host,
            endpoint: self.endpoint,
            join: self.join.take(),
            handler_abort: self.handler_abort.take(),
            owner: self.owner.clone(),
        }
    }

    /// 发布合作取消并中止 watcher 与 handler；实际句柄仍由 Drop 确认或保留。
    fn abort(&self) {
        let _ = self.cancel.send(true);
        if let Some(handler_abort) = self.handler_abort.as_ref() {
            handler_abort.abort();
        }
        if let Some(join) = self.join.as_ref() {
            join.abort();
        }
    }
}

impl Drop for WatchTask {
    /// 意外取消安装 future 时中止 watcher，并把未确认终态句柄交还 owner。
    fn drop(&mut self) {
        let _ = self.cancel.send(true);
        if self.join.is_none() && self.handler_abort.is_none() {
            return;
        }
        retain_abandoned_watch_task(RetainedWatchTask {
            host: self.host,
            endpoint: self.endpoint,
            join: self.join.take(),
            handler_abort: self.handler_abort.take(),
            owner: self.owner.clone(),
        });
    }
}

impl Drop for ActiveWatchRegistration {
    /// watcher future 到终态或被 abort 时，从 active 集合原子注销。
    fn drop(&mut self) {
        let Some(registration_id) = self.registration_id.take() else {
            return;
        };
        self.owner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .active
            .remove(&registration_id);
    }
}

/// 在任意 await 取消点保护一个已离开运行态的 watcher 所有权。
struct RetainedWatchTaskGuard {
    task: Option<RetainedWatchTask>,
}

impl RetainedWatchTaskGuard {
    /// 接管 watcher 的 JoinHandle、handler 与 endpoint 清理责任。
    fn new(task: RetainedWatchTask) -> Self {
        Self { task: Some(task) }
    }

    /// 返回受保护任务的可变引用。
    fn task_mut(&mut self) -> &mut RetainedWatchTask {
        self.task.as_mut().expect("watch task guard must be armed")
    }

    /// watcher 已确认终态后中止独立 handler，再释放两层任务句柄。
    fn clear_finished_handles(&mut self) {
        let task = self.task_mut();
        if let Some(handler_abort) = task.handler_abort.take() {
            handler_abort.abort();
        }
        task.join.take();
    }

    /// 中止 watcher 与 CDP handler，但继续持有 JoinHandle 直到确认终态。
    fn abort(&mut self) {
        let task = self.task_mut();
        if let Some(handler_abort) = task.handler_abort.as_ref() {
            handler_abort.abort();
        }
        if let Some(join) = task.join.as_ref() {
            join.abort();
        }
    }

    /// 清理或正常终态已完成，解除析构时的保留责任。
    fn complete(mut self) {
        self.task.take();
    }

    /// 当前尝试未完成，把原责任放回 service owner 供下一轮继续。
    fn retain_in_service(mut self, service: &SkinService) {
        if let Some(task) = self.task.take() {
            service.retain_watch_task(task);
        }
    }
}

impl Drop for RetainedWatchTaskGuard {
    /// 外层 future 被取消时，中止并保存尚未完成的 watcher 所有权。
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            retain_abandoned_watch_task(task);
        }
    }
}
