// 本文件的测试覆盖：测试 guard：底层 handler future 析构时发送一次可观察信号；在测试 handler 析构时发布完成信号；外层总截止取消 endpoint cleanup future 时，guard 必须中止并回收已启动的 CDP handler。
// （本文件由主文件 include! 并入，与主文件共用同一个模块作用域。）

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
}

#[tokio::test]
/// 连接后的验证 future 被真实取消时，统一 ownership scope 必须中止并销毁 handler。
async fn cancelled_connected_handler_scope_does_not_detach_task() {
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
    let handler = tokio::spawn(async move {
        let _drop_signal = WatchHandlerDropSignal(Some(dropped_tx));
        let _ = started_tx.send(());
        std::future::pending::<()>().await;
    });
    started_rx.await.expect("connected handler must start");

    let ownership_scope = super::hold_connected_handler_during(
        HandlerTaskGuard::new(handler),
        std::future::pending::<()>(),
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(10), ownership_scope)
            .await
            .is_err(),
        "test timeout must cancel the real ownership future"
    );
    tokio::time::timeout(Duration::from_secs(1), dropped_rx)
        .await
        .expect("cancelled ownership scope must abort the handler promptly")
        .expect("handler drop signal must remain connected");
}

#[tokio::test]
/// 连接后的验证 future panic 时，栈展开也必须经同一 guard 中止并销毁 handler。
async fn panicked_connected_handler_scope_does_not_detach_task() {
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
    let handler = tokio::spawn(async move {
        let _drop_signal = WatchHandlerDropSignal(Some(dropped_tx));
        let _ = started_tx.send(());
        std::future::pending::<()>().await;
    });
    started_rx.await.expect("connected handler must start");

    let ownership_scope = tokio::spawn(super::hold_connected_handler_during(
        HandlerTaskGuard::new(handler),
        async {
            panic!("verification fixture panics after the handler is connected");
            #[allow(unreachable_code)]
            ()
        },
    ));
    let panic = match ownership_scope.await {
        Ok(_) => panic!("ownership scope must propagate the fixture panic"),
        Err(error) => error,
    };
    assert!(panic.is_panic());
    tokio::time::timeout(Duration::from_secs(1), dropped_rx)
        .await
        .expect("panicked ownership scope must abort the handler promptly")
        .expect("handler drop signal must remain connected");
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
    // 源码已拆成同目录分片，契约检查要看到完整文本。
    let source = [include_str!("../lifecycle.rs"), include_str!("../lifecycle_watchers.rs"), include_str!("../lifecycle_reaping.rs")].join("\n");
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

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - lifecycle_tests_shutdown.rs：runtime 插入前的 watcher 也必须被 shutdown 取消，并在 active 登记清零后才允许返回。；区分取消到达时仍在注入，以及取消后已进入回滚的两个关键安装窗口。；在模拟的安装阶段被显式释放前…
include!("lifecycle_tests_shutdown.rs");
