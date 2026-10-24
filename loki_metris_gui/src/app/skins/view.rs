//! 皮肤资源卡保持参考的预览高度、响应式列数与受控菜单。

use super::{ModalKind, Shell, SkinSuccess, skin_reference};

#[path = "badge.rs"]
mod badge;
use badge::{SkinBadge, skin_badge};
#[path = "toolbar.rs"]
mod toolbar;
#[path = "view_toolbar.rs"]
mod page_toolbar;
#[path = "view_notices.rs"]
mod notices;
#[path = "view_card.rs"]
mod card;
use crate::app::surface::{HistoricalButtonSize, SurfaceButton, SurfaceButtonVariant};
use gpui_kit::{
    AnyElement, BoxShadow, Context, FontWeight, IntoElement, Role, Window,
    assets::IconName,
    base::{Checkbox, CheckboxState, TestSupportExt as _},
    component::{
        ActiveTheme, Disableable, Icon,
        button::{Button, ButtonVariants},
        input::Input,
        menu::{DropdownMenu, PopupMenuItem},
    },
    div,
    prelude::*,
    px, rems, rgb,
};
use loki_metris_core::{ColorMode, SkinHostKind, SkinPackageType, SkinSource};

pub(super) use super::preview::skin_preview;

impl Shell {
    /// 宿主选择共用同一查询交接；同值不失效查询，保持历史受控Tabs的查询键语义。
    fn select_skin_host(&mut self, host: SkinHostKind, cx: &mut Context<Self>) {
        if self.skins.busy || self.skins.modal.is_some() || self.skins.host == Some(host) { return; }
        self.skins.generation = self.skins.generation.wrapping_add(1);
        self.skins.query.take();
        self.skins.loading = false;
        self.skins.authority.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        self.skins.host = Some(host);
        self.skins.status = None;
        self.skins.instances.clear();
        self.skins.host_error = None;
        self.skins.last_read = None;
        self.refresh_skins(true, cx);
        cx.notify();
    }
    /// 初次读取失败不展示伪造空库，刷新失败保留已确认卡片并提供重试。
    pub(in crate::app) fn skins_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let hosts = self.skin_hosts();
        let mut page = div()
            .id("skins-page")
            .test_support()
            .w_full()
            .flex()
            .flex_col()
            // 外层页面负责滚动，短窗仍须保留网格的完整自然高度。
            .flex_shrink_0()
            .gap(px(20.))
            .on_drop(cx.listener(|this, paths: &gpui_kit::ExternalPaths, _, cx| {
                this.prepare_skin_import(paths.paths().to_vec(), cx)
            }));
        if hosts.is_empty() {
            return page
                .child(
                    div()
                        .font_weight(FontWeight::BOLD)
                        .child(self.t("skins.host.empty_title")),
                )
                .child(self.t("skins.host.empty_description"))
                .into_any_element();
        }
        let writable = self.skin_writable();
        page = page.child(self.skins_toolbar(hosts, writable, window, cx));
        for notice in self.skins_notices(writable, cx) {
            page = page.child(notice);
        }
        let Some(entries) = &self.skins.entries else {
            return page.child(self.t("skins.loading")).into_any_element();
        };
        let query = self
            .skins
            .search
            .as_ref()
            .map(|input| input.read(cx).value().trim().to_lowercase())
            .unwrap_or_default();
        let filtered: Vec<_> = entries
            .iter()
            .filter(|entry| {
                query.is_empty()
                    || [&entry.skin.name, &entry.skin.author, &entry.skin.id]
                        .into_iter()
                        .any(|value| value.to_lowercase().contains(&query))
            })
            .collect();
        if filtered.is_empty() {
            return page.child(self.t("skins.catalog.empty")).into_any_element();
        }
        let columns = if self.viewport_width >= 1408. {
            4
        } else if self.viewport_width >= 1200. {
            3
        } else if self.viewport_width >= 768. {
            2
        } else {
            1
        };
        let active = self.active_skin();
        let cards = div()
            .w_full()
            .flex_shrink_0()
            .grid()
            .grid_cols(columns)
            .gap(px(20.))
            .children(filtered.into_iter().map(|entry| self.skin_card(entry, &active, writable, cx)));
        page.child(cards).into_any_element()
    }
    /// 稳定码有专用翻译时优先，未知错误保留服务消息及可重试原因。
    fn skin_error(&self, error: &loki_metris_service::skins::SkinError) -> String {
        match error.code {
            "skin.choose_instance" | "skin.host_instance_selection_required" => {
                self.t("skins.error.choose_instance")
            }
            "skin.injection_failed" => self.t("skins.error.injection_failed"),
            _ => {
                if self.locale == "zh-CN" {
                    error.message.clone()
                } else {
                    format!("{} ({})", self.t("skins.error.unknown"), error.code)
                }
            }
        }
    }
}
