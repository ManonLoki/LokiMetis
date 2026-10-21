//! 恢复界面回归只使用假元信息和固定测试码，不读写真实凭据。

use super::{Shell, recovery::RecoveryKind};
use gpui_kit::{AppContext, TestAppContext, test::TestWindowExt};

/// 双语设备页呈现测试实例，恢复预览仅展示原UUID/数量且有独立明确确认按钮。
#[gpui_kit::test]
fn recovery_preview_identifies_target_and_requires_confirmation(cx: &mut TestAppContext) {
    for locale in ["zh-CN", "en-US"] {
        let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 960., 900.);
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |this: &mut Shell, cx| {
                this.locale = locale; this.page = crate::app::Page::Devices; this.devices.snapshot.loaded = true;
                this.devices.snapshot.instance = Some(("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into(), "LokiMetris TEST-aaaaaaaa".into(), true));
                cx.notify();
            });
            window.render_frame(cx);
            assert_eq!(window.find("devices-instance-name").label(), Some("LokiMetris TEST-aaaaaaaa"));
            assert!(window.try_find("devices-recovery-export").is_some());
            shell.update(cx, |this, cx| this.open_device_recovery(RecoveryKind::Preview(crate::usage_service::DeviceRecoveryPreview {
                ticket: "preview-ticket".into(), summary: loki_metris_core::devices::recovery::RecoverySummary {
                    client_id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into(), devices: vec![],
                },
            }), window, cx));
            window.render_frame(cx);
            assert!(window.try_find("device-recovery-preview-id").is_some());
            assert!(window.try_find("device-recovery-preview-count").is_some());
            assert!(window.try_find("device-recovery-confirm").is_some());
            assert!(window.try_find("device-recovery-input").is_none());
            assert!(window.try_find("device-recovery-code").is_none());
            window.click("device-recovery-close", cx); window.render_frame(cx);
            assert!(window.try_find("device-recovery-dialog").is_none());
        }).unwrap();
    }
}
