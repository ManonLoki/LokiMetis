//! 偏好解析和后台退出刷新回归使用独立临时文件。

use super::{LanguageChoice, Preferences, ThemeChoice, load_from};

/// 三态主题、语言与设备侧栏选择可以原样往返。
#[test]
fn preferences_round_trip() {
    let value = Preferences { theme: ThemeChoice::Dark, language: LanguageChoice::Chinese, collapsed: true, bounds: Some([30., 40., 1200., 800.]), maximized: true, @@NOTIFICATION_TEST_VALUE@@ ..Preferences::default() };
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
    for index in 0..32 {
        writer.save(&Preferences { collapsed: index % 2 == 0, ..Preferences::default() });
    }
    let expected = Preferences { theme: ThemeChoice::Dark, language: LanguageChoice::English, collapsed: true, ..Preferences::default() };
    writer.save(&expected);
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
