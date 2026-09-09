/// 测试 guard：底层 handler future 析构时发送一次可观察信号。
struct WatchHandlerDropSignal(Option<tokio::sync::oneshot::Sender<()>>);

impl Drop for WatchHandlerDropSignal {
    /// 在测试 handler 析构时发布完成信号。
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

#[test]
/// Tauri 的真实退出事件必须调用 SkinService owner，而不是只依赖状态析构。
fn app_exit_invokes_skin_service_shutdown() {
    let source = include_str!("../../lib.rs");
    let exit = source
        .split_once("tauri::RunEvent::ExitRequested")
        .expect("application exit handler must exist")
        .1;
    assert!(exit.contains("try_state::<skins::SkinService>()"));
    assert!(exit.contains("service.shutdown().await"));
    assert!(include_str!("../lifecycle.rs").contains("shutdown_process_reaper"));
}

#[tokio::test]
/// 外层总截止取消 endpoint cleanup future 时，guard 必须中止并回收已启动的 CDP handler。
async fn cancelled_endpoint_cleanup_scope_aborts_handler_task() {
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
    let handler = tokio::spawn(async move {
        let _drop_signal = WatchHandlerDropSignal(Some(dropped_tx));
        let _ = started_tx.send(());
        std::future::pending::<()>().await;
    });
    started_rx.await.expect("handler must start");

    let cleanup_scope = async move {
        let _handler = HandlerTaskGuard::new(handler);
        std::future::pending::<()>().await;
    };
    assert!(
        tokio::time::timeout(Duration::from_millis(10), cleanup_scope)
            .await
            .is_err()
    );
    tokio::time::timeout(Duration::from_secs(1), dropped_rx)
        .await
        .expect("aborted handler future must be dropped")
        .expect("handler drop signal must remain connected");

    let source = include_str!("../injection.rs");
    let cleanup_via = source
        .split_once("async fn cleanup_via")
        .expect("cleanup helper must exist")
        .1;
    assert!(cleanup_via.contains("mut handler_task: HandlerTaskGuard"));
    assert!(
        source
            .matches("HandlerTaskGuard::new(handler_task)")
            .count()
            >= 2
    );
}

#[tokio::test]
/// watcher panic 绕过自身收尾时，service 仍必须用独立 AbortHandle 回收 CDP handler。
async fn failed_watcher_join_aborts_independent_handler() {
    let root = temp_directory("failed-watcher-handler");
    let service = SkinService::new(root.join("builtin"), root.join("user"));
    let (handler_started_tx, handler_started_rx) = tokio::sync::oneshot::channel();
    let (handler_dropped_tx, handler_dropped_rx) = tokio::sync::oneshot::channel();
    let handler = tokio::spawn(async move {
        let _drop_signal = WatchHandlerDropSignal(Some(handler_dropped_tx));
        let _ = handler_started_tx.send(());
        std::future::pending::<()>().await;
    });
    handler_started_rx.await.expect("handler must start");
    let handler_abort = handler.abort_handle();
    drop(handler);

    let (cancel, _) = tokio::sync::watch::channel(false);
    let join = tokio::spawn(async move {
        panic!("watcher fixture panics before its handler cleanup");
        #[allow(unreachable_code)]
        Ok::<usize, AppError>(0)
    });
    while !join.is_finished() {
        tokio::task::yield_now().await;
    }
    let task = super::WatchTask::new_test(
        SkinHostKind::Codex,
        cancel,
        join,
        handler_abort,
        CdpEndpoint::default(),
    );
    let budget = super::WatchStopBudget {
        cooperative: Duration::from_millis(50),
        total: Duration::from_millis(250),
        abort_reap: Duration::from_millis(50),
    };
    let (cooperative_deadline, final_deadline) = budget.deadlines(tokio::time::Instant::now());
    let mut cleanup = |_: SkinHostKind, _: CdpEndpoint| std::future::ready(Ok(0));

    let error = service
        .stop_watch_tasks_until(
            vec![task],
            cooperative_deadline,
            final_deadline,
            budget.abort_reap,
            &mut cleanup,
        )
        .await
        .expect_err("watcher panic must remain observable");
    assert_eq!(error.code, "skin.task_failed");
    tokio::time::timeout(Duration::from_secs(1), handler_dropped_rx)
        .await
        .expect("handler abort must remain bounded")
        .expect("handler future must be destroyed");
    assert_eq!(service.retained_watch_task_count(), 0);
    std::fs::remove_dir_all(root).expect("test directory must be removed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// watcher 在 abort 后仍未到终态时，原 JoinHandle 必须留在 service 中并由后续操作回收。
async fn overdue_watcher_is_retained_and_reaped_by_a_later_operation() {
    let root = temp_directory("retained-watcher");
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
        Ok::<usize, AppError>(3)
    });
    started_rx.await.expect("watcher must start");
    let handler_abort = join.abort_handle();
    let task = super::WatchTask::new_test(
        SkinHostKind::Codex,
        cancel,
        join,
        handler_abort,
        CdpEndpoint::default(),
    );
    let budget = super::WatchStopBudget {
        cooperative: Duration::from_millis(5),
        total: Duration::from_millis(20),
        abort_reap: Duration::from_millis(5),
    };
    let (cooperative_deadline, final_deadline) = budget.deadlines(tokio::time::Instant::now());
    let mut cleanup = |_: SkinHostKind, _: CdpEndpoint| std::future::ready(Ok(0));

    let error = tokio::time::timeout(
        Duration::from_millis(250),
        service.stop_watch_tasks_until(
            vec![task],
            cooperative_deadline,
            final_deadline,
            budget.abort_reap,
            &mut cleanup,
        ),
    )
    .await
    .expect("watcher stop must remain bounded")
    .expect_err("non-terminal watcher must report the bounded timeout");

    assert_eq!(error.code, "skin.task_stop_timeout");
    assert_eq!(service.retained_watch_task_count(), 1);
    assert!(service.lock_watch_task_reaper().retained[0].join.is_some());

    release.store(true, Ordering::Release);
    for _ in 0..100 {
        let finished = service.lock_watch_task_reaper().retained[0]
            .join
            .as_ref()
            .is_some_and(tokio::task::JoinHandle::is_finished);
        if finished {
            break;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    let (cooperative_deadline, final_deadline) = budget.deadlines(tokio::time::Instant::now());
    service
        .reap_retained_watch_tasks_until(
            cooperative_deadline,
            final_deadline,
            budget.abort_reap,
            &mut cleanup,
        )
        .await
        .expect("later operation must reap the retained handle");
    assert_eq!(service.retained_watch_task_count(), 0);
    std::fs::remove_dir_all(root).expect("test directory must be removed");
}

#[tokio::test]
/// endpoint cleanup 也必须服从 watcher 总截止，并把尚未清理的责任留给下一轮。
async fn endpoint_cleanup_uses_shared_deadline_and_is_retried() {
    let root = temp_directory("retained-endpoint-cleanup");
    let service = SkinService::new(root.join("builtin"), root.join("user"));
    let (cancel, _) = tokio::sync::watch::channel(false);
    let join = tokio::spawn(async {
        Err::<usize, AppError>(AppError::new("skin.test_failure", "test failure"))
    });
    while !join.is_finished() {
        tokio::task::yield_now().await;
    }
    let handler_abort = join.abort_handle();
    let task = super::WatchTask::new_test(
        SkinHostKind::Codex,
        cancel,
        join,
        handler_abort,
        CdpEndpoint::default(),
    );
    let budget = super::WatchStopBudget {
        cooperative: Duration::from_millis(5),
        total: Duration::from_millis(20),
        abort_reap: Duration::from_millis(5),
    };
    let (cooperative_deadline, final_deadline) = budget.deadlines(tokio::time::Instant::now());
    let mut blocked_cleanup =
        |_: SkinHostKind, _: CdpEndpoint| std::future::pending::<Result<usize, AppError>>();

    let error = tokio::time::timeout(
        Duration::from_millis(250),
        service.stop_watch_tasks_until(
            vec![task],
            cooperative_deadline,
            final_deadline,
            budget.abort_reap,
            &mut blocked_cleanup,
        ),
    )
    .await
    .expect("endpoint cleanup must remain bounded")
    .expect_err("cleanup that crosses the total deadline must fail observably");

    assert_eq!(error.code, "skin.task_stop_timeout");
    assert_eq!(service.retained_watch_task_count(), 1);
    assert!(service.lock_watch_task_reaper().retained[0].join.is_none());

    let cleanup_calls = Arc::new(AtomicUsize::new(0));
    let observed_calls = Arc::clone(&cleanup_calls);
    let mut completed_cleanup = move |_: SkinHostKind, _: CdpEndpoint| {
        observed_calls.fetch_add(1, Ordering::AcqRel);
        std::future::ready(Ok(2))
    };
    let (cooperative_deadline, final_deadline) = budget.deadlines(tokio::time::Instant::now());
    assert_eq!(
        service
            .reap_retained_watch_tasks_until(
                cooperative_deadline,
                final_deadline,
                budget.abort_reap,
                &mut completed_cleanup,
            )
            .await
            .expect("later cleanup attempt must complete"),
        2
    );
    assert_eq!(cleanup_calls.load(Ordering::Acquire), 1);
    assert_eq!(service.retained_watch_task_count(), 0);
    std::fs::remove_dir_all(root).expect("test directory must be removed");
}

#[tokio::test]
/// cleanup 返回显式错误时也必须保留端点责任；连续失败后仍可由下一轮成功回收。
async fn failed_endpoint_cleanup_is_retained_until_a_later_success() {
    let root = temp_directory("failed-endpoint-cleanup");
    let service = SkinService::new(root.join("builtin"), root.join("user"));
    let (cancel, _) = tokio::sync::watch::channel(false);
    let join = tokio::spawn(async {
        Err::<usize, AppError>(AppError::new("skin.test_failure", "test failure"))
    });
    while !join.is_finished() {
        tokio::task::yield_now().await;
    }
    let handler_abort = join.abort_handle();
    let task = super::WatchTask::new_test(
        SkinHostKind::Codex,
        cancel,
        join,
        handler_abort,
        CdpEndpoint::default(),
    );
    let budget = super::WatchStopBudget {
        cooperative: Duration::from_millis(10),
        total: Duration::from_millis(100),
        abort_reap: Duration::from_millis(10),
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let observed_calls = Arc::clone(&calls);
    let mut cleanup = move |_: SkinHostKind, _: CdpEndpoint| {
        let attempt = observed_calls.fetch_add(1, Ordering::AcqRel);
        std::future::ready(if attempt < 2 {
            Err(AppError::new("skin.cleanup_failed", "cleanup failed"))
        } else {
            Ok(3)
        })
    };

    let (cooperative_deadline, final_deadline) = budget.deadlines(tokio::time::Instant::now());
    let error = service
        .stop_watch_tasks_until(
            vec![task],
            cooperative_deadline,
            final_deadline,
            budget.abort_reap,
            &mut cleanup,
        )
        .await
        .expect_err("first cleanup failure must remain observable");
    assert_eq!(error.code, "skin.test_failure");
    assert_eq!(service.retained_watch_task_count(), 1);
    assert!(service.lock_watch_task_reaper().retained[0].join.is_none());

    let (cooperative_deadline, final_deadline) = budget.deadlines(tokio::time::Instant::now());
    let error = service
        .reap_retained_watch_tasks_until(
            cooperative_deadline,
            final_deadline,
            budget.abort_reap,
            &mut cleanup,
        )
        .await
        .expect_err("second cleanup failure must also remain observable");
    assert_eq!(error.code, "skin.cleanup_failed");
    assert_eq!(service.retained_watch_task_count(), 1);

    let (cooperative_deadline, final_deadline) = budget.deadlines(tokio::time::Instant::now());
    assert_eq!(
        service
            .reap_retained_watch_tasks_until(
                cooperative_deadline,
                final_deadline,
                budget.abort_reap,
                &mut cleanup,
            )
            .await
            .expect("third cleanup attempt must release retained responsibility"),
        3
    );
    assert_eq!(calls.load(Ordering::Acquire), 3);
    assert_eq!(service.retained_watch_task_count(), 0);
    std::fs::remove_dir_all(root).expect("test directory must be removed");
}

#[test]
/// watcher 错误、JoinError、abort 终态与 reaper 的 cleanup Err 分支都必须保留责任。
fn every_completed_cleanup_error_branch_retains_ownership() {
    let source = include_str!("../lifecycle.rs");
    let stop = source
        .split_once("async fn stop_watch_task")
        .expect("stop helper must exist")
        .1
        .split_once("async fn reap_retained_watch_tasks_until")
        .expect("reaper helper must exist")
        .0;
    assert_eq!(
        stop.matches("task.retain_in_service(self);").count(),
        7,
        "cleanup errors, shared deadlines and an unconfirmed abort must retain full ownership"
    );
    let reaper = source
        .split_once("async fn reap_retained_watch_tasks_until")
        .expect("reaper helper must exist")
        .1;
    assert_eq!(
        reaper.matches("task.retain_in_service(self);").count(),
        3,
        "reaper join timeout, cleanup error and cleanup deadline must all retain ownership"
    );
}

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
    let source = include_str!("../service_install_body.rs");
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

    let lifecycle = include_str!("../lifecycle.rs");
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
