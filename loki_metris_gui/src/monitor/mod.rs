//! 当前产品独立的监控配置、环回事件宿主和位置快照。

pub(crate) mod settings;
mod atomic_file;
pub(crate) mod relay;
pub(crate) mod listener;
mod listener_state;
pub(crate) mod profiles;
pub(crate) mod images;
pub(crate) mod image_decode;
mod image_memory;
mod png_animation;
mod animation_metadata;
pub(crate) mod image_playback;
pub(crate) mod image_residency;
mod catalog;
mod thread_owner;
pub(crate) mod store;
mod hook_config_io;
mod wsl;
#[cfg(test)]
pub(crate) mod hook_test_support;
