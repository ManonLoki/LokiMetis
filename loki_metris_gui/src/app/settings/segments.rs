//! 主题与语言共用有原生单选语义的分段控件；方向键由稳定焦点句柄处理。

use super::super::Shell;
use crate::preferences::PreferencePatch;
use gpui_kit::{AnyElement,Axis,Context,FocusHandle,FontWeight,IntoElement,KeyDownEvent,
    base::{Radio,RadioGroup},component::ActiveTheme,prelude::*,px,rgb};
use std::cell::OnceCell;

impl Shell {
    /// 仅已确认选项进入 Tab 顺序，方向键移动焦点并提交真实保存请求。
    pub(super) fn preference_segments<const N:usize>(&self,id:&'static str,title:&str,choices:[(PreferencePatch,&'static str);N],selected:usize,focus:&OnceCell<[FocusHandle;N]>,cx:&mut Context<Self>)->AnyElement {
        let focus=focus.get_or_init(||std::array::from_fn(|_|cx.focus_handle())).clone();
        let keyboard_focus=focus.clone();let busy=self.preference_save.pending.is_some();
        let dark=cx.theme().mode.is_dark();
        RadioGroup::new(id).axis(Axis::Horizontal).aria_label(self.t(title)).w_full().flex().rounded(px(12.)).p_1().bg(rgb(if dark{0x1e1c1a}else{0xf0ebe3}))
            .on_key_down(cx.listener(move|this,event:&KeyDownEvent,window,cx| {
                if this.preference_save.pending.is_some() {return}
                let current=keyboard_focus.iter().position(|handle|handle.is_focused(window)).unwrap_or(selected);
                let next=match event.keystroke.key.as_str() {"left"|"up"=>(current+N-1)%N,"right"|"down"=>(current+1)%N,"home"=>0,"end"=>N-1,_=>return};
                window.prevent_default();cx.stop_propagation();window.focus(&keyboard_focus[next],cx);
                this.save_preference(choices[next].0,window,cx);
            }))
            .children(choices.into_iter().enumerate().map(|(index,(patch,key))| {
                let entity=cx.entity().downgrade();
                Radio::new((id,index)).checked(index==selected).disabled(busy).set_position(index+1,N).track_focus(&focus[index]).tab_stop(index==selected).accessibility_label(self.t(key))
                    .flex_1().h(px(28.)).rounded(px(10.)).flex().items_center().justify_center().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD)
                    .when(index==selected,|radio|radio.bg(rgb(if dark{0x1e1c1a}else{0xffffff})).shadow_sm())
                    .text_color(if index==selected {cx.theme().foreground}else{cx.theme().muted_foreground})
                    .child(self.t(key)).on_change(move|_,_,window,cx|{let _=entity.update(cx,|this,cx|this.save_preference(patch,window,cx));})
            })).into_any_element()
    }
}
