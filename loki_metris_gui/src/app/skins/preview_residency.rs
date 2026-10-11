//! 预览缓存以目录内容为需求边界；已删除的内容同步退役Asset，随后清理原窗口图集。

use super::{Shell, preview::PreviewImageDecoder};
use crate::usage_service::SkinEntry;
use gpui_kit::{App, Context, RenderImage};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::{Arc, Weak},
};

/// 仅记录实际预览过的解码结果弱引用，不额外保留源字节或像素。
#[derive(Clone, Default)]
pub(crate) struct PreviewResidencies(Rc<RefCell<HashMap<u64, Weak<RenderImage>>>>);

impl PreviewResidencies {
    /// 首次可绘制前记录输出身份；同内容的不同卡片共享同一Asset和图集。
    pub(super) fn observed(&self, source_id: u64, image: &Arc<RenderImage>) {
        self.0.borrow_mut().insert(source_id, Arc::downgrade(image));
    }

    /// 仅清理成功新目录不再引用的内容；pending和failed也按旧源键移除。
    fn retire(&self, old: &[SkinEntry], new: &[SkinEntry], cx: &mut App) -> Vec<Arc<RenderImage>> {
        let retained: HashSet<_> = new
            .iter()
            .filter_map(|entry| entry.preview.as_ref().map(|image| image.id()))
            .collect();
        let mut seen = HashSet::new();
        let removed: Vec<_> = old
            .iter()
            .filter_map(|entry| entry.preview.as_ref())
            .filter(|image| !retained.contains(&image.id()) && seen.insert(image.id()))
            .cloned()
            .collect();
        let retired = {
            let mut observed = self.0.borrow_mut();
            // 必须在移除Asset前升级弱引用，让旧像素存活到原窗口图集清理完成。
            let retired = removed
                .iter()
                .filter_map(|source| {
                    observed
                        .remove(&source.id())
                        .and_then(|image| image.upgrade())
                })
                .collect();
            observed.retain(|id, _| retained.contains(id));
            retired
        };
        for source in removed {
            cx.remove_asset::<PreviewImageDecoder>(&source);
        }
        retired
    }
}

impl Shell {
    /// 当前generation的成功目录回执才替换需求；失败和过期回执不调用此入口。
    pub(super) fn adopt_skin_catalog(&mut self, entries: Vec<SkinEntry>, cx: &mut Context<Self>) {
        let retired = self.skins.preview_residencies.retire(
            self.skins.entries.as_deref().unwrap_or_default(),
            &entries,
            cx,
        );
        self.skins.entries = Some(entries);
        if retired.is_empty() {
            return;
        }
        let handle = self.native_window;
        // Asset已同步移除；这里只清旧RenderImage身份，A→B→A不能误删新A。
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, _| {
                for image in retired {
                    let _ = window.drop_image(image);
                }
                // 停用旧Scene，避免后续重放已经退役的图集tile。
                window.refresh();
            });
        });
    }
}

#[cfg(test)]
#[path = "preview_residency_tests.rs"]
mod tests;
