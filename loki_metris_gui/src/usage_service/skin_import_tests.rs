//! 真实ZIP与临时目录验证租约回收，不连接任何已安装宿主。

use super::*;
use std::time::{Duration, Instant};

/// 等待应用owner的真实终态，避免只检查取消标记。
fn wait(done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !done() {
        assert!(Instant::now() < deadline, "import owner did not settle");
        std::thread::sleep(Duration::from_millis(5));
    }
}
/// 使用生产创建/导出生成有效纯主题，目标库与来源库相互隔离。
fn archive(root: &std::path::Path) -> PathBuf {
    let source = UsageService::in_test_directory(root.join("source")).unwrap();
    let bytes = source
        .skin_work(|service| async move {
            let skin = service
                .create_user_theme("Import fixture", "Fixture")
                .await?;
            service.build_export_archive(&loki_metris_core::SkinReference {
                source: skin.source,
                id: skin.id,
            })
        })
        .unwrap()
        .blocking_recv()
        .unwrap()
        .unwrap();
    let path = root.join("fixture.zip");
    std::fs::write(&path, bytes).unwrap();
    source.finish();
    path
}
#[test]
/// ready租约析构后由已接受supervisor清理，未再次依赖普通操作提交。
fn ready_import_lease_drop_cleans_staging_and_registry() {
    let root = tempfile::tempdir().unwrap();
    let path = archive(root.path());
    let target = UsageService::in_test_directory(root.path().join("target")).unwrap();
    let (lease, work) = target.prepare_skin_import(vec![path]).unwrap();
    let batch = work.blocking_recv().unwrap().unwrap();
    let staging = root
        .path()
        .join("target/skins")
        .join(format!(".import-{}", batch.token));
    assert!(staging.is_dir());
    drop(lease);
    wait(|| !staging.exists() && target.context.skins.imports.lock().unwrap().is_empty());
    target.finish();
}
#[test]
/// 导入刚提交即离页，已接管token的事务仍真实安装到目标库。
fn accepted_commit_survives_lease_drop_without_canceling_installed_theme() {
    let root = tempfile::tempdir().unwrap();
    let path = archive(root.path());
    let target = UsageService::in_test_directory(root.path().join("target")).unwrap();
    let (lease, work) = target.prepare_skin_import(vec![path]).unwrap();
    let batch = work.blocking_recv().unwrap().unwrap();
    let token = lease.session.reserve_commit().unwrap();
    let receipt = target
        .commit_skin_import(
            lease.session.clone(),
            token,
            vec![batch.items[0].item_id.clone()],
            false,
        )
        .unwrap();
    drop(lease);
    let result = receipt.blocking_recv().unwrap().unwrap();
    assert_eq!(result.installed.len(), 1);
    assert!(
        root.path()
            .join("target/skins")
            .join(&result.installed[0].id)
            .is_dir()
    );
    wait(|| target.context.skins.imports.lock().unwrap().is_empty());
    target.finish();
}
#[test]
/// 无token的前置校验失败也能退出supervisor，不遗留注册项。
fn failed_preparation_without_started_event_releases_owner() {
    let root = tempfile::tempdir().unwrap();
    let target = UsageService::in_test_directory(root.path().join("target")).unwrap();
    let (lease, work) = target.prepare_skin_import(Vec::new()).unwrap();
    assert!(work.blocking_recv().unwrap().is_err());
    drop(lease);
    wait(|| target.context.skins.imports.lock().unwrap().is_empty());
    target.finish();
}

#[test]
/// supervisor未获准入时没有后台owner，注册表必须同步撤回空租约。
fn rejected_supervisor_does_not_leave_an_import_registration() {
    let root = tempfile::tempdir().unwrap();
    let target = UsageService::in_test_directory(root.path().join("target")).unwrap();
    target.finish();
    assert!(target.prepare_skin_import(Vec::new()).is_err());
    assert!(target.context.skins.imports.lock().unwrap().is_empty());
}
#[test]
/// 页面在Started之前消失，迟到token仍会被同一session取消。
fn abandoned_session_cancels_late_started_token() {
    let root = tempfile::tempdir().unwrap();
    let service = SkinService::new(root.path().join("builtin"), root.path().join("user"));
    let session = ImportSession {
        state: Mutex::new(ImportState::default()),
    };
    session.abandon();
    session.cancel(&service).unwrap();
    assert!(session.finished());
    session.event(
        &service,
        SkinImportPreparationEvent::Started {
            token: "123-456".into(),
            total_files: 1,
        },
    );
    assert!(session.finished());
    assert!(session.reserve_commit().is_err());
}
#[test]
/// 资源库服务退出不能关闭同进程的全局CDP准入；重复资源服务仍可用。
fn catalog_only_shutdown_keeps_process_runtime_gate_open() {
    let root = tempfile::tempdir().unwrap();
    for index in 0..2 {
        let service = UsageService::in_test_directory(root.path().join(index.to_string())).unwrap();
        let entries = service.skin_catalog().unwrap().blocking_recv().unwrap().unwrap().entries;
        assert_eq!(entries.len(), 6);
        assert!(entries.iter().all(|entry| entry.skin.preview_data_url.capacity() == 0),
            "原生目录快照不得保留已清空Base64字段的分配容量");
        assert!(entries.iter().all(|entry| entry.preview.is_some()), "释放编码容量不能丢失原始预览");
        assert!(
            !service
                .context
                .skins
                .started
                .load(std::sync::atomic::Ordering::Acquire)
        );
        service.finish();
    }
}
