//! 桌宠独立悬浮窗：规格映射、创建/关闭与宫格读模型。

use loki_metis_core::{
    AiTool, HookError, PetOverlayPosition, PetOverlayToolBehavior, PetOverlayView,
    PetOverlayWindowSpec, PetOverlayWorkArea, ai_tool_name, pet_overlay_window_spec,
    project_pet_overlay_from_drafts, resolve_pet_overlay_position,
};
use serde::Serialize;
use tauri::{
    AppHandle, Manager, PhysicalPosition, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

use super::images::read_monitor_image;
use super::profiles::load_profile_drafts;
use super::settings::load_monitor_settings;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// 浮窗移动防抖代数；只有最新一次 Moved 到期后才写入位置。
static PET_OVERLAY_MOVE_GENERATION: AtomicU64 = AtomicU64::new(0);

/// 已交付的悬浮窗描述，命令与测试共用，避免另写对照实现。
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PetOverlayWindowDescription {
    /// 窗口标签。
    pub label: &'static str,
    /// 前端路径。
    pub url_path: &'static str,
    /// 是否绘制系统标题栏。
    pub decorations: bool,
    /// 是否透明。
    pub transparent: bool,
    /// 是否始终置顶。
    pub always_on_top: bool,
    /// 是否跳过任务栏。
    pub skip_taskbar: bool,
    /// 是否隐藏主窗口。
    pub hide_main_window: bool,
    /// 是否停止 Hook listener。
    pub stop_hook_listener: bool,
    /// 默认宽度。
    pub default_width: f64,
    /// 默认高度。
    pub default_height: f64,
}

/// 前端宫格槽位。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PetOverlaySlotDto {
    /// 工具。
    pub tool: AiTool,
    /// 展示名。
    pub name: String,
    /// 是否有图。
    pub occupied: bool,
    /// 图库 ID。
    pub image_id: Option<String>,
}

/// 前端宫格快照。
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PetOverlayViewDto {
    /// 四槽。
    pub slots: Vec<PetOverlaySlotDto>,
}

/// 返回已交付悬浮窗规格映射。
pub fn pet_overlay_window_description() -> PetOverlayWindowDescription {
    let spec: PetOverlayWindowSpec = pet_overlay_window_spec();
    PetOverlayWindowDescription {
        label: spec.label,
        url_path: "/pet",
        decorations: spec.decorations,
        transparent: spec.transparent,
        always_on_top: spec.always_on_top,
        skip_taskbar: spec.skip_taskbar,
        hide_main_window: spec.hide_main_window,
        stop_hook_listener: spec.stop_hook_listener,
        default_width: spec.default_width,
        default_height: spec.default_height,
    }
}

/// 把已保存展示草稿与当前行为投影为桌宠宫格 DTO。
pub fn pet_overlay_view_from_drafts(
    config_dir: &Path,
    current_behaviors: &[PetOverlayToolBehavior],
) -> Result<PetOverlayViewDto, HookError> {
    let drafts = load_profile_drafts(config_dir)?;
    Ok(to_view_dto(project_pet_overlay_from_drafts(
        &drafts.drafts,
        current_behaviors,
    )))
}

/// 该窗口标签是否属于桌宠悬浮窗。
pub fn is_pet_overlay_label(window_label: &str) -> bool {
    window_label == pet_overlay_window_description().label
}

/// 把 pet 窗口的 Moved 事件转成待持久化快照；其它窗口或无效尺寸忽略。
pub fn pet_overlay_move_snapshot(
    window_label: &str,
    event_position: Option<PetOverlayPosition>,
    outer_size: (u32, u32),
) -> Option<(PetOverlayPosition, (u32, u32))> {
    if !is_pet_overlay_label(window_label) {
        return None;
    }
    let position = event_position?;
    if outer_size.0 == 0 || outer_size.1 == 0 {
        return None;
    }
    Some((position, outer_size))
}

/// 从宿主显示器读取工作区，供位置规范化使用。
pub fn pet_overlay_work_areas<R: Runtime>(app: &AppHandle<R>) -> Vec<PetOverlayWorkArea> {
    let description = pet_overlay_window_description();
    let monitors = app
        .get_webview_window(description.label)
        .and_then(|window| window.available_monitors().ok())
        .or_else(|| app.available_monitors().ok())
        .unwrap_or_default();
    monitors
        .into_iter()
        .map(|monitor| {
            let area = monitor.work_area();
            PetOverlayWorkArea {
                x: area.position.x,
                y: area.position.y,
                width: area.size.width,
                height: area.size.height,
            }
        })
        .collect()
}

fn live_pet_overlay_outer_size(window: &WebviewWindow) -> Option<(u32, u32)> {
    window
        .outer_size()
        .ok()
        .filter(|size| size.width > 0 && size.height > 0)
        .map(|size| (size.width, size.height))
}

fn apply_saved_pet_overlay_position(app: &AppHandle, window: &WebviewWindow) {
    let Ok(config_dir) = app.path().app_config_dir() else {
        return;
    };
    let Some(overlay_size) = live_pet_overlay_outer_size(window) else {
        return;
    };
    let stored = load_monitor_settings(&config_dir)
        .ok()
        .and_then(|settings| settings.pet_overlay_position);
    let work_areas = pet_overlay_work_areas(app);
    if let Some(position) = resolve_pet_overlay_position(stored, overlay_size, &work_areas) {
        let _ = window.set_position(PhysicalPosition::new(position.x, position.y));
    }
}

/// 打开或聚焦桌宠悬浮窗；不隐藏主窗口、不停止 listener。合法已保存位置会在显示前应用。
pub fn show_or_create_pet_overlay(app: &AppHandle) -> Result<PetOverlayWindowDescription, String> {
    let description = pet_overlay_window_description();
    if description.hide_main_window {
        return Err("pet overlay must keep the main window visible".to_owned());
    }
    if description.stop_hook_listener {
        return Err("pet overlay must not stop the hook listener".to_owned());
    }
    if let Some(existing) = app.get_webview_window(description.label) {
        apply_saved_pet_overlay_position(app, &existing);
        existing.show().map_err(|error| error.to_string())?;
        existing.unminimize().map_err(|error| error.to_string())?;
        existing
            .set_always_on_top(description.always_on_top)
            .map_err(|error| error.to_string())?;
        let _ = existing.set_focus();
        return Ok(description);
    }
    let window = WebviewWindowBuilder::new(
        app,
        description.label,
        WebviewUrl::App("index.html".into()),
    )
    .title("LokiMetis")
    .inner_size(description.default_width, description.default_height)
    .decorations(description.decorations)
    .transparent(description.transparent)
    .always_on_top(description.always_on_top)
    .skip_taskbar(description.skip_taskbar)
    .resizable(false)
    .visible(true)
    .build()
    .map_err(|error| error.to_string())?;
    apply_saved_pet_overlay_position(app, &window);
    Ok(description)
}

/// 把宿主读回的物理位置与实际 outer_size 交给 core 规范化后写入本机设置。
pub fn persist_pet_overlay_position(
    config_dir: &Path,
    position: PetOverlayPosition,
    overlay_size: (u32, u32),
    work_areas: &[PetOverlayWorkArea],
) -> Result<Option<PetOverlayPosition>, HookError> {
    let mut settings = load_monitor_settings(config_dir)?;
    let resolved = resolve_pet_overlay_position(Some(position), overlay_size, work_areas);
    settings.pet_overlay_position = resolved;
    super::settings::save_monitor_settings(config_dir, &settings)?;
    Ok(resolved)
}

/// 原生 `WindowEvent::Moved` 入口：按实际物理尺寸规范化后防抖写入。
/// 非桌宠窗口在读取 `outer_size` 前就返回，拖动主窗口不会产生额外的窗管往返。
pub fn schedule_pet_overlay_position_persist<R: Runtime>(
    window: &tauri::Window<R>,
    event_position: PetOverlayPosition,
) {
    if !is_pet_overlay_label(window.label()) {
        return;
    }
    let outer_size = window
        .outer_size()
        .map(|size| (size.width, size.height))
        .unwrap_or((0, 0));
    let Some((position, outer_size)) =
        pet_overlay_move_snapshot(window.label(), Some(event_position), outer_size)
    else {
        return;
    };
    let generation = PET_OVERLAY_MOVE_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    let app = window.app_handle().clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        if generation != PET_OVERLAY_MOVE_GENERATION.load(Ordering::Relaxed) {
            return;
        }
        let Ok(config_dir) = app.path().app_config_dir() else {
            return;
        };
        let work_areas = pet_overlay_work_areas(&app);
        let _ = persist_pet_overlay_position(&config_dir, position, outer_size, &work_areas);
    });
}

/// 查询桌宠悬浮窗当前是否可见；窗口不存在视为关闭。
pub fn pet_overlay_window_is_open(app: &AppHandle) -> bool {
    let description = pet_overlay_window_description();
    app.get_webview_window(description.label)
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false)
}

/// 隐藏桌宠悬浮窗，主窗口保持原状，不销毁窗口以便再次打开。
pub fn close_pet_overlay_window(app: &AppHandle) -> Result<PetOverlayWindowDescription, String> {
    let description = pet_overlay_window_description();
    if let Some(window) = app.get_webview_window(description.label) {
        window.hide().map_err(|error| error.to_string())?;
    }
    Ok(description)
}

/// 开始拖动无边框悬浮窗。
pub fn start_pet_overlay_dragging(app: &AppHandle) -> Result<(), String> {
    let description = pet_overlay_window_description();
    let window = app
        .get_webview_window(description.label)
        .ok_or_else(|| "pet overlay window is not open".to_owned())?;
    window.start_dragging().map_err(|error| error.to_string())
}

fn to_view_dto(view: PetOverlayView) -> PetOverlayViewDto {
    PetOverlayViewDto {
        slots: view
            .slots
            .into_iter()
            .map(|slot| PetOverlaySlotDto {
                name: if slot.name.is_empty() {
                    ai_tool_name(slot.tool).to_owned()
                } else {
                    slot.name
                },
                tool: slot.tool,
                occupied: slot.occupied,
                image_id: slot.image_key,
            })
            .collect(),
    }
}

/// 读取一张本机监控图字节。
pub fn overlay_image_bytes(app_data_dir: &Path, id: &str) -> Result<Vec<u8>, HookError> {
    read_monitor_image(app_data_dir, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use loki_metis_core::{PET_OVERLAY_WINDOW_SPEC, pet_overlay_window_spec};
    use tempfile::tempdir;

    #[test]
    fn pet_overlay_window_description_matches_core_spec() {
        let spec = pet_overlay_window_spec();
        let description = pet_overlay_window_description();
        assert_eq!(description.label, spec.label);
        assert!(!description.decorations);
        assert!(description.transparent);
        assert!(description.always_on_top);
        assert!(!description.hide_main_window);
        assert!(!description.stop_hook_listener);
        assert_eq!(description.url_path, "/pet");
        assert_eq!(description.decorations, PET_OVERLAY_WINDOW_SPEC.decorations);
        assert_eq!(
            description.hide_main_window,
            PET_OVERLAY_WINDOW_SPEC.hide_main_window
        );
    }

    #[test]
    fn pet_overlay_view_maps_saved_drafts_and_rejects_cursor() {
        let root = tempdir().expect("temp");
        let data = root.path();
        let config = root.path().join("config");
        std::fs::create_dir_all(&config).expect("config");
        const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let gallery = super::super::save_monitor_image(data, "avatar.png", PNG).expect("image");
        let image_id = gallery.images.last().expect("saved").id.clone();
        let mut draft = loki_metis_core::AiProfileDraft::default_for(loki_metis_core::AiTool::Codex);
        for hook in &mut draft.hooks {
            if hook.behavior == loki_metis_core::HookBehavior::Idle {
                hook.image = image_id.clone();
            }
        }
        super::super::save_profile_draft(&config, data, draft).expect("draft");
        let view = pet_overlay_view_from_drafts(&config, &[]).expect("view");
        assert_eq!(view.slots.len(), 4);
        assert_eq!(view.slots[0].name, "Codex");
        assert!(view.slots[0].occupied);
        assert_eq!(view.slots[0].image_id.as_deref(), Some(image_id.as_str()));
        assert!(!view.slots[1].occupied);
        assert_eq!(view.slots[1].name, "Claude Code");
        assert_eq!(view.slots[2].name, "Grok Build");
        assert_eq!(view.slots[3].name, "WorkBuddy");
        let joined = view
            .slots
            .iter()
            .map(|slot| slot.name.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(!joined.to_ascii_lowercase().contains("cursor"));
    }

    #[test]
    fn opening_uses_saved_position_when_it_intersects_work_area() {
        let stored = PetOverlayPosition { x: 240, y: 90 };
        let work_areas = [PetOverlayWorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        assert_eq!(
            resolve_pet_overlay_position(Some(stored), (360, 360), &work_areas),
            Some(stored)
        );
    }

    #[test]
    fn opening_falls_back_when_saved_position_is_offscreen() {
        let stored = PetOverlayPosition {
            x: 50_000,
            y: 50_000,
        };
        let work_areas = [PetOverlayWorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        assert_eq!(
            resolve_pet_overlay_position(Some(stored), (360, 360), &work_areas),
            None
        );
        assert_eq!(
            resolve_pet_overlay_position(None, (360, 360), &work_areas),
            None
        );
    }

    #[test]
    fn persist_then_open_reuses_saved_intersecting_position() {
        let root = tempdir().expect("temp");
        let config = root.path();
        let work_areas = [PetOverlayWorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        let saved = persist_pet_overlay_position(
            config,
            PetOverlayPosition { x: 424, y: 210 },
            (360, 360),
            &work_areas,
        )
        .expect("persist");
        assert_eq!(saved, Some(PetOverlayPosition { x: 424, y: 210 }));
        let loaded = super::super::load_monitor_settings(config).expect("load");
        assert_eq!(
            resolve_pet_overlay_position(loaded.pet_overlay_position, (360, 360), &work_areas),
            Some(PetOverlayPosition { x: 424, y: 210 })
        );
    }

    #[test]
    fn moved_event_on_pet_window_persists_physical_outer_size() {
        let position = PetOverlayPosition { x: -400, y: 100 };
        let snapshot = pet_overlay_move_snapshot("pet", Some(position), (720, 720));
        assert_eq!(snapshot, Some((position, (720, 720))));
        let root = tempdir().expect("temp");
        let work_areas = [PetOverlayWorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        let saved = persist_pet_overlay_position(root.path(), position, snapshot.unwrap().1, &work_areas)
            .expect("persist");
        assert_eq!(saved, Some(position));
    }

    #[test]
    fn moved_event_on_main_window_or_zero_size_is_ignored() {
        let position = PetOverlayPosition { x: 80, y: 80 };
        assert_eq!(
            pet_overlay_move_snapshot("main", Some(position), (720, 720)),
            None
        );
        assert_eq!(pet_overlay_move_snapshot("pet", Some(position), (0, 720)), None);
        assert_eq!(pet_overlay_move_snapshot("pet", None, (720, 720)), None);
    }

    #[test]
    fn retina_half_offscreen_is_kept_only_with_physical_outer_size() {
        let stored = PetOverlayPosition { x: -400, y: 100 };
        let work_areas = [PetOverlayWorkArea {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }];
        assert_eq!(
            resolve_pet_overlay_position(Some(stored), (720, 720), &work_areas),
            Some(stored)
        );
        assert_eq!(
            resolve_pet_overlay_position(Some(stored), (360, 360), &work_areas),
            None
        );
    }
}
