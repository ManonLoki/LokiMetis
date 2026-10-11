// 本文件是 lifecycle.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：runtime 插入前的 watcher 也必须被 shutdown 取消，并在 active 登记清零后才允许返回。；区分取消到达时仍在注入，以及取消后已进入回滚的两个关键安装窗口。；在模拟的安装阶段被显式释放前，验证 shutdown 始终持有 completion 等待责任。

#[tokio::test]
/// runtime 插入前的 watcher 也必须被 shutdown 取消，并在 active 登记清零后才允许返回。
async fn shutdown_waits_for_watcher_registered_before_runtime_insertion() {
    let root = temp_directory("active-watcher-before-runtime");
    let service = SkinService::new(root.join("builtin"), root.join("user"));
    let (cancel, mut cancellation) = tokio::sync::watch::channel(false);
    let registration = service
        .register_active_watch_task(cancel.clone())
        .expect("open service must accept watcher registration");
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (observed_tx, observed_rx) = tokio::sync::oneshot::channel();
    let join = tokio::spawn(async move {
        let _registration = registration;
        let _ = started_tx.send(());
        let _ = cancellation.changed().await;
        let _ = observed_tx.send(());
        Ok::<usize, AppError>(7)
    });
    started_rx
        .await
        .expect("watcher must enter its active scope");
    let handler_abort = join.abort_handle();
    let task = super::WatchTask::new(
        &service.watch_task_reaper,
        SkinHostKind::Codex,
        cancel,
        join,
        handler_abort,
        CdpEndpoint::default(),
    );
    assert_eq!(service.lock_watch_task_reaper().active.len(), 1);
    assert!(service.runtime.lock().await.instances.is_empty());

    let budget = super::WatchStopBudget {
        cooperative: Duration::from_millis(100),
        total: Duration::from_millis(500),
        abort_reap: Duration::from_millis(50),
    };
    let mut cleanup = |_: SkinHostKind, _: CdpEndpoint| std::future::ready(Ok(0));
    assert_eq!(
        service
            .shutdown_with_budget_and_cleanup(budget, &mut cleanup)
            .await
            .expect("shutdown must wait for the active watcher scope"),
        0
    );
    observed_rx
        .await
        .expect("registered watcher must observe shutdown cancellation");
    assert!(service.lock_watch_task_reaper().active.is_empty());
    assert!(!task.is_running());
    assert_eq!(
        service
            .stop_watch_tasks(vec![task])
            .await
            .expect("completed pre-insertion watcher must remain collectable"),
        7
    );
    std::fs::remove_dir_all(root).expect("test directory must be removed");
}

/// 区分取消到达时仍在注入，以及取消后已进入回滚的两个关键安装窗口。
enum ParkedInstallPhase {
    InitialInjection,
    Rollback,
}

/// 在模拟的安装阶段被显式释放前，验证 shutdown 始终持有 completion 等待责任。
async fn assert_shutdown_waits_for_parked_install_phase(
    label: &str,
    phase_kind: ParkedInstallPhase,
) {
    let root = temp_directory(label);
    let service = Arc::new(SkinService::new(root.join("builtin"), root.join("user")));
    let (completion, install_cancel, mut cancellation) = service
        .begin_install_completion()
        .expect("open service must accept install completion registration");
    let _codex_operation = service
        .bind_install_codex_operation(install_cancel)
        .expect("install cancellation must use the registered sender");
    let (operation_ready_tx, operation_ready_rx) = tokio::sync::oneshot::channel();
    let (phase_entered_tx, phase_entered_rx) = tokio::sync::oneshot::channel();
    let (release_phase_tx, release_phase_rx) = tokio::sync::oneshot::channel();
    let phase = tokio::spawn(async move {
        let _completion = completion;
        let _ = operation_ready_tx.send(());
        match phase_kind {
            ParkedInstallPhase::InitialInjection => {
                let _ = phase_entered_tx.send(());
                cancellation
                    .changed()
                    .await
                    .expect("shutdown must cancel the parked injection");
                assert!(*cancellation.borrow());
            }
            ParkedInstallPhase::Rollback => {
                cancellation
                    .changed()
                    .await
                    .expect("shutdown must initiate rollback cancellation");
                assert!(*cancellation.borrow());
                let _ = phase_entered_tx.send(());
            }
        }
        release_phase_rx
            .await
            .expect("test must release the parked install phase");
    });
    operation_ready_rx
        .await
        .expect("install operation must enter its completion scope");

    let shutdown_service = Arc::clone(&service);
    let budget = super::WatchStopBudget {
        cooperative: Duration::from_millis(100),
        total: Duration::from_secs(1),
        abort_reap: Duration::from_millis(50),
    };
    let mut shutdown = tokio::spawn(async move {
        let mut cleanup = |_: SkinHostKind, _: CdpEndpoint| std::future::ready(Ok(0));
        shutdown_service
            .shutdown_with_budget_and_cleanup(budget, &mut cleanup)
            .await
    });
    phase_entered_rx
        .await
        .expect("target install phase must be parked");
    assert!(
        tokio::time::timeout(Duration::from_millis(30), &mut shutdown)
            .await
            .is_err(),
        "shutdown must not return while the install phase remains parked"
    );

    release_phase_tx
        .send(())
        .expect("parked install phase must still be alive");
    phase.await.expect("install phase task must finish");
    assert_eq!(
        shutdown
            .await
            .expect("shutdown task must finish")
            .expect("shutdown must finish after the install phase exits"),
        0
    );
    assert!(service.lock_watch_task_reaper().active.is_empty());
    std::fs::remove_dir_all(root).expect("test directory must be removed");
}

#[tokio::test]
/// 初次注入停驻时，shutdown 必须取消安装并等待注入作用域真实退出。
async fn shutdown_waits_for_parked_initial_injection() {
    assert_shutdown_waits_for_parked_install_phase(
        "parked-initial-injection",
        ParkedInstallPhase::InitialInjection,
    )
    .await;
}

#[tokio::test]
/// 取消后的回滚停驻时，shutdown 必须等待回滚完成而不能只等待 watcher。
async fn shutdown_waits_for_parked_initial_injection_rollback() {
    assert_shutdown_waits_for_parked_install_phase(
        "parked-initial-injection-rollback",
        ParkedInstallPhase::Rollback,
    )
    .await;
}

#[test]
/// 安装代码必须在任何 watcher 回收、宿主启动或注入前建立 completion 与持久取消绑定。
fn install_registers_completion_before_any_async_host_mutation() {
    let source = [include_str!("../service_install_body.rs"), include_str!("../service_install_body_install.rs"), include_str!("../service_install_body_uninstall.rs")].join("\n");
    let install = source
        .split_once("pub async fn install(")
        .expect("install method must exist")
        .1
        .split_once("pub async fn uninstall(")
        .expect("uninstall method must delimit install")
        .0;
    let completion = install
        .find("self.begin_install_completion()")
        .expect("install must register shutdown completion");
    let cancellation = install
        .find("self.bind_install_codex_operation(install_cancel)")
        .expect("install must bind the same cancellation sender");
    let watcher_reap = install
        .find("self.reap_watch_tasks().await?")
        .expect("install must reap previous watchers");
    let launch = install
        .find("connect_or_launch(")
        .expect("install must connect or launch the host");
    let injection = install
        .find("wait_for_initial_injection(")
        .expect("install must perform initial injection");
    assert!(completion < cancellation);
    assert!(cancellation < watcher_reap);
    assert!(watcher_reap < launch);
    assert!(launch < injection);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// 安装 future 意外析构 WatchTask 时不得 detach，原 JoinHandle 必须返回 service owner。
async fn dropped_watch_task_returns_join_handle_to_service_owner() {
    let root = temp_directory("dropped-watch-task-owner");
    let service = SkinService::new(root.join("builtin"), root.join("user"));
    let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let task_release = Arc::clone(&release);
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (cancel, _) = tokio::sync::watch::channel(false);
    let join = tokio::spawn(async move {
        let _ = started_tx.send(());
        while !task_release.load(Ordering::Acquire) {
            std::thread::park_timeout(Duration::from_millis(1));
        }
        Ok::<usize, AppError>(1)
    });
    started_rx.await.expect("watcher must start");
    let handler_abort = join.abort_handle();
    let task = super::WatchTask::new(
        &service.watch_task_reaper,
        SkinHostKind::Codex,
        cancel,
        join,
        handler_abort,
        CdpEndpoint::default(),
    );

    drop(task);
    assert_eq!(service.retained_watch_task_count(), 1);
    assert!(service.lock_watch_task_reaper().retained[0].join.is_some());

    release.store(true, Ordering::Release);
    let budget = super::WatchStopBudget {
        cooperative: Duration::from_millis(20),
        total: Duration::from_millis(500),
        abort_reap: Duration::from_millis(250),
    };
    let (cooperative_deadline, final_deadline) = budget.deadlines(tokio::time::Instant::now());
    let mut cleanup = |_: SkinHostKind, _: CdpEndpoint| std::future::ready(Ok(0));
    service
        .reap_retained_watch_tasks_until(
            cooperative_deadline,
            final_deadline,
            budget.abort_reap,
            &mut cleanup,
        )
        .await
        .expect("service owner must reap the returned watcher handle");
    assert_eq!(service.retained_watch_task_count(), 0);

    let lifecycle = [include_str!("../lifecycle.rs"), include_str!("../lifecycle_watchers.rs"), include_str!("../lifecycle_reaping.rs")].join("\n");
    assert!(lifecycle.contains("retain_process_watch_task(task);"));
    std::fs::remove_dir_all(root).expect("test directory must be removed");
}

#[tokio::test]
/// 应用退出必须排空运行态 watcher、发布合作取消并关闭后续 watcher 登记。
async fn service_shutdown_reaps_live_watchers_and_closes_registration() {
    let root = temp_directory("watcher-shutdown");
    let service = SkinService::new(root.join("builtin"), root.join("user"));
    let (cancel, mut cancellation) = tokio::sync::watch::channel(false);
    let (observed_tx, observed_rx) = tokio::sync::oneshot::channel();
    let join = tokio::spawn(async move {
        let _ = cancellation.changed().await;
        let _ = observed_tx.send(());
        Ok::<usize, AppError>(4)
    });
    let handler_abort = join.abort_handle();
    service.runtime.lock().await.instances.insert(
        "codex:test".into(),
        super::InstanceRuntime {
            active: None,
            task: Some(super::WatchTask::new_test(
                SkinHostKind::Codex,
                cancel,
                join,
                handler_abort,
                CdpEndpoint::default(),
            )),
            compatibility: None,
        },
    );
    let budget = super::WatchStopBudget {
        cooperative: Duration::from_millis(50),
        total: Duration::from_millis(100),
        abort_reap: Duration::from_millis(10),
    };
    let mut cleanup = |_: SkinHostKind, _: CdpEndpoint| std::future::ready(Ok(0));

    let affected = tokio::time::timeout(
        Duration::from_secs(1),
        service.shutdown_with_budget_and_cleanup(budget, &mut cleanup),
    )
    .await
    .expect("service shutdown must remain bounded")
    .expect("cooperative watcher shutdown must finish");
    assert_eq!(affected, 4);

    observed_rx.await.expect("watcher must observe shutdown");
    assert!(service.runtime.lock().await.instances.is_empty());
    assert_eq!(service.retained_watch_task_count(), 0);
    assert_eq!(
        service
            .ensure_watch_runtime_open()
            .expect_err("shutdown service must reject new watchers")
            .code,
        "skin.operation_cancelled"
    );
    std::fs::remove_dir_all(root).expect("test directory must be removed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// 关闭发生在operation锁等待期间，排队的宿主动作不得开始进程发现或重启。
async fn queued_host_mutations_recheck_closing_after_operation_lock() {
    let root = tempfile::tempdir().unwrap();
    let service = Arc::new(SkinService::new(root.path().join("builtin"), root.path().join("user")));
    let lock = service.operation.lock().await;
    let launch = { let service = service.clone(); tokio::spawn(async move {service.launch_host(SkinHostKind::Codex).await}) };
    let force = { let service = service.clone(); tokio::spawn(async move {service.force_launch_host(SkinHostKind::Codex).await}) };
    let restart = { let service = service.clone(); tokio::spawn(async move {service.restart_host_instance(SkinHostKind::Codex, "never-inspect-this-instance").await}) };
    let stop = { let service = service.clone(); tokio::spawn(async move {service.uninstall(SkinHostKind::Codex, None).await}) };
    service.close_host_admission();
    drop(lock);
    assert_eq!(launch.await.unwrap().unwrap_err().code, "skin.operation_cancelled");
    assert_eq!(force.await.unwrap().unwrap_err().code, "skin.operation_cancelled");
    assert_eq!(restart.await.unwrap().unwrap_err().code, "skin.operation_cancelled");
    assert_eq!(stop.await.unwrap().unwrap_err().code, "skin.operation_cancelled");
}
