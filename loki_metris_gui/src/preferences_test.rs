//! 偏好解析和后台退出刷新回归使用独立临时文件。

use super::{LanguageChoice, Preferences, ThemeChoice, PreferencePatch, load_from};

/// 三态主题、语言与设备侧栏选择可以原样往返。
#[test]
fn preferences_round_trip() {
    let value = Preferences { theme: ThemeChoice::Dark, language: LanguageChoice::Chinese, collapsed: true, bounds: Some([30., 40., 1200., 800.]), maximized: true, system_notification:true, window_state_v2:None };
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

/// 测试屏内逻辑状态与真实 HOME、OS 显示器及设备 UUID 完全隔离。
fn window_state() -> loki_metris_core::window_placement::WindowStateV2 {
    window_state_with_width(1200.)
}

/// 用实际捕获规则生成每个尺寸的相对锚点，避免构造内部不一致的持久快照。
fn window_state_with_width(width: f64) -> loki_metris_core::window_placement::WindowStateV2 {
    use loki_metris_core::window_placement::*;
    let area = WindowDisplayArea {
        key: "test-screen".into(), scale: 1.5, primary: true,
        work_area: WindowDipRect {
            origin: WindowDipPoint { x: 0., y: 24. },
            size: WindowDipSize { width: 2000., height: 1200. },
        },
    };
    capture_window_state(&area, WindowDipPoint { x: 70., y: 80. },
        WindowDipSize { width, height: 800. }, WindowFrameInsets::default(), false).unwrap()
}

/// 新字段往返且保留旧行格式，损坏新字段只回退自身，不清空主题或旧几何。
#[test]
fn versioned_window_state_roundtrip_and_corruption_are_field_local() {
    let value = Preferences {
        theme: ThemeChoice::Dark, system_notification: true,
        bounds: Some([30., 40., 1200., 800.]), window_state_v2: Some(window_state()),
        ..Preferences::default()
    };
    let encoded = value.encode();
    assert!(encoded.contains("bounds=30,40,1200,800\n"));
    assert_eq!(Preferences::decode(&encoded), value);
    for bad in ["{broken}", "null", "{\"version\":4}"] {
        let text = encoded.lines().filter(|line| !line.starts_with("window_state_v2="))
            .chain(std::iter::once("window_state_v2=" )).collect::<Vec<_>>().join("\n") + bad;
        let decoded = Preferences::decode(&text);
        assert_eq!(decoded.window_state_v2, None);
        assert_eq!(decoded.bounds, value.bounds);
        assert_eq!(decoded.theme, ThemeChoice::Dark);
        assert!(decoded.system_notification);
    }
}

/// 高频完整几何和偏好回执分别保留新屏幕身份与其它字段，退出刷新最后尺寸。
#[test]
fn coalesced_versioned_window_updates_preserve_complete_state_and_other_preferences() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("preferences.txt");
    let (_, writer) = load_from(Some(file.clone()));
    writer.save_window_v2(Some([70., 80., 1200., 800.]), false, window_state());
    for index in 0..64 {
        let state = window_state_with_width(1200. + index as f64);
        writer.save_window_v2(Some([70., 80., 1200. + index as f32, 800.]), index == 63, state);
    }
    writer.mutate(PreferencePatch::Theme(ThemeChoice::Dark)).unwrap().blocking_recv().unwrap().unwrap();
    writer.mutate(PreferencePatch::SystemNotification(true)).unwrap().blocking_recv().unwrap().unwrap();
    writer.finish();
    writer.save_window_v2(None, false, window_state());
    let saved = Preferences::decode(&std::fs::read_to_string(file).unwrap());
    let state = saved.window_state_v2.unwrap();
    assert_eq!(state.display_key, "test-screen");
    assert_eq!(state.origin, window_state().origin);
    assert_eq!(state.content_size.width, 1263.);
    let anchor = window_state_with_width(1263.).anchor;
    assert!((state.anchor.x - anchor.x).abs() < 1e-12);
    assert!((state.anchor.y - anchor.y).abs() < 1e-12);
    assert!(state.maximized);
    assert!(saved.maximized);
    assert_eq!(saved.bounds, Some([70., 80., 1263., 800.]));
    assert_eq!(saved.theme, ThemeChoice::Dark);
    assert!(saved.system_notification);
}

/// 旧入口不能证明当前屏幕身份，不得让陈旧 v2 盖过最后一次普通窗口尺寸。
#[test]
fn legacy_geometry_invalidates_stale_versioned_state() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("preferences.txt");
    let (_, writer) = load_from(Some(file.clone()));
    writer.save_window_v2(Some([70., 80., 1200., 800.]), false, window_state());
    writer.save_window(Some([70., 80., 1263., 800.]), false);
    writer.mutate(PreferencePatch::Language(LanguageChoice::English)).unwrap().blocking_recv().unwrap().unwrap();
    writer.finish();
    let saved = Preferences::decode(&std::fs::read_to_string(file).unwrap());
    assert!(saved.window_state_v2.is_none());
    assert_eq!(saved.bounds, Some([70., 80., 1263., 800.]));
    assert_eq!(saved.language, LanguageChoice::English);
}

/// 读入磁盘的新状态后，首次偏好修改保留它，首次旧几何提交则使其失效。
#[test]
fn loaded_versioned_state_is_preserved_only_by_non_geometry_patches() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("preferences.txt");
    let initial = Preferences { window_state_v2: Some(window_state()), ..Preferences::default() };
    std::fs::write(&file, initial.encode()).unwrap();
    let (loaded, writer) = load_from(Some(file.clone()));
    assert_eq!(loaded.window_state_v2, initial.window_state_v2);
    writer.mutate(PreferencePatch::Theme(ThemeChoice::Dark)).unwrap().blocking_recv().unwrap().unwrap();
    writer.mutate(PreferencePatch::SystemNotification(true)).unwrap().blocking_recv().unwrap().unwrap();
    let preserved = Preferences::decode(&std::fs::read_to_string(&file).unwrap());
    assert_eq!(preserved.window_state_v2, initial.window_state_v2);
    writer.save_window(Some([0., 24., 1300., 850.]), false);
    writer.finish();
    let saved = Preferences::decode(&std::fs::read_to_string(file).unwrap());
    assert!(saved.window_state_v2.is_none());
    assert_eq!(saved.bounds, Some([0., 24., 1300., 850.]));
    assert_eq!(saved.theme, ThemeChoice::Dark);
    assert!(saved.system_notification);
}

/// 原生失败产生的非法v2提交不能删除上一次已确认屏幕身份或普通尺寸。
#[test]
fn invalid_native_geometry_does_not_erase_committed_versioned_state() {
    let root = tempfile::tempdir().unwrap(); let file = root.path().join("preferences.txt");
    let initial = Preferences { window_state_v2: Some(window_state()), bounds: Some([70.,80.,1200.,800.]), ..Default::default() };
    std::fs::write(&file, initial.encode()).unwrap();
    let (_, writer) = load_from(Some(file.clone()));
    let mut bad = window_state(); bad.display_key.clear();
    writer.save_window_v2(Some([0.,0.,1440.,900.]), true, bad);
    writer.mutate(PreferencePatch::Theme(ThemeChoice::Dark)).unwrap().blocking_recv().unwrap().unwrap(); writer.finish();
    let saved = Preferences::decode(&std::fs::read_to_string(file).unwrap());
    assert_eq!(saved.window_state_v2, initial.window_state_v2); assert_eq!(saved.bounds, initial.bounds);
    assert_eq!(saved.maximized, initial.maximized); assert_eq!(saved.theme, ThemeChoice::Dark);
}
