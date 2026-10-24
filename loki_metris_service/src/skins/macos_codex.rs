//! 只在 macOS 上激活已验证的 Codex GUI 进程窗口。

use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};

/// 尝试把指定 PID 的 Codex GUI 窗口带到前台。
// ActivateIgnoringOtherApps 在 macOS 14+ 无效果，旧系统仍需要；保持与原实现相同的选项位。
#[allow(deprecated)]
pub(crate) fn activate_gui_process(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    // 返回对象只在当前 autorelease pool 内使用，不跨线程保存。
    autoreleasepool(|_| {
        NSRunningApplication::runningApplicationWithProcessIdentifier(pid).is_some_and(|application| {
            application.activateWithOptions(
                NSApplicationActivationOptions::ActivateAllWindows
                    | NSApplicationActivationOptions::ActivateIgnoringOtherApps,
            )
        })
    })
}
