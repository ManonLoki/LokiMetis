#[path = "../gpui-patch-support/generator.rs"]
mod generator;
use generator::{Patch, Spec};
use std::path::Path;

/// 仅在 OUT_DIR 生成已验证的完整托盘源码，原包与补丁摘要漂移立即失败。
fn generate_sources() {
    let spec = Spec {
        entry: "src/lib.rs",
        root_prefix: include_str!("patches/root-prefix.before"),
        source_manifest_sha256: "43d8cea392f1ebe242cb4845548343384739e07fe43f35de7c09b07ed4bbbdc9",
        patches: &[
            Patch {
                name: "hresult-import", path: "src/platform_impl/windows/mod.rs",
                before: include_str!("patches/hresult-import.before"),
                after: include_str!("patches/hresult-import.after"),
            },
            Patch {
                name: "hresult-query", path: "src/platform_impl/windows/mod.rs",
                before: include_str!("patches/hresult-query.before"),
                after: include_str!("patches/hresult-query.after"),
            },
        ],
        generated_hashes: &[
            ("src/lib.rs", "cdcec413a64e036d10744f1ed9f2df36b68df59cc27e6f91f29f890c46799d2e"),
            ("src/platform_impl/windows/mod.rs", "e95589127c40d04762ba13d5ebdab6eb9a95ab3d6008bb5739eee4cbc9886bef"),
        ],
    };
    let _generated = generator::generate(Path::new(env!("CARGO_MANIFEST_DIR")), &spec);
}

/// 先验证源码，再保持上游 Linux/BSD 后端的环境监听与特性提示逻辑。
fn main() {
    generate_sources();
    println!("cargo::rerun-if-env-changed=CARGO_FEATURE_LIBAPPINDICATOR");
    println!("cargo::rerun-if-env-changed=CARGO_FEATURE_KSNI");

    let gtk_platform = matches!(
        std::env::var("CARGO_CFG_TARGET_OS").as_deref(),
        Ok("linux" | "dragonfly" | "freebsd" | "netbsd" | "openbsd")
    );

    if gtk_platform
        && std::env::var_os("CARGO_FEATURE_LIBAPPINDICATOR").is_some()
        && std::env::var_os("CARGO_FEATURE_KSNI").is_some()
    {
        println!(
            "cargo::warning=features `libappindicator` and `ksni` are enabled together; tray-icon will use the `ksni` backend"
        );
    }
}
