//! 本模块聚合 GUI 侧 Source Client 入口能力。
//!
//! 扫描端口定义在 shared core；GUI 装配本机索引绑定与扫描器。

pub(crate) mod analysis;
mod capabilities;
mod scanners;

pub(crate) use capabilities::AgentClientCapabilities;
pub(crate) use scanners::{ClaudeLocalUsageScanner, CodexLocalUsageScanner, GrokLocalUsageScanner};
