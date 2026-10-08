//! 所有图片共用解码并发与像素保留预算，资源最后一个 owner 释放时归还。

use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU64, Ordering},
};

/// 同步解码只能在真正工作完成后释放许可，UI 取消不会提前扩大并发。
pub(super) static DECODERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
/// 解码后 CPU 像素总量上限；原始文件与 GPU 图集有独立生命周期。
const MAX_RESIDENT_BYTES: u64 = 512 * 1024 * 1024;

/// 预算与真实 Arc 解码结果共生，复制结果不会重复计算。
pub(super) struct ImageMemoryLease {
    retained: Arc<AtomicU64>,
    bytes: u64,
}
impl ImageMemoryLease {
    /// 全进程原子预留，失败不部分消费预算。
    pub fn reserve(bytes: u64) -> anyhow::Result<Self> {
        static RETAINED: OnceLock<Arc<AtomicU64>> = OnceLock::new();
        Self::from_counter(
            RETAINED.get_or_init(|| Arc::new(AtomicU64::new(0))).clone(),
            bytes,
            MAX_RESIDENT_BYTES,
        )
    }
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
    fn drop(&mut self) {
        self.retained.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

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
