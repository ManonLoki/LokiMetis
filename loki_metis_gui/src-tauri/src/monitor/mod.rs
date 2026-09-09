//! 本机 AgentHooks：配置写入、环回 listener 与图片库。

mod atomic_file;
mod commands;
mod hook_config_io;
mod hook_config_migration;
mod images;
mod listener;
mod listener_state;
mod pet_commands;
mod pet_events;
mod pet_geometry;
mod pet_view;
mod pet_window;
mod profiles;
mod relay;
mod settings;
mod store;
mod thread_owner;
mod wsl;

pub use commands::{
    close_pet_overlay, delete_monitor_image_cmd, get_hook_relay_status, get_monitor_capabilities,
    get_monitor_image_bytes, get_monitor_settings, list_monitor_hook_locations,
    list_monitor_images_cmd, list_monitor_profile_drafts, save_enabled_ai_selection,
    save_hook_config_directory, save_monitor_image_cmd, save_monitor_profile_draft,
    start_pet_overlay_drag, write_monitor_hook_config,
};
pub use images::{delete_monitor_image, list_monitor_image_gallery, save_monitor_image};
pub use listener::{HookListenerControl, HookRelayStatus, spawn_hook_listener};
pub use pet_commands::{
    focus_first_populated_pet_page, get_pet_overlay_view, get_pet_window_state, hide_pet_settings,
    resize_pet_step, set_pet_always_on_top, set_pet_layout, set_pet_locked, set_pet_size,
    show_main_window, show_pet_settings, turn_pet_page,
};
pub use pet_events::emit_pet_window_state_changed;
pub use pet_view::overlay_image_bytes;
pub use pet_window::{
    PET_SETTINGS_LABEL, PetOverlayWindowDescription, close_pet_overlay_window,
    constrain_pet_overlay_to_current_monitor, handle_pet_overlay_resized, is_pet_settings_label,
    pet_overlay_window_description, pet_overlay_window_is_open,
    schedule_pet_overlay_position_persist, show_or_create_pet_overlay, start_pet_overlay_dragging,
};
pub use profiles::{load_profile_drafts, save_profile_draft};
pub use relay::run_hook_relay_if_requested;
pub use settings::{MonitorSettings, load_monitor_settings, update_monitor_settings};
pub use store::{HookConfigWriter, list_hook_config_locations, validate_hook_config_directory};

use loki_metis_core::{
    AiTool, HookBehavior, ImageUploadAccept, MonitorCapabilityRange, SkinHostKind,
    agent_wire_label, image_upload_accept, profile_slot_range, public_ai_capabilities,
};

/// 前端统一 Agent 面板使用的公开目录条目与区域能力映射。
#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PublicAiOption {
    /// 全局复选与 Hooks 使用的稳定工具值。
    pub tool: AiTool,
    /// 跨区域一致的用户可见名称。
    pub name: String,
    /// 看板支持时使用的 wire 值；不支持时为空。
    pub dashboard_client: Option<String>,
    /// 换皮支持时使用的宿主值；不支持时为空。
    pub skin_host: Option<SkinHostKind>,
}

/// 监控区静态能力，不含局域网发现。
#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MonitorCapabilities {
    /// 统一目录中当前公开且可由监控识别的 Agent。
    pub ai_tools: Vec<PublicAiOption>,
    /// 四态展示行为。
    pub hook_behaviors: Vec<HookBehavior>,
    /// 展示位闭区间。
    pub profile_slot: MonitorCapabilityRange,
    /// 本机图库上传格式。
    pub image_upload_accept: ImageUploadAccept,
}

/// 返回监控区静态能力。
pub fn monitor_capabilities() -> MonitorCapabilities {
    MonitorCapabilities {
        ai_tools: public_ai_capabilities()
            .iter()
            .filter_map(|capability| {
                capability.monitor_tool.map(|tool| PublicAiOption {
                    tool,
                    name: capability.name.to_owned(),
                    dashboard_client: capability
                        .dashboard_client
                        .map(agent_wire_label)
                        .map(str::to_owned),
                    skin_host: capability.skin_host,
                })
            })
            .collect(),
        hook_behaviors: HookBehavior::DISPLAY_BEHAVIORS.to_vec(),
        profile_slot: profile_slot_range(),
        image_upload_accept: image_upload_accept(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loki_metis_core::AiTool;

    /// 监控 IPC 目录只公开统一目录中可映射的五项，并保持产品顺序与名称。
    #[test]
    fn monitor_capabilities_publish_only_the_five_catalog_tools() {
        let capabilities = monitor_capabilities();
        assert_eq!(
            capabilities
                .ai_tools
                .iter()
                .map(|descriptor| (descriptor.tool, descriptor.name.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (AiTool::Codex, "Codex"),
                (AiTool::ClaudeCode, "Claude Code"),
                (AiTool::Cursor, "Cursor"),
                (AiTool::Grok, "Grok"),
                (AiTool::WorkBuddy, "WorkBuddy"),
            ]
        );
        assert_eq!(
            capabilities
                .ai_tools
                .iter()
                .filter_map(|descriptor| descriptor.skin_host.map(|host| (descriptor.tool, host)))
                .collect::<Vec<_>>(),
            vec![
                (AiTool::Codex, SkinHostKind::Codex),
                (AiTool::WorkBuddy, SkinHostKind::WorkBuddy),
            ]
        );
    }
}
