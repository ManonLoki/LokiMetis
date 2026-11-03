//! 用真实 COM API 验证 Shell 线程隔离与引用平衡，不打开文件管理器或发送系统交互。

use super::*;
use windows::Win32::System::Com::{
    APTTYPE, APTTYPEQUALIFIER, APTTYPE_MAINSTA, APTTYPE_MTA, APTTYPE_STA,
    COINIT_APARTMENTTHREADED, COINIT_MULTITHREADED, CoGetApartmentType, CoInitializeEx, CoUninitialize,
};

/// 读取当前线程的真实 apartment，不依赖进程其它测试线程的隐式 MTA 状态。
fn apartment() -> APTTYPE {
    let mut kind = APTTYPE::default();
    let mut qualifier = APTTYPEQUALIFIER::default();
    // SAFETY: 两个输出指针指向本线程存活且类型匹配的栈变量。
    unsafe { CoGetApartmentType(&mut kind, &mut qualifier) }.expect("COM apartment must be initialized");
    kind
}

/// 归还测试自身在当前线程取得的 COM 引用，不跨线程共享。
fn uninitialize() {
    // SAFETY: 每次调用之前，测试均已确认同线程 CoInitializeEx 成功。
    unsafe { CoUninitialize(); }
}

/// 原 worker 为 MTA 时，Shell 操作仍获得 STA，结束后原 worker 的模型不变。
#[test]
fn file_manager_shell_isolates_existing_mta_worker() {
    std::thread::spawn(|| {
        // SAFETY: 独占测试线程，成功后在同一线程显式配对释放。
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok().unwrap();
        run_shell_on_sta(|| {
            assert!(matches!(apartment(), APTTYPE_STA | APTTYPE_MAINSTA));
            Ok(())
        }).unwrap();
        assert_eq!(apartment(), APTTYPE_MTA);
        uninitialize();
    }).join().unwrap();
}

/// 成功及业务失败返回都释放本次 STA，随后允许该线程改为 MTA。
#[test]
fn file_manager_shell_balances_success_and_error_apartments() {
    std::thread::spawn(|| {
        for fail in [false, true] {
            let result = with_shell_sta(|| {
                assert!(matches!(apartment(), APTTYPE_STA | APTTYPE_MAINSTA));
                if fail { Err(AppError::new("skin.open_failed", "测试失败。")) } else { Ok(()) }
            });
            assert_eq!(result.is_err(), fail);
            // SAFETY: STA guard 已归还引用，新的 MTA 初始化成功后在同线程配对释放。
            unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok().unwrap();
            uninitialize();
        }
    }).join().unwrap();
}

/// 已有 STA 返回 S_FALSE 时只释放新增引用，不提前拆除调用方 apartment。
#[test]
fn file_manager_shell_balances_nested_sta_reference() {
    std::thread::spawn(|| {
        // SAFETY: 本线程仅持有一次测试建立的 STA 引用，结束后显式释放。
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok().unwrap();
        with_shell_sta(|| Ok(())).unwrap();
        assert!(matches!(apartment(), APTTYPE_STA | APTTYPE_MAINSTA));
        uninitialize();
        // SAFETY: 原 STA 和嵌套初始化均已释放，成功建立的 MTA 在当前线程配对释放。
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok().unwrap();
        uninitialize();
    }).join().unwrap();
}

/// 模型冲突时不执行 Shell，也不释放调用方的 MTA 引用。
#[test]
fn file_manager_shell_rejects_model_conflict_without_operation() {
    std::thread::spawn(|| {
        // SAFETY: 新测试线程显式建立 MTA，最后由本测试释放。
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok().unwrap();
        let mut called = false;
        assert!(with_shell_sta(|| { called = true; Ok(()) }).is_err());
        assert!(!called);
        assert_eq!(apartment(), APTTYPE_MTA);
        uninitialize();
    }).join().unwrap();
}

/// 操作 panic 时专用线程仍被 join 回收，调用方得到稳定失败并可再次调用。
#[test]
fn file_manager_shell_reaps_panicked_operation_and_can_retry() {
    assert!(run_shell_on_sta(|| panic!("controlled shell operation failure")).is_err());
    run_shell_on_sta(|| Ok(())).unwrap();
}
