// 本文件的测试覆盖：排队停用在离页后释放锁，不摘除原watcher、不发出取消信号；操作子任务必须拥有独立维护寿命，作用域退出也不能污染下一项操作。
// （本文件由主文件 include! 并入，与主文件共用同一个模块作用域。）

// 操作锁后失权必须在宿主检查之前退出；watcher与清理不继承GUI作用域。

#[tokio::test]
/// 排队停用在离页后释放锁，不摘除原watcher、不发出取消信号。
async fn queued_stop_rechecks_authority_without_removing_existing_watcher() {
    let root = tempfile::tempdir().unwrap();
    let service = Arc::new(SkinService::new(
        root.path().join("builtin"),
        root.path().join("user"),
    ));
    let (cancel, receiver) = tokio::sync::watch::channel(false);
    let handler = tokio::spawn(std::future::pending::<()>());
    let watcher = tokio::spawn(std::future::pending::<Result<usize, AppError>>());
    let key = super::runtime_instance_key(SkinHostKind::Codex, "owned-fixture");
    service.runtime.lock().await.instances.insert(
        key.clone(),
        super::InstanceRuntime {
            active: None,
            compatibility: None,
            task: Some(super::WatchTask::new_test(
                SkinHostKind::Codex,
                cancel,
                watcher,
                handler.abort_handle(),
                CdpEndpoint::new(9341),
            )),
        },
    );
    let lock = service.operation.lock().await;
    let allowed = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let pending = {
        let service = service.clone();
        let allowed = allowed.clone();
        tokio::spawn(super::with_host_authority(
            Arc::new(move || allowed.load(Ordering::Acquire)),
            async move { service.uninstall(SkinHostKind::Codex, None).await },
        ))
    };
    tokio::task::yield_now().await;
    allowed.store(false, Ordering::Release);
    drop(lock);
    assert_eq!(
        pending.await.unwrap().unwrap_err().code,
        "skin.operation_cancelled"
    );
    assert!(!*receiver.borrow());
    let task = service
        .runtime
        .lock()
        .await
        .instances
        .remove(&key)
        .unwrap()
        .task
        .unwrap();
    assert!(task.is_running());
    let mut retained = task.into_retained();
    retained.handler_abort.take().unwrap().abort();
    let join = retained.join.take().unwrap();
    join.abort();
    let _ = join.await;
    let _ = handler.await;
}

#[tokio::test]
/// 操作子任务必须拥有独立维护寿命，作用域退出也不能污染下一项操作。
async fn host_authority_is_scoped_to_operation_polling_and_not_spawned_maintenance() {
    super::with_host_authority(Arc::new(|| false), async {
        assert!(super::check_host_authority().is_err());
        tokio::task::yield_now().await;
        assert!(super::check_host_authority().is_err());
        assert!(
            tokio::spawn(async { super::check_host_authority() })
                .await
                .unwrap()
                .is_ok()
        );
    })
    .await;
    assert!(super::check_host_authority().is_ok());
}
