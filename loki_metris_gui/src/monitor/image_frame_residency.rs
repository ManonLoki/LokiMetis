//! Metal 按真实绘制位置保留当前帧；CPU 共享 owner 不固定逐帧纹理。

use super::{ImageOwner, OwnerKey, Placement, Residencies};
use gpui_kit::{App, Global, RenderImageParams, Window, WindowId};
use std::collections::{HashMap, HashSet};

/// 只保存身份值，不持有 Entity、原始字节或解码像素。
#[derive(Default)]
struct Frames {
    /// 各位置最近绘制的图像参数。
    owners: HashMap<OwnerKey, RenderImageParams>,
    /// 各窗口待执行的帧清理。
    pending: HashMap<WindowId, Pending>,
}
impl Global for Frames {}

/// 同一窗口一个收尾任务；生命周期清理仍需失效旧 Scene。
#[derive(Default)]
struct Pending {
    /// 待从 Scene 缓存移除的图像参数。
    frames: HashSet<RenderImageParams>,
    /// 清理后需要刷新窗口，使旧 Scene 失效。
    refresh: bool,
}

/// 仅成功 paint 可改变 claim；缓存重放没有回调时仍保留上次真实帧。
pub(crate) fn painted(
    source_id: &str,
    owner: &ImageOwner,
    params: RenderImageParams,
    window: &mut Window,
    cx: &mut App,
) {
    let live = owner.key.window == window.window_handle().window_id()
        && matches!(owner.key.placement, Placement::Playback)
        && cx.has_global::<Residencies>()
        && cx
            .global::<Residencies>()
            .images
            .get(source_id)
            .is_some_and(|entry| entry.owners.contains_key(&owner.key));
    if !cx.has_global::<Frames>() {
        cx.set_global(Frames::default());
    }
    if live {
        let old = cx
            .global_mut::<Frames>()
            .owners
            .insert(owner.key, params.clone());
        if let Some(old) = old.filter(|old| *old != params) {
            enqueue(owner.key.window, old, false, cx);
        }
    } else {
        // 迟到绘制不能复活已释放 owner；上传过的孤立帧也必须清理并重建 Scene。
        enqueue(window.window_handle().window_id(), params, true, cx);
    }
}

/// 兼容旧整图清理：最新 CPU 回执不能否决其它位置仍在绘制的物理图像。
pub(super) fn has_image(window: WindowId, image_id: gpui_kit::ImageId, cx: &App) -> bool {
    cx.has_global::<Frames>()
        && cx
            .global::<Frames>()
            .owners
            .iter()
            .any(|(owner, params)| owner.window == window && params.image_id == image_id)
}

/// CPU owner 删除时同步撤销 GPU claim，不能等待同窗 Catalog/Overlay 一并退出。
pub(super) fn release_owners(owners: Vec<OwnerKey>, cx: &mut App) {
    if !cx.has_global::<Frames>() {
        return;
    }
    let removed = {
        let frames = cx.global_mut::<Frames>();
        owners
            .into_iter()
            .filter_map(|owner| {
                frames
                    .owners
                    .remove(&owner)
                    .map(|params| (owner.window, params))
            })
            .collect::<Vec<_>>()
    };
    for (window, params) in removed {
        enqueue(window, params, true, cx);
    }
}

/// 收尾重新检查所有位置的精确帧并集，A→B→A 或另一位置仍用 A 时不误删。
fn enqueue(window_id: WindowId, params: RenderImageParams, refresh: bool, cx: &mut App) {
    let scheduled = cx.global::<Frames>().pending.contains_key(&window_id);
    let pending = cx
        .global_mut::<Frames>()
        .pending
        .entry(window_id)
        .or_default();
    pending.frames.insert(params);
    pending.refresh |= refresh;
    if scheduled {
        return;
    }
    cx.defer(move |cx| {
        let Some(pending) = cx.global_mut::<Frames>().pending.remove(&window_id) else {
            return;
        };
        let Some(handle) = cx
            .windows()
            .into_iter()
            .find(|handle| handle.window_id() == window_id)
        else {
            return;
        };
        _ = handle.update(cx, |_, window, cx| {
            let keep = cx
                .global::<Frames>()
                .owners
                .iter()
                .filter(|(owner, _)| owner.window == window_id)
                .map(|(_, params)| params.clone())
                .collect::<HashSet<_>>();
            for params in pending.frames {
                if !keep.contains(&params) {
                    window.drop_image_frame(params);
                }
            }
            if pending.refresh {
                window.refresh();
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::{image_decode::MonitorImageSource, image_residency::acquire};
    use gpui_kit::TestAppContext;
    use std::{cell::RefCell, rc::Rc, sync::Arc};

    /// 纯登记器交错测试；真实 paint、AtlasTile 和像素另由原生 Metal 回归验证。
    #[gpui_kit::test]
    fn claims_use_successful_paint_identity_and_recheck_same_cycle_union(cx: &mut TestAppContext) {
        let source = MonitorImageSource {
            id: uuid::Uuid::new_v4().to_string(),
            bytes: Arc::from(include_bytes!("../../tests/fixtures/monitor-loop.gif").as_slice()),
            format: loki_metris_core::ImageFormat::Gif,
        };
        let captured = Rc::new(RefCell::new(None));
        let capture = captured.clone();
        let source_for_owner = source.clone();
        let handle = cx.add_window(move |window, cx| {
            let owner = ImageOwner::playback(window, cx);
            acquire(&source_for_owner, &owner, cx);
            *capture.borrow_mut() = Some(owner);
            gpui_kit::Empty
        });
        let owner = captured.borrow_mut().take().unwrap();
        let a = RenderImageParams {
            image_id: gpui_kit::ImageId(100),
            frame_index: 0,
        };
        let b = RenderImageParams {
            image_id: gpui_kit::ImageId(101),
            frame_index: 1,
        };
        handle
            .update(cx, |_, window, app| {
                // 没有最新 Weak 回执不影响真实 paint 的身份，Weak 不是每 owner 权威状态。
                assert!(
                    app.global::<Residencies>().images[&source.id]
                        .decoded
                        .upgrade()
                        .is_none()
                );
                for params in [a.clone(), b.clone(), a.clone()] {
                    painted(&source.id, &owner, params, window, app);
                }
                let frames = app.global::<Frames>();
                assert!(frames.owners.get(&owner.key) == Some(&a));
                assert_eq!(frames.pending.len(), 1, "同窗多个换帧只排一次收尾");
                let pending = &frames.pending[&owner.key.window];
                assert!(pending.frames == HashSet::from([a.clone(), b]));
                assert!(!pending.refresh, "正常换帧不得强制刷新缓存 Scene");
            })
            .unwrap();
        cx.run_until_parked();
        cx.update(|app| {
            let frames = app.global::<Frames>();
            assert!(frames.owners.get(&owner.key) == Some(&a));
            assert!(frames.pending.is_empty());
        });
        handle
            .update(cx, |_, window, app| {
                crate::monitor::image_residency::release(&source.id, &owner, Some(window), app);
                painted(&source.id, &owner, a.clone(), window, app);
                assert!(
                    !app.global::<Frames>().owners.contains_key(&owner.key),
                    "迟到回调不能复活 owner"
                );
                assert!(app.global::<Frames>().pending[&owner.key.window].refresh);
                window.remove_window();
            })
            .unwrap();
        cx.run_until_parked();
        cx.update(|app| {
            let frames = app.global::<Frames>();
            assert!(frames.owners.is_empty());
            assert!(frames.pending.is_empty(), "原窗关闭也必须结束排队清理");
        });
    }
}
