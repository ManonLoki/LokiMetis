// 本文件是 store_activation.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：带参数的商店版 Codex 激活调用，以及本模块单元测试

/// 在独立 STA 线程执行 COM 激活；所有成功初始化步骤都在同一线程对称清理。
fn activate_store_codex_with_arguments(
    arguments: &str,
    control: &StoreActivationControl,
) -> StoreActivationResult {
    // SAFETY: 当前函数只在新建专用线程调用，并在成功初始化后于同线程 CoUninitialize。
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
        .ok()
        .map_err(|_| StoreActivationFailure::ComInitialization)?;

    let result = (|| {
        // SAFETY: None 对应 Win32 要求的保留空参数，并在本函数末尾对称禁用。
        unsafe { CoEnableCallCancellation(None) }
            .map_err(|_| StoreActivationFailure::CancellationSetup)?;
        control
            .thread_id
            // SAFETY: 只读取当前线程标识，无参数与副作用。
            .store(unsafe { GetCurrentThreadId() }, Ordering::Release);
        control
            .phase
            .store(ACTIVATION_PHASE_COM_CALL, Ordering::Release);
        if control.is_cancelled() {
            return Err(StoreActivationFailure::Cancelled);
        }

        // SAFETY: COM 已在当前专用 STA 线程初始化，接口只在该线程内使用。
        let manager: IApplicationActivationManager = unsafe {
            CoCreateInstance(
                &ApplicationActivationManager,
                None::<&windows::core::IUnknown>,
                CLSCTX_LOCAL_SERVER,
            )
        }
        .map_err(|_| control.failure_or_cancelled(StoreActivationFailure::ManagerCreation))?;
        if control.is_cancelled() {
            return Err(StoreActivationFailure::Cancelled);
        }
        // SAFETY: manager 是当前线程拥有的有效 COM 接口。
        if unsafe { CoAllowSetForegroundWindow(&manager, None) }.is_err() {
            tracing::warn!("Windows declined foreground permission for Store activation");
        }
        if control.is_cancelled() {
            return Err(StoreActivationFailure::Cancelled);
        }

        let arguments = arguments
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        if control.is_cancelled() {
            return Err(StoreActivationFailure::Cancelled);
        }
        // SAFETY: AUMID 固定；arguments 在同步调用完成前保持 NUL 结尾且有效。
        unsafe {
            manager.ActivateApplication(
                w!("OpenAI.Codex_2p2nqsd0c76g0!App"),
                PCWSTR(arguments.as_ptr()),
                AO_NONE,
            )
        }
        .map(|_| ())
        .map_err(|_| control.failure_or_cancelled(StoreActivationFailure::Activation))
    })();

    // SAFETY: 仅在 CoEnableCallCancellation 成功后才可能进入 COM_CALL phase。
    if control.phase.load(Ordering::Acquire) == ACTIVATION_PHASE_COM_CALL
        && unsafe { CoDisableCallCancellation(None) }.is_err()
    {
        tracing::warn!("Windows failed to disable COM call cancellation during worker cleanup");
    }
    // SAFETY: 与本线程开头成功的 CoInitializeEx 配对。
    unsafe { CoUninitialize() };
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Instant;

    /// 在有限时间内由测试调用线程 join 已终止的指定 Store 激活线程。
    fn reap_owned_task_until_terminal(owner: &StoreActivationOwner, id: u64) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while owner.owned_task_count() != 0 && Instant::now() < deadline {
            owner.reap_finished(id);
            thread::park_timeout(Duration::from_millis(1));
        }
        assert_eq!(owner.owned_task_count(), 0);
    }

    #[tokio::test]
    /// 工作线程返回的业务失败必须原样送达等待方并完成回收。
    async fn worker_failure_is_returned_to_the_caller() {
        let owner = Arc::new(StoreActivationOwner::default());
        let lease = owner
            .start(|_| Err(StoreActivationFailure::ManagerCreation))
            .expect("worker starts");
        let id = lease.id;

        let outcome = lease
            .wait(Duration::from_secs(1), Duration::from_millis(50))
            .await
            .expect("lifecycle completes");

        assert_eq!(outcome, Err(StoreActivationFailure::ManagerCreation));
        reap_owned_task_until_terminal(owner.as_ref(), id);
    }

    #[tokio::test]
    /// 工作线程 panic 必须转成可观察失败，且句柄最终由 owner 回收。
    async fn worker_panic_is_observable_and_reaped() {
        let owner = Arc::new(StoreActivationOwner::default());
        let lease = owner
            .start(|_| panic!("simulated activation panic"))
            .expect("worker starts");
        let id = lease.id;

        let outcome = lease
            .wait(Duration::from_secs(1), Duration::from_millis(50))
            .await
            .expect("lifecycle completes");

        assert_eq!(outcome, Err(StoreActivationFailure::WorkerPanicked));
        reap_owned_task_until_terminal(owner.as_ref(), id);
    }

    #[tokio::test]
    /// 等待方被丢弃时应取消协作式工作线程并等待 owner 清空。
    async fn dropping_the_waiter_cancels_and_reaps_a_cooperative_worker() {
        let owner = Arc::new(StoreActivationOwner::default());
        let lease = owner
            .start(|control| {
                while !control.is_cancelled() {
                    thread::park_timeout(Duration::from_millis(1));
                }
                Err(StoreActivationFailure::Cancelled)
            })
            .expect("worker starts");
        let id = lease.id;
        assert_eq!(owner.owned_task_count(), 1);

        drop(lease);
        reap_owned_task_until_terminal(owner.as_ref(), id);
    }

    #[tokio::test]
    /// 超时应保持有界，同时在工作线程终止前持续保留其所有权。
    async fn timeout_is_bounded_and_owner_retains_worker_until_terminal() {
        let owner = Arc::new(StoreActivationOwner::default());
        let (release_tx, release_rx) = mpsc::channel();
        let lease = owner
            .start(move |_| {
                let _ = release_rx.recv();
                Err(StoreActivationFailure::Cancelled)
            })
            .expect("worker starts");
        let id = lease.id;
        let started_at = Instant::now();

        let result = lease
            .wait(Duration::from_millis(30), Duration::from_millis(10))
            .await;

        assert!(matches!(
            result,
            Err(StoreActivationLifecycleFailure::Timeout { .. })
        ));
        assert!(started_at.elapsed() < Duration::from_secs(1));
        assert_eq!(owner.owned_task_count(), 1);

        release_tx.send(()).expect("release worker");
        reap_owned_task_until_terminal(owner.as_ref(), id);
    }

    #[test]
    /// 已有副作用激活运行时必须拒绝并发启动第二个激活。
    fn concurrent_side_effecting_activation_is_rejected() {
        let owner = Arc::new(StoreActivationOwner::default());
        let (release_tx, release_rx) = mpsc::channel();
        let first = owner
            .start(move |_| {
                let _ = release_rx.recv();
                Ok(())
            })
            .expect("first worker starts");
        let id = first.id;

        assert!(matches!(
            owner.start(|_| Ok(())),
            Err(StoreActivationLifecycleFailure::Busy)
        ));

        release_tx.send(()).expect("release first worker");
        drop(first);
        reap_owned_task_until_terminal(owner.as_ref(), id);
    }

    #[tokio::test]
    /// 结果发布后的尾段仍必须保留句柄，线程退出后才允许下一次 start 回收并继续。
    async fn published_outcome_does_not_detach_the_worker_tail() {
        let owner = Arc::new(StoreActivationOwner::default());
        let (release_tx, release_rx) = mpsc::channel();
        let tail_finished = Arc::new(AtomicBool::new(false));
        let worker_tail_finished = Arc::clone(&tail_finished);
        let first = owner
            .start_with_worker_tail(
                |_| Ok(()),
                move || {
                    let _ = release_rx.recv();
                    worker_tail_finished.store(true, Ordering::Release);
                },
            )
            .expect("first worker starts");

        let outcome = first
            .wait(Duration::from_secs(1), Duration::from_millis(50))
            .await
            .expect("published outcome remains observable");

        assert_eq!(outcome, Ok(()));
        assert!(!tail_finished.load(Ordering::Acquire));
        assert_eq!(owner.owned_task_count(), 1);
        assert!(matches!(
            owner.start(|_| Ok(())),
            Err(StoreActivationLifecycleFailure::Busy)
        ));

        release_tx.send(()).expect("release worker tail");
        let deadline = Instant::now() + Duration::from_secs(1);
        let second = loop {
            match owner.start(|_| Ok(())) {
                Ok(lease) => break lease,
                Err(StoreActivationLifecycleFailure::Busy) if Instant::now() < deadline => {
                    thread::park_timeout(Duration::from_millis(1));
                }
                Err(error) => {
                    panic!("next start did not reap the terminal worker: {error:?}")
                }
            }
        };
        let second_id = second.id;
        assert!(tail_finished.load(Ordering::Acquire));
        assert_eq!(
            second
                .wait(Duration::from_secs(1), Duration::from_millis(50))
                .await
                .expect("second lifecycle completes"),
            Ok(())
        );
        reap_owned_task_until_terminal(owner.as_ref(), second_id);
    }
}
