//! 原生设备页面回归覆盖按钮所有权、双语模态、稳定草稿与跨页会话。

use gpui_kit::{AppContext, Role, TestAppContext, test::TestWindowExt};
use super::Shell;
use loki_metris_core::devices::{DiscoveredDevice, SavedDeviceSummary};

const DEVICE: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

/// 直接打开模态的测试仍需提供一项当前可配对的真实领域候选。
pub(super) fn prepare_target(shell: &mut Shell) {
    shell.devices.snapshot.discovered = vec![DiscoveredDevice::new(DEVICE.into(), "Receiver".into(),
        vec!["192.168.1.2:4000".parse().unwrap()]).unwrap().with_pairing_protocol(Some("1"))];
}

/// 新导航保持80px侧栏，发现行周围点击不代替配对按钮，移除同样只有按钮拥有。
#[gpui_kit::test]
fn devices_navigation_rows_and_modal_use_semantic_controls(cx: &mut TestAppContext) {
    let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 960., 640.);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx); window.click("nav-devices", cx);
        shell.update(cx, |this, cx| {
            this.devices.error = None; this.devices.snapshot.loaded = true;
            this.devices.snapshot.discovered.push(DiscoveredDevice::new(DEVICE.into(), "Receiver".into(), vec!["192.168.1.2:4000".parse().unwrap()]).unwrap().with_pairing_protocol(Some("1")));
            cx.notify();
        });
        window.render_frame(cx);
        assert_eq!(window.find("sidebar").bounds().size.width, gpui_kit::px(80.));
        assert!(window.try_find("devices-page").is_some());
        window.click(format!("device-found-{DEVICE}"), cx);
        assert!(shell.read(cx).devices.modal.is_none());
        window.click(format!("device-pair-{DEVICE}"), cx); window.render_frame(cx);
        assert!(shell.read(cx).devices.modal.is_some());
        assert_eq!(window.find("device-dialog").role(), Some(Role::Dialog));
        assert_eq!(window.find("device-dialog").label(), Some("Pair device"));
        window.click("device-dialog-cancel", cx); window.render_frame(cx);
        assert!(shell.read(cx).devices.modal.is_none());
        shell.update(cx, |this, cx| { this.devices.snapshot.saved.push(SavedDeviceSummary { id: DEVICE.into(), name: "Receiver".into() }); cx.notify(); });
        window.render_frame(cx); window.click(format!("device-saved-{DEVICE}"), cx);
        assert!(shell.read(cx).devices.modal.is_none());
        window.click(format!("device-remove-{DEVICE}"), cx); window.render_frame(cx);
        assert_eq!(window.find("device-dialog").label(), Some("Remove device?"));
        window.click("device-dialog-cancel", cx); window.click("nav-settings", cx); window.click("nav-devices", cx);
        assert_eq!(shell.read(cx).devices.snapshot.saved.len(), 1);
    }).unwrap();
}

/// 两种语言均从真实词典显示空列表；新根不继承另一个页面会话。
#[gpui_kit::test]
fn devices_empty_state_is_localized_and_new_root_is_empty(cx: &mut TestAppContext) {
    for (locale, label, title) in [("en-US", "Pair device", "Device management"), ("zh-CN", "配对设备", "设备管理")] {
        let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |this, cx| { this.locale = locale; this.page = crate::app::Page::Devices; cx.notify(); });
            window.render_frame(cx); assert!(window.try_find("devices-empty").is_some());
            assert_eq!(window.find("devices-title").label(), Some(title));
            assert_eq!(window.find("nav-devices").label(), Some(title));
            shell.update(cx, |this, cx| { prepare_target(this); this.open_device_dialog(DEVICE.into(), "Receiver".into(), false, window, cx); });
            window.render_frame(cx); assert_eq!(window.find("device-dialog").label(), Some(label));
            window.click("device-dialog-cancel", cx);
            assert!(shell.read(cx).devices.snapshot.saved.is_empty());
        }).unwrap();
    }
}

/// 仅广播候选仍展示并导出禁用原因，点击按钮不能打开配对弹窗；成功反馈有名称。
#[gpui_kit::test]
fn broadcast_only_candidates_have_accessible_disabled_pairing(cx: &mut TestAppContext) {
    for advertised in [None, Some("off")] {
        let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |this, cx| {
                this.page = crate::app::Page::Devices; this.devices.snapshot.loaded = true;
                this.devices.snapshot.completed = Some("paired"); prepare_target(this);
                this.devices.snapshot.discovered[0] = this.devices.snapshot.discovered[0].clone().with_pairing_protocol(advertised);
                cx.notify();
            });
            window.render_frame(cx);
            let reason = window.find(format!("device-pairing-unavailable-{DEVICE}"));
            assert_eq!(reason.role(), Some(Role::Label));
            assert_eq!(reason.label(), Some("Pairing is not enabled on this device"));
            assert_eq!(window.find("devices-feedback").label(), Some("Pairing succeeded. Device saved."));
            window.click(format!("device-pair-{DEVICE}"), cx); window.render_frame(cx);
            assert!(shell.read(cx).devices.modal.is_none());
        }).unwrap();
    }
}

/// 已认证IP与后台状态独立显示，只有发现广告时不能伪造在线或可信IP。
#[gpui_kit::test]
fn authenticated_device_metadata_and_unreachable_state_are_localized(cx: &mut TestAppContext) {
    use loki_metris_core::devices::{link::{DeviceInfo, DevicePlatform}, presence::{DevicePresenceView, DeviceConnectionState, UnavailableReason}};
    for (locale, connected, background, not_connected, ip, last_ip) in [
        ("en-US", "Connected", "In background · sync resumes when opened", "Not connected", "IP: 192.168.1.20", "Last IP: 192.168.1.20"),
        ("zh-CN", "当前可通信", "已进入后台，打开终端后恢复同步", "尚未连接", "IP：192.168.1.20", "上次 IP：192.168.1.20"),
    ] {
        let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |this, cx| {
                this.locale = locale; this.page = crate::app::Page::Devices; this.devices.snapshot.loaded = true;
                this.devices.snapshot.saved.push(SavedDeviceSummary { id: DEVICE.into(), name: "Saved name".into() });
                this.devices.snapshot.presence.push(DevicePresenceView { device_id: DEVICE.into(),
                    info: Some(DeviceInfo { device_id: DEVICE.into(), name: "Authenticated phone".into(), platform: DevicePlatform::Ios, app_version: "1.0".into() }),
                    state: DeviceConnectionState::Reachable, peer: Some("192.168.1.20:51000".parse().unwrap()), since_last_contact: Some(std::time::Duration::ZERO) });
                cx.notify();
            });
            window.render_frame(cx);
            assert_eq!(window.find(format!("device-status-{DEVICE}")).label(), Some(connected));
            assert_eq!(window.find(format!("device-ip-{DEVICE}")).label(), Some(ip));
            shell.update(cx, |this, cx| { this.devices.snapshot.presence[0].state = DeviceConnectionState::Unavailable(UnavailableReason::Background); cx.notify(); });
            window.render_frame(cx);
            assert_eq!(window.find(format!("device-status-{DEVICE}")).label(), Some(background));
            assert_eq!(window.find(format!("device-ip-{DEVICE}")).label(), Some(last_ip));
            shell.update(cx, |this, cx| { this.devices.snapshot.presence.clear(); prepare_target(this); cx.notify(); });
            window.render_frame(cx);
            assert_eq!(window.find(format!("device-status-{DEVICE}")).label(), Some(not_connected));
            assert!(window.try_find(format!("device-ip-{DEVICE}")).is_none());
        }).unwrap();
    }
}
