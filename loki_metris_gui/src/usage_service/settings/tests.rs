//! 设置回归只使用隔离文件，验证并发更新、失败回滚及页面丢弃回执后的真实提交。

use super::*;

/// 默认读取零写入，两卡并发保存不会用旧整页快照覆盖彼此。
#[test]
fn settings_default_read_is_write_free_and_parallel_fields_survive_restart() {
    let temp=tempfile::tempdir().unwrap();let directory=temp.path().join("data");
    let service=UsageService::in_directory(directory.clone()).unwrap();
    let defaults=service.usage_settings().unwrap().blocking_recv().unwrap().unwrap();
    assert_eq!(defaults,UsageSettings::default());assert!(!directory.exists());
    let interval=service.save_usage_setting(UsageSettingField::ScanInterval,7).unwrap();
    let retention=service.save_usage_setting(UsageSettingField::RetentionDays,120).unwrap();
    assert_eq!(interval.blocking_recv().unwrap().unwrap().scan_interval.get(),7);
    assert_eq!(retention.blocking_recv().unwrap().unwrap().retention_days.get(),120);
    assert_eq!(service.scan_status().revision,0);assert!(!service.scan_status().running);
    service.finish();
    let service=UsageService::in_directory(directory.clone()).unwrap();
    let settings=service.usage_settings().unwrap().blocking_recv().unwrap().unwrap();
    assert_eq!(settings.scan_interval.get(),7);assert_eq!(settings.retention_days.get(),120);
    assert_eq!(std::fs::read_dir(directory).unwrap().count(),1);service.finish();
}

/// 非法值在任务与文件创建之前拒绝；失败落盘保留上一生效值与失败目标。
#[test]
fn rejected_or_failed_save_preserves_authoritative_settings() {
    let temp=tempfile::tempdir().unwrap();let directory=temp.path().join("data");
    let service=UsageService::in_directory(directory.clone()).unwrap();
    assert!(matches!(service.save_usage_setting(UsageSettingField::ScanInterval,0),Err("usage_setting_invalid")));
    assert!(!directory.exists());
    service.usage_settings().unwrap().blocking_recv().unwrap().unwrap();
    std::fs::create_dir_all(directory.join("usage-settings.json")).unwrap();
    assert_eq!(service.save_usage_setting(UsageSettingField::RetentionDays,120).unwrap().blocking_recv().unwrap(),Err("usage_setting_save_failed"));
    assert_eq!(service.usage_settings().unwrap().blocking_recv().unwrap().unwrap(),UsageSettings::default());
    assert!(directory.join("usage-settings.json").is_dir());service.finish();
}

/// 损坏文件不得被默认值吞掉或覆盖，查询失败后修复文件可显式重试。
#[test]
fn corrupt_settings_are_preserved_and_query_retry_can_recover() {
    let temp=tempfile::tempdir().unwrap();let directory=temp.path().join("data");std::fs::create_dir_all(&directory).unwrap();
    let path=directory.join("usage-settings.json");std::fs::write(&path,b"invalid json").unwrap();
    let service=UsageService::in_directory(directory).unwrap();
    assert_eq!(service.usage_settings().unwrap().blocking_recv().unwrap(),Err("usage_settings_unavailable"));
    assert_eq!(std::fs::read(&path).unwrap(),b"invalid json");std::fs::write(&path,b"{}").unwrap();
    assert_eq!(service.usage_settings().unwrap().blocking_recv().unwrap().unwrap(),UsageSettings::default());service.finish();
}

/// 页面丢弃接收端不撤销已接受保存，服务代数和下一次读取最终反映真实提交。
#[test]
fn discarded_receipt_does_not_cancel_accepted_settings_save() {
    let temp=tempfile::tempdir().unwrap();let service=UsageService::in_directory(temp.path().join("data")).unwrap();
    service.usage_settings().unwrap().blocking_recv().unwrap().unwrap();let revision=service.usage_settings_revision();
    drop(service.save_usage_setting(UsageSettingField::ScanInterval,9).unwrap());
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(3);
    while service.usage_settings_revision()==revision {assert!(std::time::Instant::now()<deadline);std::thread::sleep(std::time::Duration::from_millis(10));}
    assert_eq!(service.usage_settings().unwrap().blocking_recv().unwrap().unwrap().scan_interval.get(),9);service.finish();
}

/// 设置预检失败必须成为可观察失败，不创建索引，也不覆盖其他 writer 的 running 状态。
#[test]
fn invalid_settings_fail_scan_before_io_with_visible_terminal_revision() {
    let temp=tempfile::tempdir().unwrap();let directory=temp.path().join("data");std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("usage-settings.json"),b"broken").unwrap();
    let service=UsageService::in_directory(directory.clone()).unwrap();
    service.context.status.lock().unwrap().running=true;
    let result=service.runtime.lock().unwrap().as_ref().unwrap().block_on(super::super::scan::run(service.context.clone(),super::super::queue::Intent{client:None,origin:loki_metris_core::ScanStartOrigin::ExplicitUser},loki_metris_core::ScanCancellation::new()));
    assert_eq!(result,Err("usage_settings_unavailable"));let status=service.scan_status();
    assert!(status.running);assert_eq!(status.error,Some("usage_settings_unavailable"));assert_eq!(status.revision,1);
    assert_eq!(std::fs::read_dir(directory).unwrap().count(),1);service.finish();
}
