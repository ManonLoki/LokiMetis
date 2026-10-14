//! 出站入口固定指向用户提供的历史源码仓库，返回系统是否接受打开请求。

/// 当前下游未配置远端；此地址仅代表参考 LokiMetis，不代表 LokiMetris 发布仓库。
pub const REFERENCE_REPOSITORY: &str = "https://github.com/ManonLoki/LokiMetis";

/// 在后台执行系统 URL 打开 API，不启动 shell，也不将来源地址拼为命令。
pub fn open_repository() -> bool {
    #[cfg(target_os = "macos")]
    {
        let Some(url) = objc2_foundation::NSURL::URLWithString(
            &objc2_foundation::NSString::from_str(REFERENCE_REPOSITORY),
        ) else {
            return false;
        };
        objc2_app_kit::NSWorkspace::sharedWorkspace().openURL(&url)
    }
    #[cfg(windows)]
    {
        use windows::Win32::System::Com::{
            COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
        };
        use windows::{
            Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
            core::PCWSTR,
        };
        // SAFETY: GPUI后台线程不保证已有COM apartment；此调用不涉及指针，初始化与释放固定在同一线程。
        if unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }
            .is_err()
        {
            return false;
        }
        /// 成功与S_FALSE都必须配对释放本次COM初始化。
        struct Apartment;
        impl Drop for Apartment {
            /// 句柄只在创建线程的同步栈中存活，不跨await或线程移动。
            fn drop(&mut self) {
                // SAFETY: 只有本线程的 CoInitializeEx 成功后才构造 Apartment，这里与那次初始化一一配对。
                unsafe {
                    CoUninitialize();
                }
            }
        }
        let _apartment = Apartment;
        let url: Vec<u16> = REFERENCE_REPOSITORY.encode_utf16().chain(Some(0)).collect();
        let open: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
        // SAFETY: 固定UTF-16缓冲在调用期间有效且NUL终止；不接受用户可控参数。
        let result = unsafe {
            ShellExecuteW(
                None,
                PCWSTR(open.as_ptr()),
                PCWSTR(url.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        result.0 as isize > 32
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        false
    }
}
