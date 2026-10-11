//! 设备展示偏好保存回执归属 Shell；离开设置页后已接受的写入仍会完成。

use super::super::Shell;
use crate::preferences::PreferencePatch;
use gpui_kit::{Context,Task,Window};

/// 主题和语言串行提交；失败状态归属原字段，不切换已经确认的值。
#[derive(Default)]
pub(in crate::app) struct PreferenceSaveState {
    pub pending:Option<PreferencePatch>,
    pub failed:Option<PreferencePatch>,
    task:Option<Task<()>>,
    pub language_focus:std::cell::OnceCell<[gpui_kit::FocusHandle;2]>,
    pub theme_focus:std::cell::OnceCell<[gpui_kit::FocusHandle;3]>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::AppContext;
    use crate::preferences::{ThemeChoice,LanguageChoice};
    /// 已接受保存跨页继续；回执到达后才切主题，窗口几何不会覆盖它。
    #[gpui_kit::test]
    fn theme_receipt_finishes_after_leaving_settings(cx:&mut gpui_kit::TestAppContext) {
        let (handle,shell,_finish)=crate::app::tests::open_shell(cx,1440.,900.);
        cx.update_window(handle,|_,window,cx|shell.update(cx,|this,cx| {
            this.navigate(crate::app::Page::Settings,cx);
            this.select_theme(ThemeChoice::Dark,window,cx);
            this.navigate(crate::app::Page::Dashboard,cx);
            this.writer.save_window(Some([10.,20.,1200.,800.]),false);
        })).unwrap();
        for _ in 0..500 {
            cx.run_until_parked();
            if shell.read_with(cx,|this,_|this.preference_save.pending.is_none()) {break}
            cx.executor().advance_clock(std::time::Duration::from_millis(20));
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(shell.read_with(cx,|this,_|this.preferences.theme==ThemeChoice::Dark&&this.preference_save.pending.is_none()&&this.page==crate::app::Page::Dashboard));
    }
    /// 提交主题期间保持旧值，第二个语言操作不能跨字段覆盖尚未确认的选择。
    #[gpui_kit::test]
    fn pending_preference_keeps_confirmed_values_and_rejects_a_second_mutation(cx:&mut gpui_kit::TestAppContext) {
        let (handle,shell,_finish)=crate::app::tests::open_shell(cx,1440.,900.);
        cx.update_window(handle,|_,window,cx|shell.update(cx,|this,cx| {
            this.select_theme(ThemeChoice::Dark,window,cx);
            assert_eq!(this.preference_save.pending,Some(PreferencePatch::Theme(ThemeChoice::Dark)));
            assert_eq!(this.preferences.theme,ThemeChoice::Light);
            this.select_language(LanguageChoice::Chinese,window,cx);
            assert_eq!(this.preferences.language,LanguageChoice::English);
            assert_eq!(this.preference_save.pending,Some(PreferencePatch::Theme(ThemeChoice::Dark)));
        })).unwrap();
    }

    /// 应用信息文字列必须获得实际宽度，窄窗也不能退化成逐字排布。
    #[gpui_kit::test]
    fn application_copy_keeps_readable_width_at_supported_window_sizes(cx:&mut gpui_kit::TestAppContext) {
        use gpui_kit::test::TestWindowExt;
        for width in [960.,1440.] {
            let (handle,shell,_finish)=crate::app::tests::open_shell(cx,width,900.);
            cx.update_window(handle,|_,window,cx| {
                shell.update(cx,|this,cx|this.navigate(crate::app::Page::Settings,cx));window.render_frame(cx);
                let bounds=window.find("settings-application-copy").bounds();
                assert!(bounds.size.width>=gpui_kit::px(280.));assert!(bounds.size.height<gpui_kit::px(180.));
                assert!(window.try_find("page-heading").is_none());
            }).unwrap();
        }
    }
}

impl Shell {
    /// 显式字段 patch 与高频窗口几何分开提交，成功只应用回执对应的字段。
    pub(in crate::app) fn save_preference(&mut self,patch:PreferencePatch,window:&Window,cx:&mut Context<Self>) {
        // 通知必须通过应用级授权 owner，禁止绕过权限门禁直接提交展示偏好。
        if matches!(patch,PreferencePatch::SystemNotification(_))||self.preference_save.pending.is_some() {return}
        if match patch {PreferencePatch::Theme(theme)=>self.preferences.theme==theme,PreferencePatch::Language(language)=>self.preferences.language==language,PreferencePatch::SystemNotification(_)=>return} {return}
        let mut work=match self.writer.mutate(patch) {Ok(work)=>work,Err(_)=>{self.preference_save.failed=Some(patch);cx.notify();return}};
        self.preference_save.pending=Some(patch);self.preference_save.failed=None;
        self.preference_save.task=Some(cx.spawn_in(window,async move|entity,cx| {
            let result=loop {
                match work.try_recv() {
                    Ok(result)=>break Some(result),
                    Err(tokio::sync::oneshot::error::TryRecvError::Closed)=>break None,
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty)=>{},
                }
                cx.background_executor().timer(std::time::Duration::from_millis(20)).await;
            };
            let _=entity.update_in(cx,|this,window,cx| {
                this.preference_save.pending=None;
                match result {
                    Some(Ok(PreferencePatch::Theme(theme)))=>{this.preferences.theme=theme;this.apply_theme(window,cx);},
                    Some(Ok(PreferencePatch::Language(language)))=>{
                        this.preferences.language=language;this.locale=this.preferences.locale();
                        crate::native::set_language(this.locale,cx);this.apply_language();cx.refresh_windows();
                    },
                    _=>this.preference_save.failed=Some(patch),
                }cx.notify();
            });
        }));cx.notify();
    }
}
