//! GUI 意图授权作用域：只在一次操作 future 的轮询期间生效，不能泄漏到已接管的 watcher 或退出清理。

use super::*;

tokio::task_local! {
    /// 默认内部维护不附带页面授权；GUI宿主变更必须显式进入该作用域。
    static SKIN_HOST_AUTHORITY: Arc<dyn Fn() -> bool + Send + Sync>;
}

/// 作用域随future取消自动释放，跨await恢复时使用同一意图探针。
pub async fn with_host_authority<F: Future>(
    allowed: Arc<dyn Fn() -> bool + Send + Sync>,
    work: F,
) -> F::Output {
    SKIN_HOST_AUTHORITY.scope(allowed, work).await
}

/// 在串行锁之后及新的宿主副作用之前复核，已接受事务的补偿不调用本检查。
pub(super) fn check_host_authority() -> Result<(), AppError> {
    if SKIN_HOST_AUTHORITY
        .try_with(|allowed| allowed())
        .unwrap_or(true)
    {
        Ok(())
    } else {
        Err(operation_cancelled())
    }
}
