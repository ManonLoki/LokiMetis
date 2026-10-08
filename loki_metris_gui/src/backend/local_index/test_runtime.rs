//! 测试共用长驻 Tokio runtime，跨调用保留 SQLite 池后台任务。

use std::{future::Future, sync::OnceLock};

/// 隔离夹具的同步测试可以多次等待同一池，不在每次调用后销毁 runtime。
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| tokio::runtime::Builder::new_multi_thread().enable_all()
        .worker_threads(2).thread_stack_size(16 * 1024 * 1024).build().expect("测试运行时启动"))
        .block_on(future)
}
