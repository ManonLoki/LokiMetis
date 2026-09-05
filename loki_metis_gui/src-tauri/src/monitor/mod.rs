//! 本机 AgentHooks：配置写入、环回 listener 与图片库。

mod commands;
mod images;
mod listener;
mod pet_window;
mod relay;
mod settings;
mod store;

pub use commands::{
    close_pet_overlay, delete_monitor_image_cmd, get_hook_relay_status, get_monitor_capabilities,
    get_monitor_image_bytes, get_monitor_settings, get_pet_overlay_view, list_monitor_hook_locations,
    list_monitor_images_cmd, open_pet_overlay, save_hook_config_directory, save_monitor_enabled_tools,
    save_monitor_image_cmd, start_pet_overlay_drag, write_monitor_hook_config,
};
pub use images::{MonitorImageRecord, delete_monitor_image, list_monitor_images, save_monitor_image};
pub use pet_window::{
    PetOverlayViewDto, PetOverlayWindowDescription, close_pet_overlay_window,
    overlay_image_bytes, pet_overlay_view_from_images, pet_overlay_window_description,
    show_or_create_pet_overlay, start_pet_overlay_dragging,
};
pub use listener::{HookRelayStatus, spawn_hook_listener};
pub use relay::run_hook_relay_if_requested;
pub use settings::{MonitorSettings, load_monitor_settings, save_monitor_settings};
pub use store::{list_hook_config_locations, write_hook_config};

use loki_metis_core::{AiToolDescriptor, HookBehavior, ai_tool_descriptors};

/// 监控区静态能力，不含局域网发现。
#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MonitorCapabilities {
    /// 四项 Agent 目录。
    pub ai_tools: Vec<AiToolDescriptor>,
    /// 四态展示行为。
    pub hook_behaviors: Vec<HookBehavior>,
}

/// 返回监控区静态能力。
pub fn monitor_capabilities() -> MonitorCapabilities {
    MonitorCapabilities {
        ai_tools: ai_tool_descriptors(),
        hook_behaviors: HookBehavior::DISPLAY_BEHAVIORS.to_vec(),
    }
}
