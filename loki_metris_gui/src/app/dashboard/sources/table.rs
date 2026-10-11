//! 来源表格保持历史固定列序、独立行操作与有界横向滚动。

use crate::app::surface::{BrandTableRow, StaticCard, StaticMaterial};
use super::{Shell,SourceButton,SourceButtonVariant};
use crate::usage_service::SourceOperation;
use gpui_kit::{AnyElement,Context,FontWeight,SharedString,div,prelude::*,px,
    base::TestSupportExt as _,component::{ActiveTheme,Disableable,table::{Table,TableHeader,TableHead,TableBody,TableRow,TableCell}}};
use loki_metris_core::{SourceRootSummary,SourceDiscoveryCode as Discovery,RootActivationState as Activation};

/// 原版 colgroup 比例保持固定，宽窗口把剩余宽度按同一比例分配。
const WIDTHS:[f32;8]=[240.,136.,84.,84.,84.,84.,160.,400.];

impl Shell {
    /// 表格保留所有可信行；空列表使用真实空态，不代造默认目录。
    pub(super) fn source_table(&self,cx:&mut Context<Self>)->AnyElement {
        let widths=self.source_column_widths();
        let mut header=TableRow::new();
        for(index,key)in ["alias","discovery","files","skipped","errors","duplicates","lastScan","actions"].into_iter().enumerate() {
            let label=self.t(&format!("sources.table.{key}"));
            header=header.child(TableHead::new().w(px(widths[index])).min_w(px(widths[index])).flex_none().px_3().py(px(16.))
                .child(div().w_full().when((2..=5).contains(&index)||index==7,|label|label.text_right())
                    .id(SharedString::from(format!("source-heading-{index}"))).test_support().role(gpui_kit::Role::Label).aria_label(label.clone()).child(label)));
        }
        let roots=self.dashboard.sources.roots.as_deref().unwrap_or_default();
        let body=if roots.is_empty() {TableBody::new().child(BrandTableRow::new("sources-empty-row",cx.theme().mode.is_dark()).bottom_corner_radius(px(19.)).child(TableCell::new().col_span(8).p_4().child(self.t("sources.table.empty"))))}
            else {TableBody::new().children(roots.iter().enumerate().map(|(index,root)|self.source_row(index,root,roots,cx)))};
        StaticCard::new("sources-root-table", cx.theme().mode.is_dark(), StaticMaterial::Table).test_support().overflow_hidden()
            .child(div().id("sources-table-scroll").test_support().role(gpui_kit::Role::Group).aria_label(self.t("sources.page.title"))
                .w_full().min_w_0().overflow_x_scroll().child(div().w(px(widths.iter().sum())).min_w(px(widths.iter().sum())).flex_none()
                    .child(Table::new().w_full().text_size(px(14.)).text_color(cx.theme().foreground)
                        .child(TableHeader::new().font_weight(FontWeight::BOLD).child(header)).child(body))))
            .into_any_element()
    }

    /// 壳体侧栏、页面外边距及来源页内边距决定当前可用宽度。
    fn source_column_widths(&self)->[f32;8] {
        let padding=if self.viewport_width<992. {20.}else{32.};
        let width=(self.viewport_width-80.-2.*padding-18.).max(WIDTHS.iter().sum());
        WIDTHS.map(|column|column*width/WIDTHS.iter().sum::<f32>())
    }

    /// 行动作捕获稳定根 ID；显示别名变化不会把变更写入另一个根。
    fn source_row(&self,index:usize,root:&SourceRootSummary,roots:&[SourceRootSummary],cx:&mut Context<Self>)->BrandTableRow {
        let alias=self.source_alias(root,roots);let disabled=self.source_writes_disabled();
        let widths=self.source_column_widths();
        let cell=|index:usize|TableCell::new().w(px(widths[index])).min_w(px(widths[index])).flex_none().px_3().py_5();
        let enabled=self.t(if root.enabled {"sources.table.enabled"}else{"sources.table.disabled"});
        let mut badges=div().flex().flex_wrap().gap_1().child(div().h(px(20.)).px(px(10.)).rounded_full().flex().items_center().gap(px(6.))
            .bg(cx.theme().foreground.opacity(0.14)).text_size(px(11.)).font_weight(FontWeight::SEMIBOLD)
            .child(div().size(px(6.)).rounded_full().bg(if root.enabled {gpui_kit::rgb(0x40c057).into()}else{cx.theme().muted_foreground}))
            .child(div().id(SharedString::from(format!("source-enabled-{index}"))).role(gpui_kit::Role::Label).aria_label(enabled.clone()).child(enabled)));
        if root.is_primary {badges=badges.child(self.quality_badge(self.t("sources.table.primary"),cx.theme().primary));}
        let activation=if self.dashboard.scanning_client==self.usage_client()&&self.dashboard.scanning&&self.dashboard.progress.as_ref().is_some_and(|progress|progress.current_root_id==root.id) {Some(("indexing",gpui_kit::rgb(0x339af0).into()))} else {match root.activation_state {
            Activation::Ready=>None,Activation::ValidationFailed=>Some(("validationFailed",cx.theme().danger)),
            Activation::Indexing if self.dashboard.progress.as_ref().is_some_and(|progress|progress.current_root_id==root.id)=>Some(("indexing",gpui_kit::rgb(0x339af0).into())),
            _=>Some(("confirmedUnindexed",gpui_kit::rgb(0x339af0).into())),
        }};
        if let Some((key,color))=activation {badges=badges.child(self.quality_badge(self.t(&format!("sources.table.activation.{key}")),color));}
        let discovery=match root.discovery_code {Discovery::DefaultRoot=>"defaultRoot",Discovery::CodexEnvironment=>"codexEnvironment",Discovery::ClaudeEnvironment=>"claudeEnvironment",Discovery::GrokEnvironment=>"grokEnvironment",Discovery::UserRegistered=>"userRegistered",Discovery::FullDevice=>"fullDevice",Discovery::MetadataDiscovery=>"metadataDiscovery"};
        let mut actions=div().w_full().flex().items_center().justify_end().gap_2();
        if self.dashboard.client!=4 {
            let id=root.id.clone();let enabled=!root.enabled;let action=self.t(if enabled {"sources.table.enable"}else{"sources.table.disable"});
            actions=actions.child(SourceButton::new(("source-toggle",index),SourceButtonVariant::SubtleBrand,cx.theme().mode.is_dark()).h(px(26.)).px_2().rounded(px(12.)).text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).disabled(disabled)
                .accessibility_label(format!("{action}{alias}")).label(action)
                .on_click(cx.listener(move|this,_,_,cx|this.change_source(SourceOperation::SetEnabled{root_id:id.clone(),enabled},cx))));
        }
        let id=root.id.clone();
        actions=actions.child(SourceButton::new(("source-reindex",index),SourceButtonVariant::SubtleBrand,cx.theme().mode.is_dark()).h(px(26.)).px_2().rounded(px(12.)).text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).disabled(disabled||!root.enabled)
            .accessibility_label(self.t("sources.table.reindexAria").replace("{{alias}}",&alias)).label(self.t("sources.table.reindex"))
            .on_click(cx.listener(move|this,_,_,cx| {
                if this.dashboard.client==4 {this.load_sources(cx);}else{this.change_source(SourceOperation::Reindex{root_id:id.clone()},cx);}
            })));
        if self.dashboard.client!=4 {
            for(remove,key)in [(false,"rename"),(true,"remove")] {
                let root=root.clone();let label=self.t(&format!("sources.table.{key}"));
                actions=actions.child(SourceButton::new(SharedString::from(format!("source-{key}-{index}")),if remove{SourceButtonVariant::SubtleRed}else{SourceButtonVariant::SubtleBrand},cx.theme().mode.is_dark()).h(px(26.)).px_2().rounded(px(12.)).text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).disabled(disabled)
                    .accessibility_label(self.t(&format!("sources.table.{key}Aria")).replace("{{alias}}",&alias)).label(label)
                    .on_click(cx.listener(move|this,_,window,cx|this.open_source_dialog(root.clone(),remove,window,cx))));
            }
        }
        let text=|column:usize,value:String|div().id(SharedString::from(format!("source-{index}-{column}"))).test_support().role(gpui_kit::Role::Label).aria_label(value.clone()).child(value);
        let mut row=BrandTableRow::new(SharedString::from(format!("source-row-{}",root.id)),cx.theme().mode.is_dark()).bottom_corner_radius(px(if index+1==roots.len(){19.}else{0.})).child(cell(0).child(div().w_full().min_w_0().flex().items_center().flex_wrap().gap_2()
            .child(text(0,alias).max_w_full().truncate().font_weight(FontWeight::BOLD).text_size(px(16.))).child(badges)))
            .child(cell(1).child(text(1,self.t(&format!("sources.discoveryLabels.{discovery}")))));
        for(index,value)in [(2,root.file_count),(3,root.skipped_count),(4,root.error_count),(5,root.duplicate_count)] {
            row=row.child(cell(index).child(text(index,value.to_string()).w_full().text_right()));
        }
        row.child(cell(6).child(text(6,self.t("sources.table.neverScanned")))).child(cell(7).child(actions))
    }
}
