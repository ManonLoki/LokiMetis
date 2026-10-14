// 宿主运行时所有权：监视任务、handler、在途操作与变更代次的作用域守卫。
// 本文件由 skins/mod.rs include! 并入，保留同一模块作用域。

/// 持续监视宿主新页面并补注入的后台任务句柄，含取消信号、CDP handler 中止句柄与回收 owner。
struct WatchTask {
    host: SkinHostKind,
    cancel: watch::Sender<bool>,
    join: Option<JoinHandle<Result<usize, AppError>>>,
    handler_abort: Option<AbortHandle>,
    endpoint: CdpEndpoint,
    owner: StdWeak<StdMutex<WatchTaskReaper>>,
}

/// 保存已经离开运行态、但尚未完成终态确认或端点清理的监视任务。
struct RetainedWatchTask {
    host: SkinHostKind,
    endpoint: CdpEndpoint,
    /// `None` 表示任务已经确认终止，只剩端点清理需要在后续操作重试。
    join: Option<JoinHandle<Result<usize, AppError>>>,
    /// watcher 终态前需要一并中止的 CDP handler。
    handler_abort: Option<AbortHandle>,
    /// 正常生命周期仍存在时把取消中的句柄归还原 service owner。
    owner: StdWeak<StdMutex<WatchTaskReaper>>,
}

#[derive(Default)]
/// 由 `SkinService` 持有的监视任务回收器；同步锁只保护短暂的句柄移交。
struct WatchTaskReaper {
    retained: Vec<RetainedWatchTask>,
    /// 已构造且任务 future 尚未到终态的 watcher 取消发送端。
    active: HashMap<u64, watch::Sender<bool>>,
    /// 下一个 active watcher 登记 ID。
    next_registration_id: u64,
    shutting_down: bool,
    /// `SkinService::drop` 已开始，后续迟到句柄必须交给进程级 owner。
    service_dropped: bool,
    /// 首次应用退出事件确定的总截止；重复退出事件不得重新获得完整预算。
    shutdown_deadline: Option<tokio::time::Instant>,
}

/// watcher future 持有的 active 登记守卫；终态或 abort 都会从 service owner 注销。
struct ActiveWatchRegistration {
    registration_id: Option<u64>,
    owner: Arc<StdMutex<WatchTaskReaper>>,
}

/// 持有一个 CDP handler 任务；丢弃或显式中止时把任务交给进程级回收，避免句柄丢失。
struct HandlerTaskGuard(Option<JoinHandle<()>>);

impl HandlerTaskGuard {
    #[cfg(test)]
    /// 为隔离测试登记容量并接管手工构造的 handler。
    fn new(task: JoinHandle<()>) -> Self {
        reserve_handler_task_capacity()
            .unwrap_or_else(|error| panic!("handler fixture capacity unavailable: {}", error.code))
            .bind(task)
    }

    /// 中止 handler 任务并移交进程级 owner 确认终态；重复调用无副作用。
    fn abort(&mut self) {
        if let Some(task) = self.0.take() {
            retain_aborted_handler_task(task);
        }
    }

    /// 中止旧 handler，并从另一个 guard 原子接管替代任务。
    fn replace(&mut self, mut replacement: Self) {
        self.abort();
        self.0 = replacement.0.take();
    }

    /// 已结束任务不能被登记为运行中的 watcher。
    fn is_finished(&self) -> bool {
        self.0.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// 为 watcher owner 复制独立取消能力，同时保留原 JoinHandle 所有权。
    fn abort_handle(&self) -> Option<AbortHandle> {
        self.0
            .as_ref()
            .filter(|task| !task.is_finished())
            .map(JoinHandle::abort_handle)
    }
}

impl Drop for HandlerTaskGuard {
    /// 守卫释放时中止仍在运行的 handler。
    fn drop(&mut self) {
        self.abort();
    }
}

/// 当前在途的宿主操作：操作代次与取消信号。
struct CodexOperation {
    id: u64,
    cancel: watch::Sender<bool>,
}

/// 在途宿主操作的作用域守卫；结束时只清除仍属于自己的操作登记。
struct CodexOperationGuard<'a> {
    id: u64,
    active: &'a StdMutex<Option<CodexOperation>>,
}

impl Drop for CodexOperationGuard<'_> {
    /// 仅当登记的仍是本操作时清除，避免误清后续操作。
    fn drop(&mut self) {
        let Ok(mut active) = self.active.lock() else {
            return;
        };
        if active
            .as_ref()
            .is_some_and(|operation| operation.id == self.id)
        {
            *active = None;
        }
    }
}

/// 在宿主变更期间把运行代次保持为奇数，结束（包括错误返回）后恢复为新的偶数代次。
struct HostRuntimeMutationGuard<'a> {
    generation: &'a AtomicU64,
}

impl Drop for HostRuntimeMutationGuard<'_> {
    /// 结束宿主变更并使所有变更前或变更中的异步探针失效。
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }
}
