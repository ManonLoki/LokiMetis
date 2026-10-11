//! 首次权限回归不登记身份、不 Show；真实 WinRT 对象只检查隐藏、过期和专属身份。
use super::*;
use std::cell::Cell;

/// 已允许、已拒绝和其它系统错误必须零初始化并保持原始结果。
#[test]
fn established_permission_and_other_errors_never_initialize() {
    for setting in [NotificationSetting::Enabled,NotificationSetting::DisabledForApplication,NotificationSetting::DisabledForUser,NotificationSetting::DisabledByGroupPolicy,NotificationSetting::DisabledByManifest] {
        assert_eq!(resolve(||Ok(setting),||panic!("已有设置不能初始化")).unwrap(),setting);
    }
    let denied=windows::core::HRESULT::from_win32(5);
    assert_eq!(resolve(||Err(windows::core::Error::from_hresult(denied)),||panic!("其它错误不能初始化")).unwrap_err().code(),denied);
}

/// 身份未就绪只能初始化一次，并以之后真实权限为准，包括系统明确拒绝。
#[test]
fn first_identity_query_initializes_once_then_uses_real_setting() {
    for setting in [NotificationSetting::Enabled,NotificationSetting::DisabledForUser] {
        let queries=Cell::new(0);let initialized=Cell::new(0);
        let result=resolve(||{let call=queries.get();queries.set(call+1);if call==0 {Err(windows::core::Error::from_hresult(ERROR_NOT_FOUND.to_hresult()))}else{Ok(setting)}},
            ||{initialized.set(initialized.get()+1);Ok(())}).unwrap();
        assert_eq!(result,setting);assert_eq!(queries.get(),2);assert_eq!(initialized.get(),1);
    }
}

/// 初始化失败直接返回；第二次仍缺身份也不能循环 Show 或伪造允许。
#[test]
fn initialization_and_requery_failures_remain_errors_without_looping() {
    let queries=Cell::new(0);let denied=windows::core::HRESULT::from_win32(5);
    let result=resolve(||{queries.set(queries.get()+1);Err(windows::core::Error::from_hresult(ERROR_NOT_FOUND.to_hresult()))},||Err(windows::core::Error::from_hresult(denied)));
    assert_eq!(result.unwrap_err().code(),denied);assert_eq!(queries.get(),1);
    let initialized=Cell::new(0);
    let result=resolve(||Err(windows::core::Error::from_hresult(ERROR_NOT_FOUND.to_hresult())),||{initialized.set(initialized.get()+1);Ok(())});
    assert_eq!(result.unwrap_err().code(),ERROR_NOT_FOUND.to_hresult());assert_eq!(initialized.get(),1);
}

/// 外层在 Show 内取消仍必须执行精确清理；预先取消则零提交。
#[test]
fn cancellation_blocks_submission_but_never_skips_accepted_cleanup() {
    let cancelled=Cell::new(false);let removed=Cell::new(false);
    let check=||if cancelled.get(){Err(windows::core::Error::from_hresult(windows::core::HRESULT::from_win32(1223)))}else{Ok(())};
    assert!(submit_initialization(||{cancelled.set(true);Ok(())},||{removed.set(true);Ok(())},&check).is_err());
    assert!(removed.get());
    assert!(submit_initialization(||panic!("取消后不能提交"),||panic!("没有提交不清理"),&check).is_err());
}

/// 授权期限和 future 生命周期独立控制，取消旧操作不影响重试。
#[test]
fn authorization_future_drop_and_deadline_cancel_only_the_owned_operation() {
    use crate::native::notifications::driver::Authorization;
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(60);
    let (first,guard)=Authorization::new(deadline);let (retry,_retry_guard)=Authorization::new(deadline);
    assert!(first.check().is_ok());drop(guard);assert!(first.check().is_err());assert!(retry.check().is_ok());
    let (expired,_expired_guard)=Authorization::new(std::time::Instant::now()-std::time::Duration::from_secs(1));
    assert!(matches!(expired.check(),Err(crate::native::notifications::Error::Timeout)));
}

/// 真正 Toast 必须禁止弹窗、静音、专属标记并保持给定过期时刻。
#[test]
fn initialization_toast_is_suppressed_expiring_and_separate_from_milestones() {
    let _apartment=super::super::Apartment::new().unwrap();
    let expiration=DateTime{UniversalTime:134_044_999_000_000_000};
    let toast=initialization_toast(expiration).unwrap();
    assert!(toast.SuppressPopup().unwrap());assert_eq!(toast.Tag().unwrap(),TAG);assert_eq!(toast.Group().unwrap(),GROUP);
    assert_eq!(toast.ExpirationTime().unwrap().Value().unwrap(),expiration);
    assert_eq!(toast.Content().unwrap().GetElementsByTagName(h!("audio")).unwrap().Item(0).unwrap().Attributes().unwrap().GetNamedItem(h!("silent")).unwrap().InnerText().unwrap(),"true");
}
