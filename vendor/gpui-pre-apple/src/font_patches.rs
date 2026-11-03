//! 可变字体补丁独立登记：0.3.9 起上游 text_system 由 gpui-pre-macos 迁入本包，补丁随之迁移。

use crate::generator::Patch;

/// 字体片段共享 text_system.rs 上下文，顺序不可调换；自有 helper 与回归经补丁 include! 引入。
pub const PATCHES: &[Patch] = &[
    patch!("../patches/", "font-native-identity-state", "src/text_system.rs"),
    patch!("../patches/", "font-requested-weight-instance", "src/text_system.rs"),
    patch!("../patches/", "font-native-registration-and-runs", "src/text_system.rs"),
    // 官方包未附带仓库外的Lilex测试字体，仅把include路径指向已核验原字节夹具。
    patch!("../patches/", "font-upstream-lilex-fixture", "src/text_system.rs"),
];
