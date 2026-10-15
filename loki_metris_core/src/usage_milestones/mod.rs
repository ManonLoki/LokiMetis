//! 每日首次突破 1B 的持久事件账本；变更先保存，再执行最多一个 OS 提交。

mod delivery;
mod state;
#[cfg(test)]
mod tests;
pub use delivery::*;
pub use state::*;

/// 十进制 1B；必须严格大于该整数才构成每日唯一事件。
pub const TOKEN_MILESTONE: u64 = 1_000_000_000;
/// 文件驱动在解码前与编码后均检查容量，超限不得继续发送。
pub const NOTIFICATION_STATE_LIMIT: usize = 256 * 1024;
