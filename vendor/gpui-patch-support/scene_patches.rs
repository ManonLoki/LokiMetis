//! 两个后端共用的 scene.rs 补丁：裁剪布局、父圆角裁剪字段与已用圆角半径。
//! 上游 scene.rs 在两个包内相同，仅登记路径不同，故片段只维护一份。

use crate::generator::Patch;

/// 顺序固定；`path` 为 scene.rs 在各包上游树内的相对路径。
pub const fn all(path: &'static str) -> [Patch; 24] {
    [
        patch!("scene_patches/", "sprite-clip-layout", path),
        patch!("scene_patches/", "parent-clip-types", path),
        patch!("scene_patches/", "parent-clip-scene-fields", path),
        patch!("scene_patches/", "parent-clip-clear", path),
        patch!("scene_patches/", "parent-clip-insert", path),
        patch!("scene_patches/", "parent-clip-replay", path),
        patch!("scene_patches/", "parent-clip-quad-field", path),
        patch!("scene_patches/", "parent-clip-shadow-field", path),
        patch!("scene_patches/", "parent-clip-underline-field", path),
        patch!("scene_patches/", "parent-clip-monochromesprite-field", path),
        patch!("scene_patches/", "parent-clip-subpixelsprite-field", path),
        patch!("scene_patches/", "parent-clip-polychromesprite-field", path),
        patch!("scene_patches/", "parent-clip-paintsurface-field", path),
        patch!("scene_patches/", "parent-clip-path-field", path),
        patch!("scene_patches/", "parent-clip-path-new", path),
        patch!("scene_patches/", "parent-clip-path-scale", path),
        patch!("scene_patches/", "parent-clip-helpers", path),
        patch!("scene_patches/", "parent-clip-tests", path),
        patch!("scene_patches/", "parent-scene-import-visibility", path),
        patch!("scene_patches/", "parent-used-radii-constructor", path),
        patch!("scene_patches/", "parent-used-radii-contains", path),
        patch!("scene_patches/", "parent-used-radii-scale", path),
        patch!("scene_patches/", "parent-used-radii-chain-push", path),
        patch!("scene_patches/", "parent-used-radii-doc", path),
    ]
}
