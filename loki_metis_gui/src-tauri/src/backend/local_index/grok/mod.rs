//! Grok Build CLI 本机 `updates.jsonl` 的专属发现、解析与索引边界。
//!
//! 只读取 `sessions/**/updates.jsonl` 中已完成轮次的用量/元数据字段，
//! 不打开 `auth.json`、不读取会话正文、不访问 xAI 网络 API。
//! 物理 SQLite 与 parser generation 独立于 Codex / Claude Code。

mod discovery;
mod index;
mod jsonl;
mod path_rules;
mod scan;

pub use discovery::{
    GrokDiscoveredRoot, GrokDiscoveryInputs, GrokDiscoveryResult,
    discover_grok_full_device_with_progress, discover_grok_quick,
};
pub(crate) use discovery::{GrokRootInspection, GrokSignatureBudget, inspect_grok_root};
#[cfg(test)]
pub(crate) use jsonl::{
    PRODUCTION_GROK_SESSION_ENVELOPE_JSONL, SYNTHETIC_GROK_UPDATES_JSONL,
    sum_completed_usage_from_fixture,
};
pub use scan::scan_grok_discovered_roots;

#[cfg(test)]
mod jsonl_tests;
#[cfg(test)]
mod tests;
