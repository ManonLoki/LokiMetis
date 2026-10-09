//! 偏好解析和后台退出刷新回归使用独立临时文件。

use super::{LanguageChoice, Preferences, ThemeChoice, PreferencePatch, load_from};

/// 三态主题、语言与设备侧栏选择可以原样往返。
#[test]
fn preferences_round_trip() {
    let value = Preferences { theme: ThemeChoice::Dark, language: LanguageChoice::Chinese, collapsed: true, bounds: Some([30., 40., 1200., 800.]), maximized: true, system_notification:true };
    assert_eq!(Preferences::decode(&value.encode()), value);
}

/// 损坏及未知字段不产生任意状态或文件路径。
#[test]
fn malformed_preferences_fall_back_to_defaults() {
    assert_eq!(Preferences::decode("theme=invalid\nlanguage=unknown\nbounds=bad\nunknown=x"), Preferences::default());
}

/// 关闭前最后一次保存必须由后台线程写入，即使队列已合并多次快速点击。
#[test]
fn writer_flushes_latest_snapshot_after_event_loop() {
    let root = std::fs::canonicalize(std::env::temp_dir()).expect("读取真实临时目录");
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("读取测试时间").as_nanos();
    let directory = root.join(format!("gpui-preference-test-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&directory).expect("创建测试临时目录");
    let file = directory.join("preferences.txt");
    let (_, writer) = load_from(Some(file.clone()));
    let receipt=writer.mutate(PreferencePatch::Theme(ThemeChoice::Dark)).unwrap();
    for index in 0..32 {writer.save_window(Some([30.,40.,1200.+index as f32,800.]),false);}
    drop(receipt);drop(writer.mutate(PreferencePatch::Language(LanguageChoice::English)).unwrap());
    let expected = Preferences {theme:ThemeChoice::Dark,language:LanguageChoice::English,bounds:Some([30.,40.,1231.,800.]),..Preferences::default()};
    writer.finish();
    assert_eq!(Preferences::decode(&std::fs::read_to_string(&file).expect("后台已写入")), expected);
    std::fs::remove_dir_all(directory).expect("清理测试临时目录");
}

/// 明确选择语言时不依赖当前宿主系统的 locale。
#[test]
fn explicit_language_overrides_system_detection() {
    assert_eq!(Preferences { language: LanguageChoice::Chinese, ..Preferences::default() }.locale(), "zh-CN");
    assert_eq!(Preferences { language: LanguageChoice::English, ..Preferences::default() }.locale(), "en-US");
}

/// 真实写入失败通过回执报告，后续恢复目录后保存不能继承失败主题。
#[test]
fn failed_patch_does_not_change_the_committed_baseline() {
    let root=tempfile::tempdir().unwrap();let file=root.path().join("preferences.txt");
    std::fs::create_dir(&file).unwrap();let (_,writer)=load_from(Some(file.clone()));
    let failed=writer.mutate(PreferencePatch::Theme(ThemeChoice::Dark)).unwrap();
    assert!(failed.blocking_recv().unwrap().is_err());std::fs::remove_dir(&file).unwrap();
    writer.mutate(PreferencePatch::Language(LanguageChoice::Chinese)).unwrap().blocking_recv().unwrap().unwrap();
    writer.finish();let saved=Preferences::decode(&std::fs::read_to_string(file).unwrap());
    assert_eq!(saved.theme,ThemeChoice::System);assert_eq!(saved.language,LanguageChoice::Chinese);
}

/// 生产缺少有效持久路径时必须失败，不把仅存在进程内的值报告成已保存。
#[test]
fn unavailable_storage_returns_a_failed_receipt() {
    let (_,writer)=load_from(None);
    assert!(writer.mutate(PreferencePatch::Theme(ThemeChoice::Dark)).unwrap().blocking_recv().unwrap().is_err());writer.finish();
}

/// 通知字段与几何、主题和语言独立合并，停止后的迟到提交同步拒绝。
#[test]
fn notification_patch_preserves_geometry_and_shutdown_closes_admission() {
    let root=tempfile::tempdir().unwrap();let file=root.path().join("preferences.txt");
    let (_,writer)=load_from(Some(file.clone()));
    writer.save_window(Some([30.,40.,1200.,800.]),true);
    writer.mutate(PreferencePatch::SystemNotification(true)).unwrap().blocking_recv().unwrap().unwrap();
    writer.mutate(PreferencePatch::Language(LanguageChoice::English)).unwrap().blocking_recv().unwrap().unwrap();
    writer.finish();
    assert!(writer.mutate(PreferencePatch::SystemNotification(false)).is_err());
    writer.save_window(Some([0.,0.,960.,640.]),false);
    let saved=Preferences::decode(&std::fs::read_to_string(file).unwrap());
    assert!(saved.system_notification);assert_eq!(saved.language,LanguageChoice::English);
    assert_eq!(saved.bounds,Some([30.,40.,1200.,800.]));assert!(saved.maximized);
}

/// 并发关闭与准入共享门禁；任何已接受命令都位于Stop前并获得真实完成回执。
#[test]
fn concurrent_finish_never_accepts_an_unprocessed_patch() {
    for _ in 0..16 {
        let writer=super::Writer::in_memory(Preferences::default());
        let barrier=std::sync::Arc::new(std::sync::Barrier::new(3));
        let submitter=writer.clone();let submit_barrier=barrier.clone();
        let submit=std::thread::spawn(move|| {
            submit_barrier.wait();
            submitter.mutate(PreferencePatch::SystemNotification(true))
                .map(|receipt|receipt.blocking_recv().expect("accepted patch must not be stranded after Stop").unwrap())
        });
        let finisher=writer.clone();let finish_barrier=barrier.clone();
        let finish=std::thread::spawn(move||{finish_barrier.wait();finisher.finish();});
        barrier.wait();finish.join().unwrap();
        if let Ok(patch)=submit.join().unwrap() {assert_eq!(patch,PreferencePatch::SystemNotification(true));}
        assert!(writer.mutate(PreferencePatch::SystemNotification(false)).is_err());
    }
}
