//! 原生确认、字段保存和失败回滚是一个串行 UI 事务；磁盘写入仍由应用服务拥有。

use super::{Controller, native_settle};
use gpui_kit::{Bounds, Context, Pixels};
use loki_metris_core::{HookError, PetPreferenceChange, PetSizeRange, PetWindowPreferences};

/// 单个字段意图的回滚资料只属于当前事务，不作为第二份权威偏好。
pub(super) struct Request {
    /// 本次要保存的字段意图。
    pub change: PetPreferenceChange,
    /// 当前屏幕下的桌宠尺寸范围，交给 core 校验。
    pub range: PetSizeRange,
    /// 事务开始前的偏好，失败时据此回滚。
    pub old: PetWindowPreferences,
    /// 原生几何变化前的浮窗矩形；拖边产生的尺寸变化为 None，失败时按旧偏好重算。
    pub old_bounds: Option<Bounds<Pixels>>,
    /// 本次改变了原生几何（布局、尺寸、置顶或锁定）。
    pub geometry: bool,
    /// 需要等待原生确认：几何变化或鼠标穿透切换。
    pub native: bool,
    /// 偏好确有变化，需要交给服务保存；纯原生尺寸校正为 false。
    pub commit: bool,
    /// 同步应用原生变化时已发生的错误；存在时直接进入回滚。
    pub initial_error: Option<HookError>,
    /// 事务所属的偏好代次，迟到回调据此判断是否已被取代。
    pub generation: u64,
}

impl Controller {
    /// 先异步确认原生效果再交服务；失败回滚也确认完成后才放行下一项。
    pub(super) fn start_preference_mutation(&mut self, request: Request, cx: &mut Context<Self>) {
        let handle = self.overlay;
        self.mutation = Some(cx.spawn(async move |entity, cx| {
            let Request { change, range, old, old_bounds, geometry, native, commit, initial_error, generation } = request;
            let lease = native_settle::Lease::Preference(generation);
            let confirmed = if let Some(error) = initial_error { Err(error) }
                else if native {
                    match handle {
                        Some(handle) => native_settle::wait(&entity, handle, lease, cx).await,
                        None => Err(HookError::new("error.monitor.petApplyFailed")),
                    }
                } else { Ok(true) };
            if matches!(confirmed, Ok(false)) { return; }
            let result = match confirmed {
                Err(error) => Err(error),
                Ok(_) if commit => {
                    // 清除待接管标记与后台接受命令处于同一次 UI 更新，退出不能落在两者之间。
                    let work = entity.update(cx, |this, _| {
                        if this.quitting || this.settings_generation != generation { return None; }
                        this.scheduled = None;
                        Some(this.service.change_pet_preferences(change, range))
                    }).ok().flatten();
                    let Some(work) = work else { return; };
                    match work {
                        Ok(work) => crate::receipt::hook_receipt(work, cx.background_executor().clone(),
                            "error.monitor.settingsWriteFailed").await,
                        Err(_) => Err(HookError::new("error.monitor.settingsWriteFailed")),
                    }
                }
                Ok(_) => {
                    let Ok(settings) = entity.update(cx, |this, _| this.settings.clone()) else { return; };
                    Ok(settings)
                }
            };
            let mut restored = true;
            if result.is_err() && native {
                let rollback = entity.update(cx, |this, cx| {
                    if this.quitting || this.settings_generation != generation { return None; }
                    this.scheduled = None;
                    this.settings.pet_window = old;
                    Some(this.rollback_native(geometry, old_bounds, cx))
                }).ok().flatten();
                let Some(started) = rollback else { return; };
                restored = if started {
                    match handle {
                        Some(handle) => match native_settle::wait(&entity, handle, lease, cx).await {
                            Ok(false) => return,
                            Ok(true) => true,
                            Err(_) => false,
                        },
                        None => false,
                    }
                } else { false };
            }
            let _ = entity.update(cx, |this, cx| {
                if this.quitting || this.settings_generation != generation { return; }
                this.scheduled = None;
                this.busy = false;
                this.native_mutating = false;
                let saved = result.is_ok();
                this.complete_first_focus(saved, cx);
                match result {
                    Ok(settings) => {
                        if commit {
                            this.settings = settings;
                            this.settings_ready = true;
                            this.observed.1 = this.service.pet_settings_revision();
                        }
                        if native && this.geometry_restored { this.display_geometry_unconfirmed = false; }
                        this.last_error = None;
                    }
                    Err(error) => {
                        this.settings.pet_window = old;
                        if !restored {
                            this.mouse_state.confirmed = None;
                            if geometry { this.display_geometry_unconfirmed = true; }
                        }
                        tracing::warn!(code = error.code, "pet preference transaction failed");
                        this.last_error = Some(if restored { error } else { HookError::new("error.monitor.petApplyFailed") });
                    }
                }
                this.next_pending(cx);
                if saved {
                    this.recheck_native_resize(cx);
                    if geometry && let Some(handle) = handle {
                        cx.defer(move |cx| { let _ = handle.update(cx, |_, window, cx| window.bounds_changed(cx)); });
                    }
                }
                cx.notify();
            });
        }));
    }
}
