//! 测试平台只验证提交边界，不能代替真实系统的许可和横幅。

use super::{Submission, submit};
use gpui_kit::TestAppContext;

/// 默认关闭与非法载荷都不能触达操作系统适配器。
#[gpui_kit::test]
fn disabled_and_invalid_notifications_do_not_reach_platform(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_app_identity("@@APP_IDENTIFIER@@", "@@NAME_EN@@");
        assert_eq!(submit(cx, false, "test/1", "Title", "Body"), Submission::Disabled);
        assert_eq!(submit(cx, true, "../invalid tag", "Title", "Body"), Submission::InvalidPayload);
        assert_eq!(submit(cx, true, "test/1", "", "Body"), Submission::InvalidPayload);
    });
    assert!(cx.shown_system_notifications().is_empty());
}

/// 已启用时传递原始标题与正文，结果只称提交，不称授权或送达。
#[gpui_kit::test]
fn enabled_notification_submits_bounded_payload(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_app_identity("@@APP_IDENTIFIER@@", "@@NAME_EN@@");
        assert_eq!(submit(cx, true, "test/1", "Title", "Body"), Submission::Submitted);
    });
    let shown = cx.shown_system_notifications();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].title.as_ref(), "Title");
    assert_eq!(shown[0].body.as_ref(), "Body");
    assert!(shown[0].actions.is_empty());
}
