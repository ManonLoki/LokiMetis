//! 承担桌面宿主在重型扫描边界上的内存回收提示；不参与扫描规则、结果或持久化。

#[cfg(target_os = "macos")]
use std::ffi::c_void;

#[cfg(target_os = "macos")]
#[link(name = "System")]
unsafe extern "C" {
    fn malloc_zone_pressure_relief(zone: *mut c_void, goal: usize) -> usize;
}

/// 保证扫描函数所有退出路径都在后声明的重对象释放后执行宿主内存回收提示。
#[must_use = "guard 必须存活到扫描函数退出"]
pub(crate) struct ScanMemoryPressureReliefGuard;

impl ScanMemoryPressureReliefGuard {
    /// 在扫描函数第一个局部绑定处创建，利用 Rust 逆序析构把自身留到最后释放。
    pub(crate) fn new() -> Self {
        Self
    }
}

impl Drop for ScanMemoryPressureReliefGuard {
    /// 提示宿主尽量归还已空闲堆页；该动作不改变扫描结果或持久化语义。
    fn drop(&mut self) {
        relieve_scan_memory_pressure();
    }
}

/// 调用当前平台的空闲堆页回收能力；非 macOS 平台保持 no-op。
fn relieve_scan_memory_pressure() {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: Apple 将 NULL zone 定义为检查所有 malloc zones，将 0 goal
        // 定义为尽最大努力释放空闲页；调用不接收或保留任何 Rust 指针。
        let _released_bytes = unsafe { malloc_zone_pressure_relief(std::ptr::null_mut(), 0) };
    }
}

#[cfg(test)]
mod tests {
    use super::ScanMemoryPressureReliefGuard;

    /// 宿主回收提示允许在多个扫描边界重复调用且不依赖待释放字节数。
    #[test]
    fn scan_memory_pressure_relief_is_repeatable() {
        let temporary_scan_buffer = vec![0_u8; 64 * 1024];
        std::hint::black_box(&temporary_scan_buffer);
        drop(temporary_scan_buffer);

        drop(ScanMemoryPressureReliefGuard::new());
        drop(ScanMemoryPressureReliefGuard::new());
    }
}
