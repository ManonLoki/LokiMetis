//! Android JNI与Apple C ABI共用的安全状态机；unsafe仅位于独立ABI适配层。

#![deny(unsafe_op_in_unsafe_fn)]
mod abi;
mod state;
pub use state::MobileClient;

/// C/JNI调用输入上限，业务明文仍受共享会话的64KiB限制。
pub const MAX_COMMAND_BYTES: usize = 128 * 1024;
/// 调用方始终提供固定缓冲，容量不足时零状态变更，不重试已消费操作。
pub const RESULT_CAPACITY: usize = 192 * 1024;
