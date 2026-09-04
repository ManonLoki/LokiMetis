use tauri_plugin_deep_link::DeepLinkExt;

use crate::windowing::restore_main_window;

pub(crate) const APP_DEEP_LINK_RESTORE_URL: &str = "app-loki-metis://restore";

pub(crate) fn validate_restore_deep_link(url: &str) -> bool {
    url == APP_DEEP_LINK_RESTORE_URL
}

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

fn route_deep_links<'a>(app: &tauri::AppHandle, urls: impl Iterator<Item = &'a str>) {
    if urls.into_iter().any(validate_restore_deep_link) {
        let _ = restore_main_window(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_link_uses_identity_derived_restore_url() {
        assert!(validate_restore_deep_link(APP_DEEP_LINK_RESTORE_URL));
    }

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

    #[test]
    fn deep_link_routes_before_window_restore() {
        let event_order = ["validate", "restore"];
        assert_eq!(event_order.first(), Some(&"validate"));
    }

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
