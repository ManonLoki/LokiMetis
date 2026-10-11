#![allow(clippy::disallowed_methods, reason = "build scripts are exempt")]
mod fixture_verify;
#[path = "../gpui-patch-support/generator.rs"]
mod generator;
use generator::{Patch, Spec};
use std::path::Path;

/// 固定官方 17 文件与七份 OFL 夹具均逐字节核验，修改只发生于 OUT_DIR。
fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for name in [
        "src/variable_font.rs",
        "src/semantic_identity.rs",
        "src/descriptor_identity.rs",
        "src/variable_font_tests.rs",
        "src/descriptor_identity_tests.rs",
        "src/menu_tests.rs",
        "src/frame_demand.rs",
        "fixture_verify.rs",
        "test-fixtures",
        "test-fixtures.sha256",
    ] {
        println!("cargo:rerun-if-changed={name}");
    }
    fixture_verify::verify(root).unwrap_or_else(|error| panic!("{error}"));
    let spec = Spec {
        entry: "src/gpui_macos.rs",
        root_prefix: include_str!("patches/root-prefix.before"),
        source_manifest_sha256: "4b29969a342cfbc80229ca6690ec4f29a1954ddd50470a0106d794e669edf406",
        patches: &[
            Patch {
                name: "menu-action-ownership",
                path: "src/platform.rs",
                before: include_str!("patches/menu-action-ownership.before"),
                after: include_str!("patches/menu-action-ownership.after"),
            },
            Patch {
                name: "menu-command-unlock",
                path: "src/platform.rs",
                before: include_str!("patches/menu-command-unlock.before"),
                after: include_str!("patches/menu-command-unlock.after"),
            },
            Patch {
                name: "menu-validation-unlock",
                path: "src/platform.rs",
                before: include_str!("patches/menu-validation-unlock.before"),
                after: include_str!("patches/menu-validation-unlock.after"),
            },
            Patch {
                name: "menu-tests",
                path: "src/platform.rs",
                before: include_str!("patches/menu-tests.before"),
                after: include_str!("patches/menu-tests.after"),
            },
            Patch {
                name: "reopen-registration",
                path: "src/platform.rs",
                before: include_str!("patches/reopen-registration.before"),
                after: include_str!("patches/reopen-registration.after"),
            },
            Patch {
                name: "reopen-event",
                path: "src/platform.rs",
                before: include_str!("patches/reopen-event.before"),
                after: include_str!("patches/reopen-event.after"),
            },
            Patch {
                name: "zoom-target-registration",
                path: "src/window.rs",
                before: include_str!("patches/zoom-target-registration.before"),
                after: include_str!("patches/zoom-target-registration.after"),
            },
            Patch {
                name: "zoom-target-event",
                path: "src/window.rs",
                before: include_str!("patches/zoom-target-event.before"),
                after: include_str!("patches/zoom-target-event.after"),
            },
            Patch {
                name: "fullscreen-failure-registration",
                path: "src/window.rs",
                before: include_str!("patches/fullscreen-failure-registration.before"),
                after: include_str!("patches/fullscreen-failure-registration.after"),
            },
            Patch {
                name: "fullscreen-failure-event",
                path: "src/window.rs",
                before: include_str!("patches/fullscreen-failure-event.before"),
                after: include_str!("patches/fullscreen-failure-event.after"),
            },
            Patch {
                name: "font-native-identity-state",
                path: "src/text_system.rs",
                before: include_str!("patches/font-native-identity-state.before"),
                after: include_str!("patches/font-native-identity-state.after"),
            },
            Patch {
                name: "font-requested-weight-instance",
                path: "src/text_system.rs",
                before: include_str!("patches/font-requested-weight-instance.before"),
                after: include_str!("patches/font-requested-weight-instance.after"),
            },
            Patch {
                name: "font-native-registration-and-runs",
                path: "src/text_system.rs",
                before: include_str!("patches/font-native-registration-and-runs.before"),
                after: include_str!("patches/font-native-registration-and-runs.after"),
            },
            Patch {
                name: "demand-window-state", path: "src/window.rs",
                before: include_str!("patches/demand-window-state.before"),
                after: include_str!("patches/demand-window-state.after"),
            },
            Patch {
                name: "demand-window-init", path: "src/window.rs",
                before: include_str!("patches/demand-window-init.before"),
                after: include_str!("patches/demand-window-init.after"),
            },
            Patch {
                name: "demand-window-source", path: "src/window.rs",
                before: include_str!("patches/demand-window-source.before"),
                after: include_str!("patches/demand-window-source.after"),
            },
            Patch {
                name: "demand-window-waker", path: "src/window.rs",
                before: include_str!("patches/demand-window-waker.before"),
                after: include_str!("patches/demand-window-waker.after"),
            },
            Patch {
                name: "demand-unocclude", path: "src/window.rs",
                before: include_str!("patches/demand-unocclude.before"),
                after: include_str!("patches/demand-unocclude.after"),
            },
            Patch {
                name: "demand-screen-change", path: "src/window.rs",
                before: include_str!("patches/demand-screen-change.before"),
                after: include_str!("patches/demand-screen-change.after"),
            },
            Patch {
                name: "demand-activation-consume", path: "src/window.rs",
                before: include_str!("patches/demand-activation-consume.before"),
                after: include_str!("patches/demand-activation-consume.after"),
            },
            Patch {
                name: "demand-layer-consume", path: "src/window.rs",
                before: include_str!("patches/demand-layer-consume.before"),
                after: include_str!("patches/demand-layer-consume.after"),
            },
            Patch {
                name: "demand-main-dispatch", path: "src/window.rs",
                before: include_str!("patches/demand-main-dispatch.before"),
                after: include_str!("patches/demand-main-dispatch.after"),
            },
            Patch {
                name: "demand-subscriber", path: "src/display_link.rs",
                before: include_str!("patches/demand-subscriber.before"),
                after: include_str!("patches/demand-subscriber.after"),
            },
            Patch {
                name: "demand-vsync-gate", path: "src/display_link.rs",
                before: include_str!("patches/demand-vsync-gate.before"),
                after: include_str!("patches/demand-vsync-gate.after"),
            },
            Patch {
                name: "demand-subscribe-argument", path: "src/display_link.rs",
                before: include_str!("patches/demand-subscribe-argument.before"),
                after: include_str!("patches/demand-subscribe-argument.after"),
            },
            Patch {
                name: "demand-subscribe-state", path: "src/display_link.rs",
                before: include_str!("patches/demand-subscribe-state.before"),
                after: include_str!("patches/demand-subscribe-state.after"),
            },
            Patch {
                name: "demand-frame-source", path: "src/display_link.rs",
                before: include_str!("patches/demand-frame-source.before"),
                after: include_str!("patches/demand-frame-source.after"),
            },

        ],
        generated_hashes: &[
            ("src/display_link.rs", "0250b29688658285c250a340ae13068eb379e0a0b626d17c759a132a5932fadb"),
            ("src/platform.rs", "879ef085b1978afddf3e9b3b4f1269073b93e092c919a5f0648b6cff4bb63e26"),
            ("src/window.rs", "879fe244c79cff2ac45a9ca542137594404a9a5a475fa8ae60feff1b1d46ef75"),
            ("src/gpui_macos.rs", "5c784ad4ba70a5454f1805cdd3e6ac4a4002bae77c856db8546832683cc7b870"),
            ("src/text_system.rs", "b25ab17aee418545a62ae1d604845ac2a0f5922c4d7fca967d629142fa9f59eb"),
        ],
    };
    generator::generate(root, &spec);
}
