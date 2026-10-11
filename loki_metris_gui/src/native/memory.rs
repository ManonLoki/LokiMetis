//! 大批量后台读取结束后，把分配器持有的空闲页归还系统。
//!
//! 读取保留期内的全部调用会在短时间内形成数十到上百 MB 的临时分配；释放之后分配器
//! 仍把这些页留在自己的区域里，footprint 不会随之回落。每次大读取结束后请求一次
//! 回收，并限制调用频率，避免在高频刷新时反复进入分配器锁。

/// 两次回收之间的最短间隔（毫秒）。
#[cfg(target_os = "macos")]
const MIN_INTERVAL_MS: i64 = 5_000;

/// 请求分配器归还可回收的空闲页；非 macOS 目标没有等价的稳定接口，保持空操作。
pub(crate) fn release_free_pages() {
    #[cfg(target_os = "macos")]
    {
        use std::sync::atomic::{AtomicI64, Ordering};
        static LAST_MS: AtomicI64 = AtomicI64::new(i64::MIN / 2);
        unsafe extern "C" {
            /// libmalloc 公开接口：zone 传空表示全部 zone，goal 为 0 表示尽可能回收。
            fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
        }
        let now = crate::backend::local_index::current_epoch_ms();
        let last = LAST_MS.load(Ordering::Relaxed);
        if now.saturating_sub(last) < MIN_INTERVAL_MS
            || LAST_MS.compare_exchange(last, now, Ordering::AcqRel, Ordering::Relaxed).is_err()
        {
            return;
        }
        // SAFETY: 参数符合 libmalloc 约定；该调用只释放空闲页，不触碰存活分配。
        unsafe {
            malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
        }
    }
}
