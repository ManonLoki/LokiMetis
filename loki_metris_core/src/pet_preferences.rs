//! 桌宠偏好与显示器无关的尺寸、分页规则。

use crate::{HookError, PetLayout, wrap_pet_overlay_page};
use serde::{Deserialize, Serialize};

/// 桌宠窗口的持久偏好；可见性只属于当前会话，不在这里落盘。
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PetWindowPreferences {
    /// 当前桌宠布局。
    #[serde(default, deserialize_with = "deserialize_pet_layout")]
    pub layout: PetLayout,
    /// 当前页首个位置的零基索引。
    #[serde(default, deserialize_with = "deserialize_focused_slot")]
    pub focused_slot: u8,
    /// 单格逻辑像素边长。
    #[serde(
        default = "default_pet_size",
        deserialize_with = "deserialize_pet_size"
    )]
    pub pet_size: u16,
    /// 是否始终置顶。
    #[serde(
        default = "default_true",
        deserialize_with = "deserialize_default_true"
    )]
    pub always_on_top: bool,
    /// 是否锁定拖拽和缩放。
    #[serde(default, deserialize_with = "deserialize_default_false")]
    pub locked: bool,
    /// 整个桌宠浮窗是否把鼠标交给下方窗口；独立于拖动和缩放锁定。
    #[serde(default, deserialize_with = "deserialize_default_false")]
    pub mouse_passthrough: bool,
}

impl Default for PetWindowPreferences {
    /// 返回 2×2、64 像素、置顶且未锁定的桌宠默认偏好。
    fn default() -> Self {
        Self {
            layout: PetLayout::Grid,
            focused_slot: 0,
            pet_size: default_pet_size(),
            always_on_top: true,
            locked: false,
            mouse_passthrough: false,
        }
    }
}

/// 默认桌宠单格边长，与 AIMonitorDesktop 的 64 逻辑像素基线一致。
const fn default_pet_size() -> u16 {
    64
}

/// serde 使用的 true 产品默认值。
const fn default_true() -> bool {
    true
}

/// 把单个畸形 JSON 字段恢复为产品默认，而不连坐整个设置文件。
fn deserialize_or_default<'de, D, T>(
    deserializer: D,
    fallback: impl FnOnce() -> T,
) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(T::deserialize(value).unwrap_or_else(|_| fallback()))
}

/// 解析布局，畸形值恢复为默认 2×2。
fn deserialize_pet_layout<'de, D>(deserializer: D) -> Result<PetLayout, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_or_default(deserializer, PetLayout::default)
}

/// 解析页首位置，畸形值恢复为第一页。
fn deserialize_focused_slot<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_or_default(deserializer, || 0)
}

/// 解析单格大小，畸形值恢复为 64。
fn deserialize_pet_size<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_or_default(deserializer, default_pet_size)
}

/// 解析默认为 true 的布尔偏好。
fn deserialize_default_true<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_or_default(deserializer, default_true)
}

/// 解析默认为 false 的布尔偏好。
fn deserialize_default_false<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserialize_or_default(deserializer, || false)
}

/// 规范化历史持久偏好，运行态尺寸另按当前显示器收敛。
pub fn normalize_pet_window_preferences(
    mut preferences: PetWindowPreferences,
) -> PetWindowPreferences {
    preferences.focused_slot = preferences.focused_slot.min(11);
    preferences.pet_size = preferences.pet_size.clamp(32, 2_048);
    preferences
}

/// 当前显示器工作区允许的单格逻辑尺寸。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PetSizeRange {
    /// 原生无边框缩放热区的安全最小尺寸。
    pub min: u16,
    /// 布局最长轴不超过工作区短边四分之一。
    pub max: u16,
}
impl PetSizeRange {
    /// 宿主只提供逻辑工作区短边；无有效显示器时按1440逻辑像素回退。
    pub fn for_work_area(shortest: f64, layout: PetLayout) -> Self {
        let shortest = if shortest.is_finite() && shortest > 0. {
            shortest
        } else {
            1440.
        };
        let (rows, columns) = layout.dimensions();
        Self {
            min: 64,
            max: (shortest / 4. / rows.max(columns) as f64)
                .floor()
                .clamp(64., 2048.) as u16,
        }
    }
    /// 运行态夹紧，不改变尚未提交的历史记录。
    pub fn constrain(self, requested: u16) -> u16 {
        requested.clamp(self.min, self.max.max(self.min))
    }
}

/// 只携带意图，字段合并与校验由core负责。
#[derive(Clone, Copy, Debug)]
pub enum PetPreferenceChange {
    /// 修改布局并保留当前绝对位置所在页。
    Layout(PetLayout),
    /// 滑块提交绝对尺寸，越界显式拒绝。
    Size(u16),
    /// 原生边缘缩放已换算的单格尺寸。
    NativeSize(u16),
    /// Ctrl/Cmd滚轮以24像素增减，锁定时忽略。
    ResizeStep(bool),
    /// 置顶开关。
    AlwaysOnTop(bool),
    /// 同时锁定拖动与原生缩放。
    Locked(bool),
    /// 鼠标穿透开关；配置窗口和恢复入口仍可操作。
    MousePassthrough(bool),
    /// 循环翻页方向。
    TurnPage(isize),
    /// 自动定位有图的绝对位置。
    FocusedSlot(u8),
}

/// 对最新权威偏好应用单个意图，禁止旧窗口整快照覆盖其它字段。
pub fn change_pet_preferences(
    mut value: PetWindowPreferences,
    change: PetPreferenceChange,
    range: PetSizeRange,
) -> Result<PetWindowPreferences, HookError> {
    match change {
        PetPreferenceChange::Layout(layout) => {
            value.layout = layout;
            value.pet_size = range.constrain(value.pet_size);
            value.focused_slot = (usize::from(value.focused_slot.min(11)) / layout.capacity()
                * layout.capacity()) as u8;
        }
        PetPreferenceChange::Size(size) => {
            if size < range.min || size > range.max {
                return Err(HookError::new("error.monitor.petSizeInvalid")
                    .param("min", range.min.to_string())
                    .param("max", range.max.to_string()));
            }
            value.pet_size = size;
        }
        PetPreferenceChange::NativeSize(size) => {
            if !value.locked {
                value.pet_size = range.constrain(size);
            }
        }
        PetPreferenceChange::ResizeStep(grow) => {
            if !value.locked {
                value.pet_size = range.constrain(if grow {
                    value.pet_size.saturating_add(24)
                } else {
                    value.pet_size.saturating_sub(24)
                });
            }
        }
        PetPreferenceChange::AlwaysOnTop(enabled) => value.always_on_top = enabled,
        PetPreferenceChange::Locked(locked) => value.locked = locked,
        PetPreferenceChange::MousePassthrough(enabled) => value.mouse_passthrough = enabled,
        PetPreferenceChange::TurnPage(delta) => {
            let page = usize::from(value.focused_slot.min(11)) / value.layout.capacity();
            value.focused_slot =
                (wrap_pet_overlay_page(value.layout, page, delta) * value.layout.capacity()) as u8;
        }
        PetPreferenceChange::FocusedSlot(slot) => value.focused_slot = slot.min(11),
    }
    Ok(value)
}

/// 布局单元格尺寸转换为原生窗口逻辑宽高。
pub fn pet_window_dimensions(layout: PetLayout, size: u16) -> (f64, f64) {
    let (rows, columns) = layout.dimensions();
    (
        f64::from(size) * columns as f64,
        f64::from(size) * rows as f64,
    )
}

/// 根据改变最多的轴收敛原生边缘缩放，保留当前布局比例。
pub fn pet_size_from_resize(
    layout: PetLayout,
    width: f64,
    height: f64,
    previous: u16,
    range: PetSizeRange,
) -> u16 {
    if !width.is_finite() || !height.is_finite() || width <= 0. || height <= 0. {
        return range.constrain(previous);
    }
    let (rows, columns) = layout.dimensions();
    let (old_width, old_height) = pet_window_dimensions(layout, previous);
    let cell = if (width - old_width).abs() >= (height - old_height).abs() {
        width / columns as f64
    } else {
        height / rows as f64
    };
    range.constrain(cell.round().clamp(0., u16::MAX as f64) as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 旧配置和畸形字段默认关闭，合法值往返保留且不连坐其它偏好。
    #[test]
    fn mouse_passthrough_legacy_default_and_roundtrip() {
        for raw in [r#"{}"#, r#"{"mousePassthrough":"true","locked":true}"#, r#"{"mousePassthrough":null}"#] {
            let prefs: PetWindowPreferences = serde_json::from_str(raw).unwrap();
            assert!(!prefs.mouse_passthrough);
            if raw.contains("locked") { assert!(prefs.locked); }
        }
        let prefs: PetWindowPreferences = serde_json::from_str(r#"{"mousePassthrough":true,"locked":false,"petSize":80}"#).unwrap();
        assert!(prefs.mouse_passthrough);
        assert!(!prefs.locked);
        assert_eq!(serde_json::from_value::<PetWindowPreferences>(serde_json::to_value(prefs).unwrap()).unwrap(), prefs);
    }
    /// 穿透只合并自身字段，锁定也不阻止取消穿透或普通分页。
    #[test]
    fn mouse_passthrough_intent_is_independent_of_lock_and_geometry() {
        let old = PetWindowPreferences { layout: PetLayout::Row3, focused_slot: 6, pet_size: 80, always_on_top: false, locked: true, mouse_passthrough: false };
        let range = PetSizeRange { min: 64, max: 90 };
        let enabled = change_pet_preferences(old, PetPreferenceChange::MousePassthrough(true), range).unwrap();
        assert_eq!(enabled, PetWindowPreferences { mouse_passthrough: true, ..old });
        assert_eq!(change_pet_preferences(enabled, PetPreferenceChange::MousePassthrough(false), range).unwrap(), old);
        let paged = change_pet_preferences(enabled, PetPreferenceChange::TurnPage(1), range).unwrap();
        assert!(paged.mouse_passthrough && paged.locked);
        assert_eq!(paged.focused_slot, 9);
    }
    /// 高DPI宿主先换算逻辑尺寸，布局最长轴约束且异常显示器回退。
    #[test]
    fn work_area_ranges_and_native_resize_follow_layout() {
        let range = PetSizeRange::for_work_area(1080., PetLayout::Grid);
        assert_eq!(range, PetSizeRange { min: 64, max: 135 });
        assert_eq!(PetSizeRange::for_work_area(1080., PetLayout::Row3).max, 90);
        assert_eq!(
            PetSizeRange::for_work_area(f64::NAN, PetLayout::Single).max,
            360
        );
        assert_eq!(PetSizeRange::for_work_area(100., PetLayout::Grid).max, 64);
        assert_eq!(
            pet_size_from_resize(PetLayout::Row, 180., 64., 64, range),
            90
        );
        assert_eq!(
            pet_size_from_resize(PetLayout::Column, 64., 180., 64, range),
            90
        );
        assert_eq!(pet_window_dimensions(PetLayout::Column3, 80), (80., 240.));
    }
    /// 分页保留绝对位置，锁定只阻止原生／滚轮缩放，非法滑块值不提交。
    #[test]
    fn preference_intents_preserve_fields_and_enforce_boundaries() {
        let range = PetSizeRange::for_work_area(1080., PetLayout::Single);
        let value = PetWindowPreferences {
            focused_slot: 7,
            ..Default::default()
        };
        let value =
            change_pet_preferences(value, PetPreferenceChange::Layout(PetLayout::Single), range)
                .unwrap();
        assert_eq!(value.focused_slot, 7);
        let value = change_pet_preferences(value, PetPreferenceChange::TurnPage(5), range).unwrap();
        assert_eq!(value.focused_slot, 0);
        let mut value =
            change_pet_preferences(value, PetPreferenceChange::ResizeStep(true), range).unwrap();
        assert_eq!(value.pet_size, 88);
        value.locked = true;
        assert_eq!(
            change_pet_preferences(value, PetPreferenceChange::ResizeStep(true), range)
                .unwrap()
                .pet_size,
            88
        );
        assert_eq!(
            change_pet_preferences(value, PetPreferenceChange::NativeSize(150), range)
                .unwrap()
                .pet_size,
            88
        );
        assert!(change_pet_preferences(value, PetPreferenceChange::Size(271), range).is_err());
        assert_eq!(
            change_pet_preferences(value, PetPreferenceChange::TurnPage(-1), range)
                .unwrap()
                .focused_slot,
            11
        );
    }
}
