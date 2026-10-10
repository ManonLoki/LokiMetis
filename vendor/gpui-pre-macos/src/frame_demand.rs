use std::sync::atomic::{AtomicBool, Ordering};

/// 每窗口共享的帧需求；CV线程只观察，主线程取得回调后才消费，回调内新请求留到下一帧。
pub(crate) struct FrameDemand {
    pending: AtomicBool,
}

impl FrameDemand {
    /// 新窗口先无需求，由回调注册、Core失效或原生可见性事件明确唤醒。
    pub(crate) fn new() -> Self {
        Self { pending: AtomicBool::new(false) }
    }

    /// 合并重复请求；不借用App或窗口锁，绘制重入期间也能安全调用。
    pub(crate) fn request(&self) {
        self.pending.store(true, Ordering::Release);
    }

    /// CV线程仅检查需求，不提前消费，主线程暂时取走回调时也不会丢帧。
    pub(crate) fn is_pending(&self) -> bool {
        self.pending.load(Ordering::Acquire)
    }

    /// 主线程确认可执行回调后消费当前请求；此后新请求不会被帧结束清除。
    pub(crate) fn take(&self) -> bool {
        self.pending.swap(false, Ordering::AcqRel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// 静态窗口没有需求；重复唤醒只形成一个可消费请求。
    #[test]
    fn idle_and_repeated_requests_coalesce() {
        let demand = FrameDemand::new();
        assert!(!demand.is_pending());
        assert!(!demand.take());
        demand.request();
        demand.request();
        assert!(demand.is_pending());
        assert!(demand.take());
        assert!(!demand.take());
    }

    /// CV观察和暂时不可执行的回调都不消费；恢复后仍能执行原需求。
    #[test]
    fn observation_preserves_unserved_demand() {
        let demand = Arc::new(FrameDemand::new());
        let observer = demand.clone();
        demand.request();
        std::thread::spawn(move || {
            for _ in 0..100 { assert!(observer.is_pending()); }
        }).join().unwrap();
        assert!(demand.take());
        assert!(!demand.is_pending());
    }

    /// 回调先消费再申请下一帧；跨线程的新请求不能被上一帧的结束逻辑覆盖。
    #[test]
    fn request_after_consumption_survives() {
        let demand = Arc::new(FrameDemand::new());
        demand.request();
        assert!(demand.take());
        let requester = demand.clone();
        std::thread::spawn(move || requester.request()).join().unwrap();
        assert!(demand.is_pending());
        assert!(demand.take());
        assert!(!demand.is_pending());
    }
}
