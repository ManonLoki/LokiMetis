//! Agent 会话日志的有界流式解析：Codex rollout、Claude Code transcript、Grok updates 与 WorkBuddy project JSONL。
//!
//! 解析器只接收任意 `std::io::Read` 字节流，只反序列化用量与白名单元数据，不保留会话正文；
//! 打开文件、文件身份校验与目录遍历属于宿主驱动 crate。

pub mod claude;
pub mod codex;
pub mod grok;
pub mod workbuddy;
