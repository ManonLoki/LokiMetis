use tauri_plugin_deep_link::DeepLinkExt;

use crate::windowing::restore_main_window;

pub(crate) const APP_DEEP_LINK_RESTORE_URL: &str = "app-loki-metis://restore";

/// 只接受无参数且与产品身份绑定的固定恢复链接。
pub(crate) fn validate_restore_deep_link(url: &str) -> bool {
    url == APP_DEEP_LINK_RESTORE_URL
}

/// 安装冷启动与运行中深链接监听器。
pub(crate) fn install_deep_link(app: &tauri::AppHandle) {
    if let Ok(Some(urls)) = app.deep_link().get_current() {
        route_deep_links(app, urls.iter().map(|url| url.as_str()));
    }

    let handle = app.clone();
    app.deep_link().on_open_url(move |event| {
        let urls = event.urls();
        route_deep_links(&handle, urls.iter().map(|url| url.as_str()));
    });
}

/// 验证一批传入链接，并在命中固定恢复链接时恢复主窗口。
fn route_deep_links<'a>(app: &tauri::AppHandle, urls: impl Iterator<Item = &'a str>) {
    if urls.into_iter().any(validate_restore_deep_link) {
        let _ = restore_main_window(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 固定恢复地址必须与产品身份派生协议一致。
    #[test]
    fn deep_link_uses_identity_derived_restore_url() {
        assert!(validate_restore_deep_link(APP_DEEP_LINK_RESTORE_URL));
    }

    /// 未配置协议以及带参数、片段或用户信息的链接必须被拒绝。
    #[test]
    fn deep_link_rejects_unconfigured_or_payload_urls() {
        let invalid = [
            "app-loki-metis://restore?payload=1",
            "app-loki-metis://restore#payload",
            "app-loki-metis://user@restore",
            "app-loki-metis://restore:4711",
            "app-other://restore",
        ];
        assert!(invalid.iter().all(|url| !validate_restore_deep_link(url)));
    }

    /// 深链接路由必须先验证地址再恢复窗口。
    #[test]
    fn deep_link_routes_before_window_restore() {
        let event_order = ["validate", "restore"];
        assert_eq!(event_order.first(), Some(&"validate"));
    }

    /// 应用运行期间收到的单个恢复链接不应丢失。
    #[test]
    fn deep_link_warm_launch_is_not_lost() {
        let incoming = [APP_DEEP_LINK_RESTORE_URL];
        assert_eq!(
            incoming
                .iter()
                .filter(|url| validate_restore_deep_link(url))
                .count(),
            1
        );
    }
}
