//! 原生设备页面回归覆盖按钮所有权、双语模态、稳定草稿与跨页会话。

use gpui_kit::{AppContext, Role, TestAppContext, test::TestWindowExt};
use super::Shell;
use loki_metris_core::devices::{DiscoveredDevice, SavedDeviceSummary};

const DEVICE: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

/// 刷新位于发现栏头且双语可见；仅按钮触发发现，扫描和保存期间均阻止重复操作。
#[gpui_kit::test]
fn discovery_refresh_belongs_to_section_and_respects_busy_state(cx: &mut TestAppContext) {
    for (width, locale, refresh, scanning) in [
        (960., "zh-CN", "刷新", "正在发现…"),
        (1440., "en-US", "Refresh", "Discovering…"),
    ] {
        let (handle, shell, _finish) = crate::app::tests::open_shell(cx, width, 900.);
        cx.update_window(handle, |_, window, cx| {
            shell.update(cx, |this, cx| {
                this.locale = locale; this.page = crate::app::Page::Devices;
                this.devices.error = Some("device_timeout");
                cx.notify();
            });
            window.render_frame(cx);
            let header = window.find("devices-discovered-header").bounds();
            let button = window.find("devices-discover");
            assert_eq!(button.role(), Some(Role::Button));
            assert_eq!(button.label(), Some(refresh));
            assert!(button.bounds().left() >= header.left() && button.bounds().right() <= header.right());
            assert!(button.bounds().top() >= header.top() && button.bounds().bottom() <= header.bottom());
            assert!(button.bounds().left() >= window.find("devices-discovered-title").bounds().right());
            window.click("devices-discovered-header", cx);
            assert_eq!(shell.read(cx).devices.error, Some("device_timeout"));
            for (is_scanning, pending) in [(true, false), (false, true)] {
                shell.update(cx, |this, cx| {
                    this.devices.snapshot.scanning = is_scanning;
                    this.devices.snapshot.pending = pending;
                    cx.notify();
                });
                window.render_frame(cx);
                let button = window.find("devices-discover");
                assert_eq!(button.label(), Some(if is_scanning { scanning } else { refresh }));
                window.click("devices-discover", cx);
                assert_eq!(shell.read(cx).devices.error, Some("device_timeout"));
            }
            shell.update(cx, |this, cx| {
                this.devices.snapshot.pending = false;
                cx.notify();
            });
            window.render_frame(cx);
            window.click("devices-discover", cx);
            // 测试根没有服务，真实发现入口必须返回不可用，证明点击确实到达该入口。
            assert_eq!(shell.read(cx).devices.error, Some("device_unavailable"));
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
            let first_row = window.find(format!("device-found-{DEVICE}")).bounds();
            let second_row = window.find("device-found-aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").bounds();
            assert_eq!(first_row.size.width, second_row.size.width);
            assert!(second_row.top() >= first_row.bottom());
            assert!(window.try_find(format!("device-artwork-{DEVICE}")).is_none());
            let id = "00000000-0000-4000-8000-000000000000";
            let watch = window.find(format!("device-watch-{id}")).bounds();
            let details = window.find(format!("device-details-{id}")).bounds();
            assert!(watch.left() >= details.left() && watch.right() <= details.right() + px(1.));
            assert!(watch.bottom() <= details.bottom() + px(1.));
            for target in ["device-saved", "device-artwork", "device-name", "device-ip", "device-status", "device-version", "device-contact", "device-watch"] {
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
    for (locale, label, title) in [("en-US", "Pair device", "Terminal devices"), ("zh-CN", "配对设备", "终端设备")] {
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
    use loki_metris_core::devices::{companion::{WatchInfo, WatchCompanionView}, link::{DeviceInfo, DevicePlatform}, presence::{DevicePresenceView, DeviceConnectionState, UnavailableReason}};
    for (locale, connected, background, not_connected, ip, last_ip) in [
        ("en-US", "Connected", "In background · sync resumes when opened", "Not connected", "IP: 192.168.1.20", "Last IP: 192.168.1.20"),
        ("zh-CN", "当前可通信", "已进入后台，打开终端后恢复同步", "尚未连接", "IP：192.168.1.20", "上次 IP：192.168.1.20"),
    ] {
        let (handle, shell, _finish) = crate::app::tests::open_shell(cx, 1440., 900.);
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
            assert_eq!(window.find(format!("device-watch-sync-{DEVICE}")).label(), Some(if locale == "en-US" { "Last confirmed sync: at most 2s ago" } else { "最近确认同步：至多 2 秒前" }));
            assert!(window.try_find("device-ip-c169da9b-4716-4774-bdcb-a33bfd6e1ac1").is_none());
            assert!(window.try_find("device-remove-c169da9b-4716-4774-bdcb-a33bfd6e1ac1").is_none());
            shell.update(cx, |this, cx| { this.devices.snapshot.presence[0].state = DeviceConnectionState::Unavailable(UnavailableReason::Background); cx.notify(); });
            window.render_frame(cx);
            assert_eq!(window.find(format!("device-status-{DEVICE}")).label(), Some(background));
            assert_eq!(window.find(format!("device-status-tag-{DEVICE}")).label(), Some(if locale == "en-US" { "Offline" } else { "离线" }));
            assert_eq!(window.find(format!("device-ip-{DEVICE}")).label(), Some(last_ip));
            assert_eq!(window.find(format!("device-watch-status-{DEVICE}")).label(), Some(if locale == "en-US" { "Sync unavailable · cache retained" } else { "暂不可同步 · 保留缓存" }));
            shell.update(cx, |this, cx| { this.devices.snapshot.presence.clear(); prepare_target(this); cx.notify(); });
            window.render_frame(cx);
            assert_eq!(window.find(format!("device-status-{DEVICE}")).label(), Some(not_connected));
            assert_eq!(window.find(format!("device-status-tag-{DEVICE}")).label(), Some(if locale == "en-US" { "Offline" } else { "离线" }));
            assert!(window.try_find(format!("device-ip-{DEVICE}")).is_none());
        }).unwrap();
    }
}
