//! 所有图片共用解码并发与像素保留预算，资源最后一个 owner 释放时归还。

use gpui_kit::App;
#[cfg(not(test))]
use std::sync::OnceLock;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// 解码后 CPU 像素总量上限；原始文件与 GPU 图集有独立生命周期。
const MAX_RESIDENT_BYTES: u64 = 512 * 1024 * 1024;

/// 资源只含线程安全所有权，解码 future 与像素租约均不借用 App。
pub(super) struct DecodeResources {
    decoders: Arc<Semaphore>,
    retained: Arc<AtomicU64>,
}
impl DecodeResources {
    /// 正式执行与测试使用相同的并发数、计数器和预算上限。
    pub(super) fn new() -> Self {
        Self {
            decoders: Arc::new(Semaphore::new(2)),
            retained: Arc::new(AtomicU64::new(0)),
        }
    }

    /// 同步解码真正结束后才释放许可；取消不会提前扩大工作并发。
    pub(super) async fn acquire(&self) -> OwnedSemaphorePermit {
        self.decoders
            .clone()
            .acquire_owned()
            .await
            .expect("decoder semaphore is never closed")
    }

    /// 全额原子预留，失败不部分消费；租约直接持有原计数器。
    pub(super) fn reserve(&self, bytes: u64) -> anyhow::Result<ImageMemoryLease> {
        ImageMemoryLease::from_counter(self.retained.clone(), bytes, MAX_RESIDENT_BYTES)
    }

    /// 真实 pending 回归占满所属池；许可可安全长于 App 本身。
    #[cfg(test)]
    pub(super) fn hold_decoders(&self) -> [OwnedSemaphorePermit; 2] {
        [
            self.decoders.clone().try_acquire_owned().unwrap(),
            self.decoders.clone().try_acquire_owned().unwrap(),
        ]
    }
}

/// 生产所有窗口继续共享进程总预算；测试隔离独立 App 的 scheduler 等待者。
pub(super) fn resources_for_app(cx: &mut App) -> Arc<DecodeResources> {
    #[cfg(not(test))]
    {
        let _ = cx;
        static RESOURCES: OnceLock<Arc<DecodeResources>> = OnceLock::new();
        RESOURCES
            .get_or_init(|| Arc::new(DecodeResources::new()))
            .clone()
    }
    #[cfg(test)]
    {
        if !cx.has_global::<AppDecodeResources>() {
            cx.set_global(AppDecodeResources(Arc::new(DecodeResources::new())));
        }
        cx.global::<AppDecodeResources>().0.clone()
    }
}

/// 同 App 的所有窗口仍共用一池；只在测试构建中按真实 App 边界隔离。
#[cfg(test)]
struct AppDecodeResources(Arc<DecodeResources>);
#[cfg(test)]
impl gpui_kit::Global for AppDecodeResources {}

/// 预算与真实 Arc 解码结果共生，复制结果不会重复计算。
pub(super) struct ImageMemoryLease {
    retained: Arc<AtomicU64>,
    bytes: u64,
}
impl ImageMemoryLease {
    /// 独立计数器使边界回归不依赖其它并行图片测试的瞬时占用。
    fn from_counter(retained: Arc<AtomicU64>, bytes: u64, limit: u64) -> anyhow::Result<Self> {
        retained
            .try_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|total| *total <= limit)
            })
            .map_err(|_| anyhow::anyhow!("monitor resident pixel budget exceeded"))?;
        Ok(Self { retained, bytes })
    }
}
impl Drop for ImageMemoryLease {
    /// 最后一个租约释放时归还常驻像素预算。
    fn drop(&mut self) {
        self.retained.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

#[cfg(test)]
#[path = "image_memory_tests.rs"]
mod isolation_tests;

#[cfg(test)]
mod tests {
    use super::*;
    /// 失败零消费，克隆 owner 不提前返还；最后引用释放后可以重新预留。
    #[test]
    fn resident_budget_follows_last_owner_and_rejects_overflow() {
        let counter = Arc::new(AtomicU64::new(0));
        let first = Arc::new(ImageMemoryLease::from_counter(counter.clone(), 60, 100).unwrap());
        let second = first.clone();
        assert!(ImageMemoryLease::from_counter(counter.clone(), 41, 100).is_err());
        assert!(ImageMemoryLease::from_counter(counter.clone(), u64::MAX, 100).is_err());
        assert_eq!(counter.load(Ordering::Acquire), 60);
        drop(first);
        assert_eq!(counter.load(Ordering::Acquire), 60);
        drop(second);
        assert_eq!(counter.load(Ordering::Acquire), 0);
        let _full = ImageMemoryLease::from_counter(counter.clone(), 100, 100).unwrap();
        assert_eq!(counter.load(Ordering::Acquire), 100);
    }
}
