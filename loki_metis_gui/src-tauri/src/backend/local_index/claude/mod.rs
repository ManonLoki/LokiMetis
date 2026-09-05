//! Claude Code 本机 transcript 的专属发现、解析与索引边界。
//!
//! 该模块不会读取认证文件、配置正文或历史命令，只允许进入通过结构签名的
//! `projects` transcript 区域。
//! 结构上和父模块 local_index 是同一套四段流程（发现->解析->扫描->索引），
//! 只是完全独立实现，物理隔离 Codex 与 Claude 两个客户端的判定规则，
//! 避免其中一个客户端的数据源逻辑变化意外影响另一个。

mod discovery; // Claude 数据根发现：只认可 `projects/<project>/<session>.jsonl` 结构签名
mod index; // Claude 专属的 LocalIndex 扩展方法（复用父模块同一 SQLite，物理隔离数据）
mod jsonl; // Claude transcript 的流式解析器
mod path_rules; // transcript 相对路径与文件名硬规则
mod scan; // Claude 数据根的扫描编排入口

pub use discovery::{
    ClaudeDiscoveredRoot, ClaudeDiscoveryInputs, ClaudeDiscoveryResult,
    discover_claude_full_device_with_progress, discover_claude_quick,
};
pub(crate) use discovery::{ClaudeRootInspection, ClaudeSignatureBudget, inspect_claude_root};
#[cfg(test)]
pub(crate) use jsonl::CLAUDE_PARSER_VERSION;
pub use scan::scan_claude_discovered_roots;
