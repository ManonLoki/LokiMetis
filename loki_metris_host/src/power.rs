//! 只读系统电源计时，供后台网络服务在睡眠后恢复原生资源。

use std::time::Duration;

/// 读取 Windows 自启动以来的累计睡眠时间；其它平台或查询失败返回空，由调用方保留原检测方式。
/// QueryUnbiasedInterruptTime 排除睡眠，来源：https://learn.microsoft.com/windows/win32/api/realtimeapiset/nf-realtimeapiset-queryunbiasedinterrupttime
pub fn suspended_time() -> Option<Duration> {
    #[cfg(windows)]
    {
        use windows::Win32::System::WindowsProgramming::{QueryInterruptTime, QueryUnbiasedInterruptTime};
        let mut awake = 0;
        // SAFETY: awake 为有效可写输出，两项只读时钟 API 无额外初始化要求。
        if !unsafe { QueryUnbiasedInterruptTime(&mut awake) }.as_bool() { return None; }
        // SAFETY: 此系统时钟查询不接收指针，也不改变系统电源状态。
        let total = unsafe { QueryInterruptTime() };
        Some(Duration::from_nanos(total.saturating_sub(awake).saturating_mul(100)))
    }
    #[cfg(not(windows))]
    { None }
}

#[cfg(all(test, windows))]
mod tests {
    /// Windows 原生计时查询能返回累计睡眠量，无需改变真实电源状态。
    #[test]
    fn windows_suspended_time_is_available() {
        assert!(super::suspended_time().is_some());
    }
}
