//! 把 Codex/Claude/Grok 的本机发现根接入 core 的 [`DiscoveredRootIdentity`]，
//! 使三条索引管线共享 [`super::client::ScanClient`] 与 core 的发现合并规则。

use std::path::Path;

use loki_metris_core::DiscoveredRootIdentity;

use crate::local_index::{ClaudeDiscoveredRoot, DiscoveredRoot, GrokDiscoveredRoot};

/// 为带 `path`、`root_id`、`alias` 字段的发现根实现 [`DiscoveredRootIdentity`]。
///
/// 三个 Agent 的根类型刻意保持不同（不可混入同一发现结果），但身份字段与
/// 继承语义完全一致：路径用于合并匹配，`adopt_identity` 用登记库已确定的
/// 稳定 ID 与别名覆盖本轮临时身份。展开结果的方法语义由此处统一说明。
macro_rules! impl_discovered_root_identity {
    ($($root:ty),+ $(,)?) => {$(
        impl DiscoveredRootIdentity for $root {
            /// 返回该发现根的绝对路径。
            fn path(&self) -> &Path {
                &self.path
            }

            /// 返回该发现根当前的稳定 ID。
            fn root_id(&self) -> &str {
                &self.root_id
            }

            /// 用登记后确定的 ID 与别名覆盖临时发现身份。
            fn adopt_identity(&mut self, root_id: &str, alias: &str) {
                self.root_id = root_id.to_owned();
                self.alias = alias.to_owned();
            }
        }
    )+};
}

impl_discovered_root_identity!(DiscoveredRoot, ClaudeDiscoveredRoot, GrokDiscoveredRoot);
