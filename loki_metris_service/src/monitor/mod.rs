//! 当前产品独立的监控配置、环回事件宿主、图库存储和位置快照；不含图片解码与播放。

pub mod settings;
mod atomic_file;
pub mod relay;
pub mod listener;
mod listener_state;
pub mod profiles;
pub mod images;
mod catalog;
mod thread_owner;
pub mod store;
mod hook_config_io;
mod wsl;
#[cfg(any(test, feature = "test-support"))]
pub mod hook_test_support;
