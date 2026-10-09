#[path = "../gpui-patch-support/generator.rs"]
mod generator;

use generator::{Patch, Spec};
use std::path::Path;

/// 校验不可变官方原包并生成带圆角视口补丁的完整 WGPU 编译树。
fn main() {
    let spec = Spec {
        entry: "src/gpui_wgpu.rs",
        root_prefix: include_str!("patches/root-prefix.before"),
        source_manifest_sha256: "49af2ca1842fcf48ce4d903972a531b8d0355b73a2c8af30ab466a5e068f60b7",
        patches: &[
            Patch {
                name: "polychrome-layout",
                path: "src/shaders.wgsl",
                before: include_str!("patches/polychrome-layout.before"),
                after: include_str!("patches/polychrome-layout.after"),
            },
            Patch {
                name: "polychrome-distance",
                path: "src/shaders.wgsl",
                before: include_str!("patches/polychrome-distance.before"),
                after: include_str!("patches/polychrome-distance.after"),
            },
            Patch {
                name: "webgl-polychrome-decoder",
                path: "src/shaders_webgl.wgsl",
                before: include_str!("patches/webgl-polychrome-decoder.before"),
                after: include_str!("patches/webgl-polychrome-decoder.after"),
            },
            Patch {
                name: "polychrome-word-stride",
                path: "src/wgpu_renderer.rs",
                before: include_str!("patches/polychrome-word-stride.before"),
                after: include_str!("patches/polychrome-word-stride.after"),
            },
            Patch {
                name: "shader-test-naga-import",
                path: "src/wgpu_renderer.rs",
                before: include_str!("patches/shader-test-naga-import.before"),
                after: include_str!("patches/shader-test-naga-import.after"),
            },
        ],
        generated_hashes: &[
            ("src/gpui_wgpu.rs", "b51033ac07d187c89c4079f208d3a557c6fff309b1cd403927ce8974e795afa0"),
            ("src/shaders.wgsl", "03d4c3ee901b800fe0beb775abfc191820affdec17bc38759129f3254c7c2e48"),
            ("src/shaders_webgl.wgsl", "d48a0ef2bd567c342f91f9b45ec4483de23e0f9ded11a4a232b3b46f9a43e6c9"),
            ("src/wgpu_renderer.rs", "d37fa295545b2ed128030f94ce428be168a05781fea78ece8e51d7f274f916eb"),
        ],
    };
    generator::generate(Path::new(env!("CARGO_MANIFEST_DIR")), &spec);
}
