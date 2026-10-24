//! 监控图库图片在界面中的解码、内存预算、驻留与逐帧播放；图库存储与 Hook 宿主位于服务层 crate。

pub(crate) mod image_decode;
mod image_memory;
mod png_animation;
mod webp_budget;
mod animation_metadata;
pub(crate) mod image_playback;
pub(crate) mod image_residency;
