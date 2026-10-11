//! 每 100M 的持久事件账本；变更由文件驱动先保存，再执行最多一个 OS 提交。

mod delivery;
mod state;
#[cfg(test)]
mod tests;
pub use delivery::*;
pub use state::*;

/// 十进制 100M，业务阈值不接受展示缩写或浮点数。
pub const TOKEN_MILESTONE: u64 = 100_000_000;
/// 文件驱动在解码前与编码后均检查容量，超限不得继续发送。
pub const NOTIFICATION_STATE_LIMIT: usize = 256 * 1024;
