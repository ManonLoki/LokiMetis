#[test]
/// handler owner 在 spawn 前按 active + retained 总数施加固定上限背压。
fn handler_owner_capacity_is_bounded() {
    let mut owner = super::HandlerTaskReaper::default();
    for _ in 0..super::MAX_OWNED_HANDLER_TASKS {
        assert!(super::try_reserve_handler_task_capacity_locked(&mut owner));
    }
    assert_eq!(owner.owned_tasks, super::MAX_OWNED_HANDLER_TASKS);
    assert!(!super::has_handler_task_capacity(&owner));
    assert!(!super::try_reserve_handler_task_capacity_locked(&mut owner));
    assert_eq!(owner.owned_tasks, super::MAX_OWNED_HANDLER_TASKS);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// 退出 gate 在同一短锁内拒绝新 reservation，并取消已登记的 active handler。
async fn handler_owner_shutdown_gate_closes_capacity_and_aborts_active_task() {
    let mut owner = super::HandlerTaskReaper::default();
    let task = tokio::spawn(std::future::pending::<()>());
    let task_id = task.id();
    owner.active.insert(task_id, task.abort_handle());
    owner.owned_tasks = 1;

    super::close_handler_task_reservations_locked(&mut owner);

    assert!(owner.reservations_closed);
    assert!(!super::has_handler_task_capacity(&owner));
    assert!(!super::try_reserve_handler_task_capacity_locked(&mut owner));
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("shutdown gate must promptly abort the active handler")
        .expect_err("active handler must be cancelled by the shutdown gate");
}

#[test]
/// 生产 shutdown 在独立进程关闭全局 gate，并等待 active probe guard 释放所有权。
fn production_shutdown_closes_handler_gate_and_waits_for_active_probe() {
    const CHILD_ENV: &str = "LOKI_METIS_HANDLER_SHUTDOWN_GATE_CHILD";
    if std::env::var_os(CHILD_ENV).is_none() {
        let status = std::process::Command::new(
            std::env::current_exe().expect("current unit-test executable must exist"),
        )
        .args([
            "--exact",
            "skins::tests::production_shutdown_closes_handler_gate_and_waits_for_active_probe",
            "--nocapture",
        ])
        .env(CHILD_ENV, "1")
        .status()
        .expect("isolated shutdown-gate test process must start");
        assert!(status.success(), "isolated shutdown-gate test must pass");
        return;
    }

    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("test runtime must build")
        .block_on(async {
            let root = temp_directory("production-handler-shutdown");
            let service = SkinService::new(root.join("builtin"), root.join("user"));
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (terminal_tx, terminal_rx) = tokio::sync::oneshot::channel();
            let (handler_release_tx, handler_release_rx) = std::sync::mpsc::channel();
            let handler = tokio::spawn(async move {
                let _drop_signal = WatchHandlerDropSignal(Some(terminal_tx));
                let _ = started_tx.send(());
                let _ = handler_release_rx.recv();
            });
            started_rx.await.expect("active probe handler must start");
            let guard = HandlerTaskGuard::new(handler);
            let (probe_release_tx, probe_release_rx) = tokio::sync::oneshot::channel();
            let probe = tokio::spawn(async move {
                let (_guard, _) =
                    super::hold_connected_handler_during(guard, probe_release_rx).await;
            });

            let mut shutdown = Box::pin(service.shutdown());
            tokio::time::timeout(Duration::from_millis(10), shutdown.as_mut())
                .await
                .expect_err("shutdown must wait while the active probe owns its handler");
            let gate_error = match super::reserve_handler_task_capacity() {
                Ok(_) => panic!("production shutdown must close new handler reservations"),
                Err(error) => error,
            };
            assert_eq!(gate_error.code, "skin.cdp_failed");

            handler_release_tx
                .send(())
                .expect("blocked handler must be released");
            tokio::time::timeout(Duration::from_secs(1), terminal_rx)
                .await
                .expect("active handler must reach terminal state")
                .expect("handler terminal signal must remain connected");
            tokio::time::timeout(Duration::from_millis(10), shutdown.as_mut())
                .await
                .expect_err("shutdown must still wait until the active guard releases ownership");

            probe_release_tx
                .send(())
                .expect("active probe must be released");
            tokio::time::timeout(Duration::from_secs(1), shutdown.as_mut())
                .await
                .expect("shutdown must finish after the active guard releases ownership");
            probe.await.expect("active probe task must finish");
            std::fs::remove_dir_all(root).expect("handler shutdown test directory must be removed");
        });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// helper future 未开始 poll 就被取消时，handler 仍由稳定 owner 持有直到真实终态。
async fn unpolled_connected_handler_scope_retains_task_until_terminal() {
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let handler = tokio::spawn(async move {
        let _drop_signal = WatchHandlerDropSignal(Some(dropped_tx));
        let _ = started_tx.send(());
        let _ = release_rx.recv();
    });
    let task_id = handler.id();
    started_rx.await.expect("connected handler must start");

    let ownership_scope = super::hold_connected_handler_during(
        HandlerTaskGuard::new(handler),
        std::future::pending::<()>(),
    );
    drop(ownership_scope);
    assert!(
        handler_task_is_retained(task_id),
        "unpolled ownership future must return the aborted JoinHandle to the stable owner"
    );

    assert!(
        tokio::time::timeout(
            Duration::from_millis(10),
            reap_handler_tasks_until(tokio::time::Instant::now() + Duration::from_secs(1)),
        )
        .await
        .is_err(),
        "fixture must cancel the real reaper while the handler has not reached terminal state"
    );
    assert!(
        handler_task_is_retained(task_id),
        "cancelled reaper must restore the original JoinHandle"
    );
    reap_handler_tasks_until(tokio::time::Instant::now() + Duration::from_millis(10))
        .await
        .expect_err("shared deadline must expire while the handler is still blocked");
    assert!(
        handler_task_is_retained(task_id),
        "deadline expiry must keep the original JoinHandle in the stable owner"
    );

    release_tx.send(()).expect("release handler fixture");
    tokio::time::timeout(Duration::from_secs(1), dropped_rx)
        .await
        .expect("released handler must reach terminal state")
        .expect("handler drop signal must remain connected");
    reap_handler_tasks_until(tokio::time::Instant::now() + Duration::from_secs(1))
        .await
        .expect("terminal handler must be reaped");
    assert!(!handler_task_is_retained(task_id));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// SkinService shutdown 使用既有共同截止等待 retained handler 确认真正终态。
async fn shutdown_reaps_retained_handler_before_returning() {
    let root = temp_directory("shutdown-handler-owner");
    let service = SkinService::new(root.join("builtin"), root.join("user"));
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let handler = tokio::spawn(async move {
        let _drop_signal = WatchHandlerDropSignal(Some(dropped_tx));
        let _ = started_tx.send(());
        let _ = release_rx.recv();
    });
    let task_id = handler.id();
    started_rx.await.expect("handler fixture must start");
    drop(HandlerTaskGuard::new(handler));
    assert!(handler_task_is_retained(task_id));

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        let _ = release_tx.send(());
    });
    let budget = super::WatchStopBudget {
        cooperative: Duration::from_millis(50),
        total: Duration::from_millis(500),
        abort_reap: Duration::from_millis(50),
    };
    let mut cleanup = |_host, _endpoint| async { Ok(0) };
    service
        .shutdown_with_budget_and_cleanup(budget, &mut cleanup)
        .await
        .expect("shutdown must wait for the retained handler within the shared deadline");
    tokio::time::timeout(Duration::from_secs(1), dropped_rx)
        .await
        .expect("shutdown must observe handler terminal state")
        .expect("handler drop signal must remain connected");
    assert!(!handler_task_is_retained(task_id));
    std::fs::remove_dir_all(root).expect("handler owner test directory must be removed");
}
