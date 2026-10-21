//! 调用表列宽：用原生字体测量表头与已加载行的自然宽度，并按分页增量缓存。

use super::super::format;
use crate::app::Shell;
use gpui_kit::{Context, FontWeight, component::ActiveTheme, px};
use loki_metris_core::{SourceClientKind, UsageCallsPage};

/// 内容测量之前的最小列宽，最终宽度由当前结果及原版最小表宽决定。
const CALL_COLUMN_WIDTHS:[f32;13]=[72.,160.,68.,106.,100.,100.,112.,100.,100.,76.,112.,100.,100.];

/// 调用表自然列宽的增量缓存：追加分页只测量新行，替换结果、语言或 Agent 范围变化时整体重算。
///
/// 每次渲染都对全部已加载行做文本排版会随分页线性增长；列宽只取各行最大值，
/// 因此对新增行求最大值即可得到与全量测量相同的结果。
#[derive(Default)]
pub(super) struct CallWidthCache {
    /// 语言、Agent 范围与整页替换代数；任一变化都使缓存失效。
    key:Option<(&'static str,usize,u64)>,
    /// 已测量的前缀行数。
    measured:usize,
    /// 已测量前缀的首行与末行 ID，用于识别未经替换代数而被直接换掉的页面。
    edges:Option<(String,String)>,
    /// 表头与已测量行的自然宽度，尚未按最小表宽缩放。
    natural:[f32;13],
}

impl Shell {
    /// 使用实际原生字体测量自然列宽，长项目名扩展表格而不丢失内容；只为尚未测量的行排版文本。
    pub(in crate::app) fn call_widths(&self,page:&UsageCallsPage,window:&gpui_kit::Window,cx:&Context<Self>)->[f32;13] {
        let key=(self.locale,self.dashboard.client,self.dashboard.calls.replacement);
        let mut cache=self.dashboard.calls.widths.borrow_mut();
        let edges=|count:usize| (count>0).then(|| (page.items[0].id.clone(),page.items[count-1].id.clone()));
        let reusable=cache.key==Some(key)&&cache.measured<=page.items.len()&&cache.edges==edges(cache.measured);
        if !reusable {*cache=CallWidthCache {key:Some(key),measured:0,edges:None,natural:CALL_COLUMN_WIDTHS};}
        let cache=&mut *cache;
        let widths=&mut cache.natural;
        let measure=|value:&str,size:f32,mono:bool,bold:bool| {
            let mut font=gpui_kit::font(if mono {"Geist Mono"}else{"Geist"});
            font.weight=if bold {FontWeight::BOLD}else{FontWeight::NORMAL};
            value.lines().map(|line| {
                let run=gpui_kit::TextRun {len:line.len(),font:font.clone(),color:cx.theme().foreground,..Default::default()};
                f32::from(window.text_system().shape_line(line.to_owned().into(),px(size),&[run],None).width())
            }).fold(0_f32,f32::max)
        };
        // 表头只依赖语言，缓存重建时测量一次。
        if !reusable {for (column,key) in [(1,"calls.time"),(2,"statistics.project"),(3,"statistics.thread"),(4,"statistics.model"),
            (5,"statistics.reasoning"),(6,"calls.input"),(7,"statistics.total"),(8,"statistics.cached"),(9,"statistics.cache_write"),
            (10,"statistics.uncached"),(11,"statistics.output"),(12,"statistics.reasoning_output")] {
            widths[column]=widths[column].max(measure(&self.t(key),14.,false,true)+if matches!(column,2|3|9) {20.}else{44.});
        }}
        for item in &page.items[cache.measured..] {
            let labels=[item.client.display_name().to_owned(),
                format::observed(item.occurred_at_epoch_ms,self.locale),self.display_label(&item.project_label,item.project_label_code),
                self.display_label(&item.thread_label,item.thread_label_code),self.display_label(&item.model_label,item.model_label_code),
                if item.client==SourceClientKind::Grok {self.t("dashboard.not_provided")}else{self.display_label(&item.reasoning_effort_label,item.reasoning_effort_label_code)}];
            for (column,label) in labels.iter().enumerate() {widths[column]=widths[column].max(measure(label,14.,false,false)+20.);}
            let tokens=&item.fact.value;
            for (offset,value) in [Some(tokens.input_tokens),Some(tokens.total_tokens),tokens.cached_input_tokens,tokens.cache_write_input_tokens,
                item.uncached_input_tokens,Some(tokens.output_tokens),tokens.reasoning_output_tokens].into_iter().enumerate() {
                let width=match value {Some(value)=>measure(&format::compact(value),16.,true,true).max(measure(&format::exact(value),12.,true,false)),
                    None=>measure(&self.t("dashboard.not_provided"),14.,false,false)};
                widths[offset+6]=widths[offset+6].max(width+20.);
            }
        }
        cache.measured=page.items.len();cache.edges=edges(cache.measured);
        let mut widths=cache.natural;
        let all=self.dashboard.client==0;let start=if all {0}else{1};
        let natural: f32=widths[start..].iter().sum();let minimum=if all {1560.}else{1440.};
        if natural<minimum {for width in &mut widths[start..] {*width*=minimum/natural;}}
        widths
    }
}
