//! 原生设备页面回归覆盖按钮所有权、双语模态、稳定草稿与跨页会话。

use gpui_kit::{AppContext, Role, TestAppContext, test::TestWindowExt};
use super::Shell;
use loki_metris_core::devices::{DiscoveredDevice, SavedDeviceSummary};

const DEVICE: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

/// 设备页只展示已配对列表，不再提供发现、刷新或未认证候选入口。
#[gpui_kit::test]
fn devices_page_has_no_discovery_controls(cx: &mut TestAppContext) {
    for (width, locale) in [(960., "zh-CN"), (1440., "en-US")] {
        let (handle, shell, _finish) = crate::app::tests::open_shell(cx, width, 900.);
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |this, cx| { this.locale = locale; this.page = crate::app::Page::Devices; prepare_target(this); cx.notify(); });
            window.render_frame(cx);
            assert!(window.try_find("devices-saved").is_some());
            for id in ["devices-discover", "devices-discovered", "devices-discovered-header"] { assert!(window.try_find(id).is_none()); }
            assert!(shell.read(cx).devices.modal.is_none(), "普通广告不能触发弹窗");
        }).unwrap();
    }
}

/// 宽窄窗口、双语和双主题保持三行卡片；平台与状态、名称与版本、IP与通信同行且不溢出。
#[gpui_kit::test]
fn device_cards_reflow_and_contain_long_authenticated_metadata(cx: &mut TestAppContext) {
    use gpui_kit::{px, component::{Theme, ThemeMode}};
    use loki_metris_core::devices::{link::{DeviceInfo, DevicePlatform}, presence::{DevicePresenceView, DeviceConnectionState}};
    for (width, columns, dark, locale) in [
        (960., 2, false, "en-US"), (960., 2, true, "en-US"),
        (1280., 3, false, "en-US"), (1280., 3, true, "en-US"),
        (1440., 3, false, "en-US"), (1440., 3, true, "en-US"),
        (960., 2, false, "zh-CN"), (960., 2, true, "zh-CN"),
        (1280., 3, false, "zh-CN"), (1280., 3, true, "zh-CN"),
        (1440., 3, false, "zh-CN"), (1440., 3, true, "zh-CN"),
    ] {
        let (handle, shell, _finish) = crate::app::tests::open_shell(cx, width, 1100.);
        cx.update(|cx| Theme::change(if dark { ThemeMode::Dark } else { ThemeMode::Light }, None, cx));
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |this, cx| {
                this.locale = locale; this.page = crate::app::Page::Devices;
                this.devices.snapshot.loaded = true;
                for index in 0..4 {
                    let id = format!("00000000-0000-4000-8000-{index:012}");
                    let name = ["Manon's iPhone", "Work Android phone", "iPad", "Android Tablet"][index].repeat(5);
                    this.devices.snapshot.saved.push(SavedDeviceSummary { id: id.clone(), name: "Saved".into() });
                    this.devices.snapshot.presence.push(DevicePresenceView { form_factor: Some(if index < 2 { loki_metris_core::devices::link::DeviceFormFactor::Phone } else { loki_metris_core::devices::link::DeviceFormFactor::Tablet }),
                        device_id: id.clone(), info: Some(DeviceInfo { device_id: id, name,
                            platform: if index % 2 == 0 { DevicePlatform::Ios } else { DevicePlatform::Android },
                            app_version: "1".repeat(64) }), state: DeviceConnectionState::Reachable,
                        peer: Some("[fe80:abcd:abcd:abcd:abcd:abcd:abcd:abcd%12]:4200".parse().unwrap()),
                        since_last_contact: Some(std::time::Duration::from_secs(if index == 3 { u64::MAX } else { 4 })), companion: None,
                    });
                }
                this.devices.snapshot.presence[0].companion = Some(loki_metris_core::devices::companion::WatchCompanionView {
                    info: loki_metris_core::devices::companion::WatchInfo {
                        device_id: "c169da9b-4716-4774-bdcb-a33bfd6e1ac1".into(), name: "Long Watch name ".repeat(10), app_version: "1".repeat(64),
                    }, reachable: true, since_last_contact: Some(std::time::Duration::from_secs(4)), since_last_sync: None,
                });
                prepare_target(this);
                this.devices.snapshot.discovered.push(DiscoveredDevice::new("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into(),
                    "Another nearby device".into(), vec!["192.168.1.3:4000".parse().unwrap()]).unwrap().with_pairing_protocol(Some("1")));
                cx.notify();
            });
            window.render_frame(cx);
            let grid = window.find("devices-saved-grid").bounds();
            let first = window.find("device-saved-00000000-0000-4000-8000-000000000000").bounds();
            for index in 0..4 {
                let id = format!("00000000-0000-4000-8000-{index:012}");
                let card = window.find(format!("device-saved-{id}")).bounds();
                let image = window.find(format!("device-artwork-{id}")).bounds();
                let details = window.find(format!("device-details-{id}")).bounds();
                let name = window.find(format!("device-name-{id}")).bounds();
                let ip = window.find(format!("device-ip-{id}")).bounds();
                assert!(card.left() >= grid.left() && card.right() <= grid.right() + px(1.));
                assert!((card.size.width - first.size.width).abs() < px(1.));
                assert_eq!(image.size, gpui_kit::size(px(96.), px(112.)));
                assert!(image.left() >= card.left() + px(16.) && image.top() >= card.top() + px(16.));
                assert!(details.left() >= image.right() + px(16.));
                assert!(details.right() <= card.right() - px(16.) && details.bottom() <= card.bottom() - px(16.) + px(1.));
                if index == 0 || index >= columns {
                    assert!(card.size.height <= details.size.height.max(image.size.height) + px(35.), "卡片只保留内容高度与边框/内边距");
                }
                let status = window.find(format!("device-status-{id}")).bounds();
                let platform = window.find(format!("device-platform-{id}")).bounds();
                let remove = window.find(format!("device-remove-{id}")).bounds();
                let header = window.find(format!("device-header-{id}")).bounds();
                let address = window.find(format!("device-address-{id}")).bounds();
                let version = window.find(format!("device-version-{id}")).bounds();
                let contact = window.find(format!("device-contact-{id}")).bounds();
                for content in [name, ip, address, platform, status, version, contact, window.find(format!("device-metadata-{id}")).bounds()] {
                    assert!(content.left() >= details.left() && content.right() <= details.right() + px(1.));
                    assert!(content.top() >= details.top() && content.bottom() <= details.bottom() + px(1.));
                }
                assert!(address.size.height < px(40.));
                assert!((status.left() - platform.right() - px(8.)).abs() <= px(1.), "状态标签紧随平台");
                assert!((status.center().y - platform.center().y).abs() <= px(1.), "平台和状态标签保持同行");
                assert!(status.right() <= remove.left() - px(7.), "状态标签不覆盖移除按钮");
                assert!(name.top() >= header.bottom(), "名称位于平台行下方");
                assert!(version.left() >= name.right() + px(7.), "版本不能覆盖设备名");
                assert!((version.center().y - name.center().y).abs() <= px(1.), "名称和版本保持同行");
                assert!((version.right() - details.right()).abs() <= px(1.), "版本位于名称行最右侧");
                assert!((ip.left() - details.left()).abs() <= px(1.), "IP与设备名左侧对齐");
                assert!(address.top() >= name.bottom(), "IP位于名称行下方");
                assert!(contact.left() >= address.right() + px(7.), "通信时间不能覆盖IP");
                assert!((address.center().y - contact.center().y).abs() <= px(1.), "IP和通信时间保持同行");
                assert!((contact.right() - details.right()).abs() <= px(1.), "通信时间位于IP行最右侧");
                assert!(name.size.width > px(20.) && version.size.width > px(20.) && ip.size.width > px(20.) && contact.size.width > px(20.), "长文本不能挤掉同一行的另一项");
                assert_eq!(window.find(format!("device-version-{id}")).label(), Some("1".repeat(64).as_str()), "版本只显示原始版本号");
                if index >= columns { assert!(card.size.height <= px(160.), "无Watch的卡片不再为各元数据保留独立行"); }
                let expected_name = ["Manon's iPhone", "Work Android phone", "iPad", "Android Tablet"][index].repeat(5);
                assert_eq!(window.find(format!("device-name-{id}")).label(), Some(expected_name.as_str()), "截断不丢失完整辅助名称");
                if index < columns { assert_eq!(card.top(), first.top()); }
                else { assert!(card.top() >= first.bottom()); }
            }
            let id = "00000000-0000-4000-8000-000000000000";
            let watch = window.find(format!("device-watch-{id}")).bounds();
            let details = window.find(format!("device-details-{id}")).bounds();
            let watch_image = window.find(format!("device-watch-artwork-{id}")).bounds();
            let watch_details = window.find(format!("device-watch-details-{id}")).bounds();
            let watch_name = window.find(format!("device-watch-name-{id}")).bounds();
            let watch_status = window.find(format!("device-watch-status-{id}")).bounds();
            assert!(watch.left() >= details.left() && watch.right() <= details.right() + px(1.));
            assert!(watch.bottom() <= details.bottom() + px(1.));
            assert_eq!(watch_image.size, gpui_kit::size(px(40.), px(48.)));
            assert!(watch_image.left() >= watch.left() && watch_image.bottom() <= watch.bottom());
            assert!((watch_details.left() - watch_image.right() - px(8.)).abs() <= px(1.));
            assert!(watch_details.right() <= watch.right() + px(1.));
            assert!(watch_name.size.width > px(20.) && watch_status.left() >= watch_name.right() + px(7.));
            assert!((watch_name.center().y - watch_status.center().y).abs() <= px(1.));
            assert!(watch_status.right() <= watch_details.right() + px(1.));
            assert_eq!(window.find(format!("device-watch-name-{id}")).label(), Some(format!("Watch · {}", "Long Watch name ".repeat(10)).as_str()));
            for target in ["device-saved", "device-artwork", "device-name", "device-ip", "device-status", "device-version", "device-contact", "device-watch", "device-watch-artwork", "device-watch-name", "device-watch-status"] {
                window.click(format!("{target}-{id}"), cx); window.render_frame(cx);
                assert!(shell.read(cx).devices.modal.is_none());
                assert_eq!(shell.read(cx).devices.snapshot.saved.len(), 4);
            }
            window.click(format!("device-remove-{id}"), cx); window.render_frame(cx);
            assert!(shell.read(cx).devices.modal.is_some());
        }).unwrap();
    }
}

/// 真实服务为尚未连接设备也保留presence，发现提示与等待IP仍必须显示且不误示在线。
#[gpui_kit::test]
fn discovered_saved_device_without_authenticated_contact_keeps_pending_ip(cx: &mut TestAppContext) {
    use loki_metris_core::devices::presence::{DevicePresenceView, DeviceConnectionState};
    let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 960., 640.);
    cx.update_window(handle, |_, window, cx| {
        shell.update(cx, |this, cx| {
            this.page = crate::app::Page::Devices;
            this.devices.snapshot.loaded = true;
            prepare_target(this);
            this.devices.snapshot.saved.push(SavedDeviceSummary { id: DEVICE.into(), name: "Saved receiver".into() });
            this.devices.snapshot.presence.push(DevicePresenceView { form_factor: None, device_id: DEVICE.into(), info: None,
                state: DeviceConnectionState::NotConnected, peer: None, since_last_contact: None, companion: None });
            cx.notify();
        });
        window.render_frame(cx);
        assert_eq!(window.find(format!("device-seen-{DEVICE}")).label(), Some("Recently discovered (connection unverified)"));
        assert_eq!(window.find(format!("device-ip-pending-{DEVICE}")).label(), Some("IP · waiting for a connection"));
        assert_eq!(window.find(format!("device-status-{DEVICE}")).label(), Some("Not connected"));
        assert_eq!(window.find(format!("device-status-tag-{DEVICE}")).label(), Some("Offline"));
        assert!(window.try_find(format!("device-found-{DEVICE}")).is_none());
        assert!(window.try_find(format!("device-ip-{DEVICE}")).is_none());
    }).unwrap();
}

/// 直接打开模态的测试仍需提供一项当前可配对的真实领域候选。
pub(super) fn prepare_target(shell: &mut Shell) {
    shell.devices.snapshot.discovered = vec![DiscoveredDevice::new(DEVICE.into(), "Receiver".into(),
        vec!["192.168.1.2:4000".parse().unwrap()]).unwrap().with_pairing_protocol(Some("1"))];
}

/// 请求夹具显式模拟真实连接，普通广告夹具保持不触发自动弹窗。
pub(super) fn prepare_request(shell: &mut Shell) {
    prepare_target(shell);
    shell.devices.snapshot.loaded = true;
    shell.devices.snapshot.requests = vec![crate::usage_service::DevicePairingRequest {
        ticket: 1, device: shell.devices.snapshot.discovered[0].clone(),
    }];
}

/// 新导航保持80px侧栏，客户端弹窗全局展示，移除只有按钮拥有。
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
        assert!(shell.read(cx).devices.modal.is_none());
        shell.update(cx, |this, cx| { prepare_request(this); cx.notify(); });
        window.render_frame(cx);
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
    for (locale, label, title) in [("en-US", "Pair device", "Terminal devices"), ("zh-CN", "配对设备", "终端设备")] {
        let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |this, cx| { this.locale = locale; this.page = crate::app::Page::Devices; cx.notify(); });
            window.render_frame(cx); assert!(window.try_find("devices-empty-message").is_some());
            assert_eq!(window.find("devices-title").label(), Some(title));
            assert_eq!(window.find("nav-devices").label(), Some(title));
            shell.update(cx, |this, cx| { prepare_request(this); this.open_device_dialog(DEVICE.into(), "Receiver".into(), false, window, cx); });
            window.render_frame(cx); assert_eq!(window.find("device-dialog").label(), Some(label));
            window.click("device-dialog-cancel", cx);
            assert!(shell.read(cx).devices.snapshot.saved.is_empty());
        }).unwrap();
    }
}

/// 自动弹窗跨页面接收请求、展示来源信息，取消后同一请求不重开而新连接可再次提示。
#[gpui_kit::test]
fn incoming_requests_open_globally_and_cancel_per_ticket(cx: &mut TestAppContext) {
    let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
    assert!(cx.simulate_accessibility_activation(handle).is_some());
    cx.update_window(handle, |_, window, cx| {
        shell.update(cx, |this, cx| { this.page = crate::app::Page::Settings; prepare_request(this); cx.notify(); });
        window.render_frame(cx);
        assert_eq!(window.find("device-request-name").label(), Some("Receiver"));
        assert_eq!(window.find("device-request-id").label(), Some(DEVICE));
        assert_eq!(window.find("device-request-ip").label(), Some("192.168.1.2"));
        assert_eq!(shell.read(cx).page, crate::app::Page::Settings);
        window.input("0007", cx); window.render_frame(cx);
        shell.update(cx, |this, cx| { this.navigate(crate::app::Page::Dashboard, cx); });
        window.render_frame(cx);
        assert!(shell.read(cx).devices.modal.is_some(), "切页不取消应用级请求");
        window.click("device-dialog-cancel", cx); window.render_frame(cx); window.render_frame(cx);
        assert!(shell.read(cx).devices.modal.is_none());
        shell.update(cx, |this, cx| { prepare_request(this); this.devices.snapshot.requests[0].ticket = 2; cx.notify(); });
        window.render_frame(cx);
        assert!(shell.read(cx).devices.modal.is_some());
        assert!(!window.debug_a11y_tree_json().unwrap().contains("0007"), "新连接不能继承旧配对码");
    }).unwrap();
}

/// 现有模态不被配对请求抢占；请求消失后丢弃输入，并给下一连接创建新草稿。
#[gpui_kit::test]
fn incoming_requests_wait_for_other_modals_and_expire(cx: &mut TestAppContext) {
    let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
    cx.update_window(handle, |_, window, cx| {
        shell.update(cx, |this, cx| {
            prepare_request(this); this.open_device_dialog(DEVICE.into(), "Saved".into(), true, window, cx); cx.notify();
        });
        window.render_frame(cx);
        assert_eq!(window.find("device-dialog").label(), Some("Remove device?"));
        window.click("device-dialog-cancel", cx); window.render_frame(cx);
        assert_eq!(window.find("device-dialog").label(), Some("Pair device"));
        window.input("0007", cx);
        shell.update(cx, |this, cx| { this.devices.snapshot.requests.clear(); cx.notify(); });
        window.render_frame(cx);
        assert!(shell.read(cx).devices.modal.is_none());
    }).unwrap();
}

/// 已认证IP与后台状态独立显示，只有发现广告时不能伪造在线或可信IP。
#[gpui_kit::test]
fn authenticated_device_metadata_and_unreachable_state_are_localized(cx: &mut TestAppContext) {
    use gpui_kit::component::{ActiveTheme, Theme, ThemeMode};
    use loki_metris_core::devices::{companion::{WatchInfo, WatchCompanionView}, link::{DeviceInfo, DevicePlatform}, presence::{DevicePresenceView, DeviceConnectionState, UnavailableReason}};
    for (locale, connected, background, not_connected, ip, last_ip) in [
        ("en-US", "Connected", "In background · sync resumes when opened", "Not connected", "IP: 192.168.1.20", "Last IP: 192.168.1.20"),
        ("zh-CN", "当前可通信", "已进入后台，打开终端后恢复同步", "尚未连接", "IP：192.168.1.20", "上次 IP：192.168.1.20"),
    ] {
        for dark in [false, true] {
            let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
            cx.update(|cx| Theme::change(if dark { ThemeMode::Dark } else { ThemeMode::Light }, None, cx));
            cx.update_window(handle, |_, window, cx| {
                shell.update(cx, |this, cx| {
                    this.locale = locale; this.page = crate::app::Page::Devices; this.devices.snapshot.loaded = true;
                    this.devices.snapshot.saved.push(SavedDeviceSummary { id: DEVICE.into(), name: "Saved name".into() });
                    this.devices.snapshot.presence.push(DevicePresenceView { form_factor: None, device_id: DEVICE.into(),
                        info: Some(DeviceInfo { device_id: DEVICE.into(), name: "Authenticated phone".into(), platform: DevicePlatform::Ios, app_version: "1.0".into() }),
                        state: DeviceConnectionState::Reachable, peer: Some("192.168.1.20:51000".parse().unwrap()), since_last_contact: Some(std::time::Duration::ZERO), companion: None });
                    this.devices.snapshot.presence[0].companion = Some(WatchCompanionView {
                        info: WatchInfo { device_id: "c169da9b-4716-4774-bdcb-a33bfd6e1ac1".into(), name: "My Watch".into(), app_version: "1.0".into() },
                        reachable: true, since_last_contact: Some(std::time::Duration::ZERO), since_last_sync: Some(std::time::Duration::from_secs(2)) });
                    cx.notify();
                });
                window.render_frame(cx);
                assert_eq!(window.find(format!("device-status-{DEVICE}")).label(), Some(connected));
                assert_eq!(window.find(format!("device-status-tag-{DEVICE}")).label(), Some(if locale == "en-US" { "Online" } else { "在线" }));
                assert_eq!(window.find(format!("device-version-{DEVICE}")).label(), Some("1.0"));
                assert_eq!(window.find(format!("device-ip-{DEVICE}")).label(), Some(ip));
                assert_eq!(window.find(format!("device-watch-status-{DEVICE}")).label(), Some(connected));
                let online = if locale == "en-US" { "Online" } else { "在线" };
                let offline = if locale == "en-US" { "Offline" } else { "离线" };
                assert_eq!(window.find(format!("device-watch-status-tag-{DEVICE}")).label(), Some(online));
                for prefix in ["device-status", "device-watch-status"] {
                    assert_connection_tag_background(window, &format!("{prefix}-{DEVICE}"), cx.theme().success.opacity(0.12));
                }
                assert_eq!(window.find(format!("device-watch-sync-{DEVICE}")).label(), Some(if locale == "en-US" { "Last confirmed sync: at most 2s ago" } else { "最近确认同步：至多 2 秒前" }));
                assert!(window.try_find("device-ip-c169da9b-4716-4774-bdcb-a33bfd6e1ac1").is_none());
                assert!(window.try_find("device-remove-c169da9b-4716-4774-bdcb-a33bfd6e1ac1").is_none());
                shell.update(cx, |this, cx| { this.devices.snapshot.presence[0].companion.as_mut().unwrap().reachable = false; cx.notify(); });
                window.render_frame(cx);
                assert_eq!(window.find(format!("device-status-tag-{DEVICE}")).label(), Some(online), "手机在线不能代替Watch独立回执");
                assert_eq!(window.find(format!("device-watch-status-tag-{DEVICE}")).label(), Some(offline));
                assert_connection_tag_background(window, &format!("device-watch-status-{DEVICE}"), cx.theme().secondary);
                shell.update(cx, |this, cx| {
                    this.devices.snapshot.presence[0].companion.as_mut().unwrap().reachable = true;
                    this.devices.snapshot.presence[0].state = DeviceConnectionState::Unavailable(UnavailableReason::Background); cx.notify();
                });
                window.render_frame(cx);
                assert_eq!(window.find(format!("device-status-{DEVICE}")).label(), Some(background));
                assert_eq!(window.find(format!("device-status-tag-{DEVICE}")).label(), Some(if locale == "en-US" { "Offline" } else { "离线" }));
                assert_eq!(window.find(format!("device-ip-{DEVICE}")).label(), Some(last_ip));
                assert_eq!(window.find(format!("device-watch-status-{DEVICE}")).label(), Some(if locale == "en-US" { "Sync unavailable · cache retained" } else { "暂不可同步 · 保留缓存" }));
                assert_eq!(window.find(format!("device-watch-status-tag-{DEVICE}")).label(), Some(offline));
                for prefix in ["device-status", "device-watch-status"] {
                    assert_connection_tag_background(window, &format!("{prefix}-{DEVICE}"), cx.theme().secondary);
                }
                shell.update(cx, |this, cx| { this.devices.snapshot.presence.clear(); prepare_target(this); cx.notify(); });
                window.render_frame(cx);
                assert_eq!(window.find(format!("device-status-{DEVICE}")).label(), Some(not_connected));
                assert_eq!(window.find(format!("device-status-tag-{DEVICE}")).label(), Some(if locale == "en-US" { "Offline" } else { "离线" }));
                assert!(window.try_find(format!("device-ip-{DEVICE}")).is_none());
                assert!(window.try_find(format!("device-watch-{DEVICE}")).is_none());
                assert!(window.try_find(format!("device-watch-artwork-{DEVICE}")).is_none());
            }).unwrap();
        }
    }
}

/// 直接核对真实绘制Tag背景，防止在线只改文字却仍然使用品牌红色或离线误示绿色。
fn assert_connection_tag_background(window: &gpui_kit::Window, id: &str, color: gpui_kit::Hsla) {
    let bounds = window.find(id.to_owned()).bounds().scale(window.scale_factor());
    assert!(window.painted_quads().iter().any(|quad| quad.bounds == bounds && quad.background.as_solid() == Some(color)),
        "{id}必须绘制当前通信状态对应的背景色{color:?}");
}
