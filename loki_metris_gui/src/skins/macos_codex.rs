//! 只在 macOS 上激活已验证的 Codex GUI 进程窗口。

use std::ffi::{c_char, c_void};

/// 声明 macOS Objective-C 互操作入口 `ObjectiveCObject`。
type ObjectiveCObject = *mut c_void;
/// 声明 macOS Objective-C 互操作入口 `ObjectiveCSelector`。
type ObjectiveCSelector = *mut c_void;

const ACTIVATE_ALL_WINDOWS: usize = 1;
const ACTIVATE_IGNORING_OTHER_APPS: usize = 1 << 1;

#[link(name = "objc")]
unsafe extern "C" {
    /// 声明 macOS Objective-C 互操作入口 `objc_getClass`。
    fn objc_getClass(name: *const c_char) -> ObjectiveCObject;
    /// 声明 macOS Objective-C 互操作入口 `sel_registerName`。
    fn sel_registerName(name: *const c_char) -> ObjectiveCSelector;
    /// 声明 macOS Objective-C 互操作入口 `objc_msgSend`。
    fn objc_msgSend();
    /// 声明 macOS Objective-C 互操作入口 `objc_autoreleasePoolPush`。
    fn objc_autoreleasePoolPush() -> *mut c_void;
    /// 声明 macOS Objective-C 互操作入口 `objc_autoreleasePoolPop`。
    fn objc_autoreleasePoolPop(context: *mut c_void);
}

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

/// 尝试把指定 PID 的 Codex GUI 窗口带到前台。
pub(crate) fn activate_gui_process(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    // SAFETY: 类名与 selector 均为编译期 NUL 结尾字符串；调用签名对应
    // NSRunningApplication 的 class method 和 instance method。返回对象只在当前
    // autorelease pool 内使用，不跨线程保存。
    unsafe {
        let pool = objc_autoreleasePoolPush();
        let class = objc_getClass(c"NSRunningApplication".as_ptr());
        let running_application =
            sel_registerName(c"runningApplicationWithProcessIdentifier:".as_ptr());
        let activate = sel_registerName(c"activateWithOptions:".as_ptr());
        let send_with_pid: unsafe extern "C" fn(
            ObjectiveCObject,
            ObjectiveCSelector,
            i32,
        ) -> ObjectiveCObject = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let send_with_options: unsafe extern "C" fn(
            ObjectiveCObject,
            ObjectiveCSelector,
            usize,
        ) -> i8 = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());

        let application = send_with_pid(class, running_application, pid);
        let activated = !application.is_null()
            && send_with_options(
                application,
                activate,
                ACTIVATE_ALL_WINDOWS | ACTIVATE_IGNORING_OTHER_APPS,
            ) != 0;
        objc_autoreleasePoolPop(pool);
        activated
    }
}
