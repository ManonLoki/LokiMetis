#![allow(clippy::disallowed_methods, reason = "build scripts are exempt")]
#[path = "../gpui-patch-support/generator.rs"]
mod generator;
use generator::{Patch, Spec};
use std::path::Path;

/// 固定官方 15 文件逐字节核验，修改只发生于 OUT_DIR；可变字体补丁随上游 text_system 迁入 gpui-pre-apple。
fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for name in [
        "src/menu_tests.rs",
        "src/frame_demand.rs",
    ] {
        println!("cargo:rerun-if-changed={name}");
    }
    let spec = Spec {
        entry: "src/gpui_macos.rs",
        root_prefix: include_str!("patches/root-prefix.before"),
        source_manifest_sha256: "cac5202f3d5b7b69c5c7fd4b5f67a9811a3c24a1846edd8221bb0a46e325dd13",
        patches: &[
            Patch {
                name: "demand-window-handle", path: "src/window.rs",
                before: include_str!("patches/demand-window-handle.before"),
                after: include_str!("patches/demand-window-handle.after"),
            },
            Patch {
                name: "demand-window-handle-init", path: "src/window.rs",
                before: include_str!("patches/demand-window-handle-init.before"),
                after: include_str!("patches/demand-window-handle-init.after"),
            },
            Patch {
                name: "demand-native-waker", path: "src/window.rs",
                before: include_str!("patches/demand-native-waker.before"),
                after: include_str!("patches/demand-native-waker.after"),
            },
            Patch {
                name: "demand-metal-consume", path: "src/window.rs",
                before: include_str!("patches/demand-metal-consume.before"),
                after: include_str!("patches/demand-metal-consume.after"),
            },
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
            ("src/display_link.rs", "6777d274de042a1de4292cd2c05a3490ba9902c00d2d71a3c2e17eac2a3d01c9"),
            ("src/platform.rs", "2951979cf1034e2e4062d4dd0fbcdec1f6edc6d73cb96d1491ef719a93998f5e"),
            ("src/window.rs", "37b0fd143e1e029452a9bd0b596608525839ec5c193505d4f46fe65647421b5b"),
            ("src/gpui_macos.rs", "328fb915fd82f9a90a95924d11380ff5494d7f0240ceb1798dd07235e7759fe9"),
        ],
    };
    generator::generate(root, &spec);
}
