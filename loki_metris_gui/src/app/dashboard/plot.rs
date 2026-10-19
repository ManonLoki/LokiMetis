//! 原生画布按参考 SVG 的 920×300 坐标系绘图，缺失值断线而不补零。

use gpui_kit::{prelude::*, AnyElement, App, Window, Hsla, FontWeight, SharedString,
    Bounds, Pixels, Point, TextRun, TextAlign, FontFeatures, PathBuilder, MouseMoveEvent,
    MouseExitEvent, HitboxBehavior, CursorStyle, canvas, div, fill, point, px, size, rgb};
use std::{cell::Cell, rc::Rc, sync::Arc};
use gpui_kit::base::TestSupportExt as _;

/// 整数保留到 tooltip 格式化时；浮点只用于积分与绘制坐标。
#[derive(Clone, Copy)]
pub(super) enum Value {
    /// 整数计数，如调用数或 Token。
    Count(u64),
    /// 小数值，如积分。
    Decimal(f64),
}
impl Value {
    /// 数值转换仅作用于几何计算。
    pub(super) fn number(self) -> f64 { match self { Self::Count(v)=>v as f64, Self::Decimal(v)=>v } }
    /// 原版悬浮提示使用紧凑 Token 或两位积分。
    pub(super) fn display(self) -> String { match self { Self::Count(v)=>super::format::exact(v),Self::Decimal(v)=>super::format::credits(v) } }
}

/// 轴的显示格式来自图形指标，不影响同图中的 tooltip 精确值。
#[derive(Clone, Copy)]
pub(super) enum Scale {
    /// Token 刻度，按紧凑格式显示。
    Tokens,
    /// 计数刻度。
    Count,
    /// 积分刻度。
    Credits,
}

/// 单条曲线投影，不保存另一份领域状态。
pub(super) struct Series {
    /// 曲线名称。
    pub label: String,
    /// 曲线颜色。
    pub color: Hsla,
    /// 按横轴顺序的取值；None 表示该点没有数据。
    pub values: Vec<Option<Value>>,
}

/// 图表专用颜色沿用参考主题，避免借用面板边框造成低对比网格。
pub(super) struct Palette {
    /// 主文字色。
    pub foreground:Hsla,
    /// 次要文字色。
    pub muted:Hsla,
    /// 网格线色。
    pub grid:Hsla,
    /// 悬停指示线色。
    pub hover:Hsla,
    /// 数据点填充色。
    pub point:Hsla,
    /// 提示框背景色。
    pub background:Hsla,
    /// 提示框边框色。
    pub tooltip_border:Hsla,
    /// 条形图底轨色。
    pub track:Hsla,
    /// 条形图主色。
    pub bar:Hsla,
    /// “其余”条目的颜色。
    pub remainder:Hsla,
}
impl Palette {
    /// 透明度与颜色都来自历史 AppThemeProvider 的图表变量。
    pub fn new(dark:bool)->Self {
        if dark {Self {foreground:rgb(0xf1ede8).into(),muted:rgb(0xc8c2ba).into(),grid:rgb(0xffffff).alpha(0.12).into(),hover:rgb(0xffffff).alpha(0.52).into(),point:rgb(0x1f2023).into(),background:rgb(0x1c1d20).alpha(0.97).into(),tooltip_border:rgb(0xffffff).alpha(0.18).into(),track:rgb(0xffffff).alpha(0.08).into(),bar:rgb(0xfb923c).into(),remainder:rgb(0xa8a29e).into()}}
        else {Self {foreground:rgb(0x332b24).into(),muted:rgb(0x655d54).into(),grid:rgb(0x67513c).alpha(0.16).into(),hover:rgb(0x4b3e31).alpha(0.5).into(),point:rgb(0xffffff).into(),background:rgb(0xffffff).alpha(0.97).into(),tooltip_border:rgb(0x67513c).alpha(0.24).into(),track:rgb(0x67513c).alpha(0.09).into(),bar:rgb(0xd97706).into(),remainder:rgb(0x78716c).into()}}
    }
}

/// 分布行使用后端顺序与总 Token 占比，指标变化不会重排。
pub(super) struct Bar {
    /// 分组名称。
    pub label:String,
    /// 分组取值；None 表示未提供。
    pub value:Option<Value>,
    /// 取值的显示文本。
    pub display:String,
    /// 占总 Token 的百分比文本。
    pub share:String,
    /// 是“其余”汇总行。
    pub remainder:bool,
}

/// 原生条形图沿用参考的 220/150 边距、52 行高和五条竖网格。
pub(super) fn bars(id:&'static str,rows:Vec<Bar>,palette:Palette,scale_kind:Scale)->AnyElement {
    let height=64.+52.*rows.len() as f32;
    let Palette {muted,grid,track,bar,remainder,..}=palette;
    let overlays=div().absolute().inset_0().children(rows.iter().enumerate().map(|(index,row)| {
        let title=format!("{}: {}; {}",row.label,row.display,row.share);
        let label=title.clone();
        div().id((id,index)).tab_index(0).absolute().left_0().right_0()
            .top(gpui_kit::relative((20.+52.*index as f32)/height)).h(gpui_kit::relative(52./height))
            .role(gpui_kit::Role::Group).aria_label(label)
            .focus_visible(|style|style.border_1().border_color(bar))
            .tooltip(move|window,cx|gpui_kit::component::tooltip::Tooltip::new(title.clone()).build(window,cx))
    }));
    let description=rows.iter().map(|row|format!("{}: {}; {}",row.label,row.display,row.share)).collect::<Vec<_>>().join(" · ");
    div().id(id).relative().w_full().min_w(px(832.)).role(gpui_kit::Role::Image).aria_label(description)
        .child(canvas(|_,_,_|(),move|bounds,_,window,cx|{
            let scale=f32::from(bounds.size.width)/920.;
            let position=|x:f32,y:f32|bounds.origin+point(px(x*scale),px(y*scale));
            let maximum=rows.iter().filter_map(|row|row.value).map(Value::number).fold(1_f64,f64::max);
            for step in 0..=4 {
                let x=220.+550.*step as f32/4.;
                stroke(window,[position(x,20.),position(x,height-44.)],px(scale),grid,false);
                text(window,cx,&axis(maximum*step as f64/4.,scale_kind),position(x,height-15.),TextStyle {size:11.*scale,weight:FontWeight::NORMAL,color:muted},
                    if step==0 {0.}else if step==4 {1.}else{0.5});
            }
            for (index,row) in rows.iter().enumerate() {
                let top=20.+52.*index as f32;
                let label=if row.label.chars().count()>24 {format!("{}…",row.label.chars().take(23).collect::<String>())}else{row.label.clone()};
                text(window,cx,&label,position(208.,top+20.),TextStyle {size:12.*scale,weight:if row.remainder{FontWeight::BOLD}else{FontWeight::SEMIBOLD},color:muted},1.);
                text(window,cx,&row.share,position(208.,top+38.),TextStyle {size:11.*scale,weight:FontWeight::NORMAL,color:muted},1.);
                window.paint_quad(fill(Bounds::new(position(220.,top+8.),size(px(550.*scale),px(22.*scale))),track).corner_radii(px(11.*scale)));
                if let Some(value)=row.value {
                    window.paint_quad(fill(Bounds::new(position(220.,top+8.),size(px((550.*value.number()/maximum) as f32*scale),px(22.*scale))),if row.remainder{remainder}else{bar}).corner_radii(px(11.*scale)));
                }
                text(window,cx,&row.display,position(904.,top+23.),TextStyle {size:12.*scale,weight:FontWeight::BOLD,color:muted},1.);
            }
        }).w_full().aspect_ratio(920./height)).child(overlays).into_any_element()
}

/// 绘制与交互共用同一桶序列，hover 仅属于当前原生视图。
#[expect(clippy::too_many_arguments, reason = "图表组件的标识、数据、悬停状态与配色彼此独立，由唯一调用方一次性传入")]
pub(super) fn line(id: &'static str, label:String, labels: Vec<String>, series: Vec<Series>, hover: Rc<Cell<Option<usize>>>,
    palette:Palette, absent: String, axis_scale:Scale) -> AnyElement
{
    let labels=Arc::new(labels); let series=Arc::new(series);
    let Palette {foreground,muted,grid,hover:hover_color,point:point_color,background,tooltip_border,..}=palette;
    let legend=div().flex().flex_wrap().gap_4().px_2().pt_1().children(series.iter().map(|series|
        div().flex().items_center().gap_2().text_xs().child(div().size(px(12.8)).rounded_full().bg(series.color)).child(series.label.clone())));
    div().id(id).test_support().w_full().min_w(px(608.)).role(gpui_kit::Role::Image).aria_label(label)
        .child(canvas(move |bounds,window,_| {
            let scale=f32::from(bounds.size.width)/920.;
            window.insert_hitbox(Bounds::new(bounds.origin+point(px(72.*scale),px(18.*scale)),size(px(826.*scale),px(234.*scale))),HitboxBehavior::Normal)
        },move |bounds,hitbox,window,cx| {
            let scale=f32::from(bounds.size.width)/920.;
            let position=|x:f32,y:f32| bounds.origin+point(px(x*scale),px(y*scale));
            let view=window.current_view();let current=hover.clone();let count=labels.len();
            window.set_cursor_style(CursorStyle::Crosshair,&hitbox);
            window.on_mouse_event(move |event:&MouseMoveEvent,phase,window,cx| {
                if !phase.bubble() { return }
                let next=if hitbox.is_hovered(window) && count>0 {
                    let ratio=(f32::from(event.position.x-bounds.origin.x)/scale-72.)/826.;
                    Some(nearest(ratio,count))
                } else {None};
                if current.replace(next)!=next {cx.notify(view);}
            });
            let current=hover.clone();
            window.on_mouse_event(move |_:&MouseExitEvent,phase,_,cx| {
                if phase.bubble() && current.replace(None).is_some() {cx.notify(view);}
            });
            let maximum=series.iter().flat_map(|s|s.values.iter().flatten()).map(|v|v.number()).fold(1_f64,f64::max);
            let x=|index:usize| if labels.len()<=1 {485.}else{72.+826.*index as f32/(labels.len()-1) as f32};
            let y=|value:Value|252.-234.*(value.number()/maximum) as f32;
            for step in 0..=4 {
                let grid_y=252.-234.*step as f32/4.;
                stroke(window,[position(72.,grid_y),position(898.,grid_y)],px(scale),grid,false);
                text(window,cx,&axis(maximum*step as f64/4.,axis_scale),position(62.,grid_y+4.),TextStyle {size:12.*scale,weight:FontWeight::NORMAL,color:muted},1.);
            }
            for index in ticks(labels.len()) {
                let anchor=if index==0 {0.}else if index+1==labels.len(){1.}else{0.5};
                text(window,cx,&labels[index],position(x(index),282.),TextStyle {size:12.*scale,weight:FontWeight::NORMAL,color:muted},anchor);
            }
            for series in series.iter() {
                let mut previous=None;
                for (index,value) in series.values.iter().enumerate() {
                    if let Some(value)=value {
                        let point=position(x(index),y(*value));
                        if let Some(previous)=previous {stroke(window,[previous,point],px(2.5*scale),series.color,false);}
                        dot(window,point,px(3.2*scale),series.color);
                        previous=Some(point);
                    } else {previous=None;}
                }
            }
            if let Some(index)=hover.get().filter(|index|*index<labels.len()) {
                let anchor=x(index);
                stroke(window,[position(anchor,18.),position(anchor,252.)],px(scale),hover_color,true);
                for series in series.iter() {
                    if let Some(Some(value))=series.values.get(index) {
                        dot(window,position(anchor,y(*value)),px(6.*scale),point_color);
                        dot(window,position(anchor,y(*value)),px(4.*scale),series.color);
                    }
                }
                let left=if anchor+252.<=898. {anchor+12.}else{(anchor-252.).max(72.)};
                let height=32.+20.*series.len() as f32;
                window.paint_quad(fill(Bounds::new(position(left-0.5,25.5),size(px(241.*scale),px((height+1.)*scale))),tooltip_border).corner_radii(px(8.5*scale)));
                window.paint_quad(fill(Bounds::new(position(left,26.),size(px(240.*scale),px(height*scale))),background).corner_radii(px(8.*scale)));
                text(window,cx,&labels[index],position(left+12.,47.),TextStyle {size:13.*scale,weight:FontWeight::BOLD,color:foreground},0.);
                for (row,series) in series.iter().enumerate() {
                    let baseline=66.+20.*row as f32;
                    dot(window,position(left+15.,baseline-4.),px(3.5*scale),series.color);
                    text(window,cx,&series.label,position(left+25.,baseline),TextStyle {size:12.*scale,weight:FontWeight::NORMAL,color:foreground},0.);
                    let value=series.values.get(index).copied().flatten().map(Value::display).unwrap_or_else(||absent.clone());
                    text(window,cx,&value,position(left+228.,baseline),TextStyle {size:12.*scale,weight:FontWeight::BOLD,color:foreground},1.);
                }
            }
        }).w_full().aspect_ratio(920./300.)).child(legend).into_any_element()
}

/// 小桶数全部显示，长序列最多七个均匀取样标签，首尾始终可见。
fn ticks(count:usize)->Vec<usize> {
    if count<=7 {(0..count).collect()}else{(0..7).map(|index|((count-1) as f32*index as f32/6.).round() as usize).collect()}
}

/// 桶定位仅用于交互，单桶没有除零或越界。
fn nearest(ratio:f32,count:usize)->usize {if count<=1 {0}else{(ratio.clamp(0.,1.)*(count-1) as f32).round() as usize}}

/// 轴标签保持参考的紧凑格式，坐标比例不修改业务读数。
fn axis(value:f64,scale:Scale)->String { let value=value.round().max(0.);match scale {Scale::Tokens=>super::format::compact(value as u64),Scale::Count=>super::format::exact(value as u64),Scale::Credits=>super::format::credits(value)} }

/// 缺失点由调用方截断，线段不跨越未知区间。
fn stroke(window:&mut Window,points:[Point<Pixels>;2],width:Pixels,color:Hsla,dashed:bool) {
    let mut path=PathBuilder::stroke(width);
    if dashed {path=path.dash_array(&[width*4.,width*4.]);}
    path.move_to(points[0]);path.line_to(points[1]);
    if let Ok(path)=path.build() {window.paint_path(path,color);}
}

/// 圆点同时补齐线段端帽，保持各分辨率下的圆形几何。
fn dot(window:&mut Window,center:Point<Pixels>,radius:Pixels,color:Hsla) {
    window.paint_quad(fill(Bounds::new(center-point(radius,radius),size(radius*2.,radius*2.)),color).corner_radii(radius));
}

/// 绘图标签的字号、字重与颜色。
#[derive(Clone,Copy)]
struct TextStyle {
    /// 已乘以显示比例的字号。
    size:f32,
    /// 字重。
    weight:FontWeight,
    /// 文字颜色。
    color:Hsla,
}

/// TextRun 明确字重和等宽数字；避免绘图标签丢失字体权重。
fn text(window:&mut Window,cx:&mut App,value:&str,anchor:Point<Pixels>,style:TextStyle,align:f32) {
    let TextStyle {size:font_size,weight,color}=style;
    let mut font=gpui_kit::font("Geist");font.weight=weight;
    font.features=FontFeatures(Arc::new(vec![("tnum".into(),1)]));
    let value=SharedString::from(value.to_owned());
    let run=TextRun {len:value.len(),font,color,..Default::default()};
    let line=window.text_system().shape_line(value,px(font_size),&[run],None);
    let origin=point(anchor.x-line.width*align,anchor.y-line.ascent);
    let _=line.paint(origin,line.ascent+line.descent,TextAlign::Left,None,window,cx);
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 左右边缘、单桶和长序列定位不能越界；坐标在桶中心切换。
    #[test]
    fn hover_and_ticks_preserve_edges() {
        assert_eq!(nearest(-1.,24),0);assert_eq!(nearest(2.,24),23);assert_eq!(nearest(0.5,1),0);
        assert_eq!(ticks(24),vec![0,4,8,12,15,19,23]);assert_eq!(ticks(0),Vec::<usize>::new());
    }
}
