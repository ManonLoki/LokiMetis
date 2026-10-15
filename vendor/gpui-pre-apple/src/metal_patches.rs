//! Metal 后端补丁独立登记：图集延迟回收、裁剪距离，以及父圆角裁剪的 renderer/shader 接线。

use crate::generator::Patch;

/// scene 图元补丁由两个后端共用，见 gpui-patch-support；此处只登记 Metal 专属片段。
pub const PATCHES: &[Patch] = &[
    patch!("../patches/", "atlas-retirement-submit", "src/metal_renderer.rs"),
    patch!("../patches/", "atlas-retirement-render-frame", "src/metal_renderer.rs"),
    patch!("../patches/", "atlas-retirement-frame", "src/metal_atlas.rs"),
    patch!("../patches/", "atlas-retirement-physical", "src/metal_atlas.rs"),
    patch!("../patches/", "atlas-retirement-deallocate", "src/metal_atlas.rs"),
    patch!("../patches/", "atlas-retirement-state", "src/metal_atlas.rs"),
    patch!("../patches/", "atlas-retirement-init", "src/metal_atlas.rs"),
    patch!("../patches/", "atlas-retirement-metal-tests", "src/metal_renderer.rs"),
    patch!("../patches/", "metal-clip-distance", "src/shaders.metal"),
    patch!("../patches/", "parent-metal-renderer-import-parent-clip", "src/metal_renderer.rs"),
    patch!("../patches/", "parent-metal-renderer-path-vertex-field", "src/metal_renderer.rs"),
    patch!("../patches/", "parent-metal-renderer-bind-nodes-in-frame", "src/metal_renderer.rs"),
    patch!("../patches/", "parent-metal-renderer-pass-nodes-to-path-intermediate", "src/metal_renderer.rs"),
    patch!("../patches/", "parent-metal-renderer-rebind-nodes-after-path-pass", "src/metal_renderer.rs"),
    patch!("../patches/", "parent-metal-renderer-path-intermediate-nodes-param", "src/metal_renderer.rs"),
    patch!("../patches/", "parent-metal-renderer-path-vertex-value", "src/metal_renderer.rs"),
    patch!("../patches/", "parent-metal-renderer-bind-nodes-in-path-intermediate", "src/metal_renderer.rs"),
    patch!("../patches/", "parent-metal-renderer-bind-helper-and-field", "src/metal_renderer.rs"),
    patch!("../patches/", "parent-metal-renderer-surface-bounds-field", "src/metal_renderer.rs"),
    patch!("../patches/", "parent-metal-shader-parent-clip-helpers", "src/shaders.metal"),
    patch!("../patches/", "parent-metal-shader-quad-unclipped-split", "src/shaders.metal"),
    patch!("../patches/", "parent-metal-shader-quad-parent-clip", "src/shaders.metal"),
    patch!("../patches/", "parent-metal-shader-shadow-unclipped-split", "src/shaders.metal"),
    patch!("../patches/", "parent-metal-shader-shadow-parent-clip", "src/shaders.metal"),
    patch!("../patches/", "parent-metal-shader-underline-unclipped-split", "src/shaders.metal"),
    patch!("../patches/", "parent-metal-shader-underline-parent-clip", "src/shaders.metal"),
    patch!("../patches/", "parent-metal-shader-monochrome-sprite-unclipped-split", "src/shaders.metal"),
    patch!("../patches/", "parent-metal-shader-monochrome-sprite-parent-clip", "src/shaders.metal"),
    patch!("../patches/", "parent-metal-shader-polychrome-sprite-unclipped-split", "src/shaders.metal"),
    patch!("../patches/", "parent-metal-shader-polychrome-sprite-parent-clip", "src/shaders.metal"),
    patch!("../patches/", "parent-metal-shader-path-unclipped-split", "src/shaders.metal"),
    patch!("../patches/", "parent-metal-shader-path-parent-clip", "src/shaders.metal"),
    patch!("../patches/", "parent-metal-shader-surface-vertex-parent-clip", "src/shaders.metal"),
    patch!("../patches/", "parent-metal-shader-surface-unclipped-split", "src/shaders.metal"),
    patch!("../patches/", "parent-metal-shader-surface-parent-clip", "src/shaders.metal"),
    // 0.3.9 上游新增的 Surface 渲染测试直接构造 PaintSurface，开启父裁剪时须补齐字段。
    patch!("../patches/", "parent-metal-surface-test-field", "src/metal_renderer.rs"),
];
