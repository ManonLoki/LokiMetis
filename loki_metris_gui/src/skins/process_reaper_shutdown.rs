// 本文件是 process_reaper.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：退出等待、保留进程回收与 reaper 关闭流程，以及本模块单元测试

/// 等待进程级 shutdown；发送端意外消失也按关闭处理。
pub(super) async fn wait_for_process_shutdown(receiver: &mut watch::Receiver<bool>) {
    if *receiver.borrow_and_update() {
        return;
    }
    while receiver.changed().await.is_ok() {
        if *receiver.borrow_and_update() {
            return;
        }
    }
}

/// 在指定 owner 中逐项回收 retained 子进程，批次移出期间仍占用原总槽位。
async fn reap_retained_processes_until_in(
    reaper: &'static ProcessReaper,
    deadline: tokio::time::Instant,
) -> Result<usize, OwnedProcessError> {
    let mut batch = RetainedChildBatch {
        children: {
            let mut state = lock_reaper(reaper);
            std::mem::take(&mut state.retained)
        },
        reaper,
    };
    let mut reaped = 0_usize;
    let mut first_error = None;
    while let Some(child) = batch.children.pop() {
        let mut owner = OwnedProcessChild::new_retained(child, reaper);
        match owner.terminate_and_reap_until(deadline).await {
            Ok(()) => reaped = reaped.saturating_add(1),
            Err(error) if first_error.is_none() => first_error = Some(error),
            Err(_) => {}
        }
    }
    first_error.map_or(Ok(reaped), Err)
}

/// 应用退出只建立一次总截止，并继续持有最终期限内尚未到终态的句柄。
pub(super) async fn shutdown_process_reaper(timeout: Duration) -> Result<usize, OwnedProcessError> {
    shutdown_process_reaper_in(process_reaper(), timeout).await
}

/// 在指定 owner 中关闭 spawn 门禁，并在首次绝对截止内等待全部槽位释放。
async fn shutdown_process_reaper_in(
    reaper: &'static ProcessReaper,
    timeout: Duration,
) -> Result<usize, OwnedProcessError> {
    let (deadline, shutdown_signals) = begin_process_shutdown_in(reaper, timeout);
    for shutdown in shutdown_signals {
        let _ = shutdown.send(true);
    }

    let mut reaped = 0_usize;
    loop {
        match reap_retained_processes_until_in(reaper, deadline).await {
            Ok(count) => reaped = reaped.saturating_add(count),
            Err(OwnedProcessError::ReapTimedOut) => return Err(OwnedProcessError::ReapTimedOut),
            Err(_) => {}
        }
        if lock_reaper(reaper).owned_processes == 0 {
            return Ok(reaped);
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(OwnedProcessError::ReapTimedOut);
        }
        tokio::select! {
            _ = reaper.changed.notified() => {}
            _ = tokio::time::sleep(PROCESS_REAPER_STATE_POLL.min(remaining)) => {}
        }
    }
}

/// 对指定 owner 原子关闭门禁，供生产 shutdown 与隔离竞态回归复用。
fn begin_process_shutdown_in(
    reaper: &'static ProcessReaper,
    timeout: Duration,
) -> (tokio::time::Instant, Vec<watch::Sender<bool>>) {
    let now = tokio::time::Instant::now();
    let mut state = lock_reaper(reaper);
    state.shutting_down = true;
    let deadline = *state.shutdown_deadline.get_or_insert(now + timeout);
    let signals = state.active.values().cloned().collect::<Vec<_>>();
    drop(state);
    reaper.changed.notify_waiters();
    (deadline, signals)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    /// 为并行回归分配不污染生产单例的进程 owner。
    fn isolated_reaper() -> &'static ProcessReaper {
        Box::leak(Box::new(ProcessReaper::default()))
    }

    /// 构造不继承标准流的长时间测试子进程。
    fn sleeping_command() -> Command {
        let mut command = Command::new("/bin/sleep");
        command
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }

    /// 回收截止已经耗尽时必须保存原 Child，下一轮可继续等待到真实终态。
    #[tokio::test]
    async fn reap_timeout_retains_child_until_a_later_attempt() {
        let reaper = isolated_reaper();
        let mut owner = OwnedProcessChild::spawn_in(&mut sleeping_command(), reaper)
            .expect("测试子进程应可启动");
        let expired = tokio::time::Instant::now()
            .checked_sub(Duration::from_millis(1))
            .expect("测试截止可构造");
        assert_eq!(
            owner.terminate_and_reap_until(expired).await,
            Err(OwnedProcessError::ReapTimedOut)
        );
        {
            let state = lock_reaper(reaper);
            assert_eq!(state.retained.len(), 1);
            assert_eq!(state.owned_processes, 1);
        }

        reap_retained_processes_until_in(
            reaper,
            tokio::time::Instant::now() + Duration::from_secs(1),
        )
        .await
        .expect("下一次命令必须先回收已 kill 的子进程");
        let state = lock_reaper(reaper);
        assert!(state.retained.is_empty());
        assert_eq!(state.owned_processes, 0);
    }

    /// active owner 必须在同步 spawn 返回前登记，并在确认终态后原子移除。
    #[tokio::test]
    async fn active_child_is_registered_for_shutdown_until_reaped() {
        let reaper = isolated_reaper();
        let mut owner = OwnedProcessChild::spawn_in(&mut sleeping_command(), reaper)
            .expect("测试子进程应可启动");
        let registration_id = owner
            .registration_id
            .expect("主动子进程必须保存自己的登记标识");
        {
            let state = lock_reaper(reaper);
            assert!(state.active.contains_key(&registration_id));
            assert_eq!(state.spawn_reservations, 0);
            assert_eq!(state.owned_processes, 1);
        }
        owner
            .terminate_and_reap_until(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
            .expect("测试子进程必须可终止并回收");
        let state = lock_reaper(reaper);
        assert!(!state.active.contains_key(&registration_id));
        assert_eq!(state.owned_processes, 0);
    }

    /// spawn 必须按 reservation、锁外启动、原子移交的顺序消除 shutdown TOCTOU。
    #[test]
    fn spawn_registration_has_no_shutdown_toctou() {
        let source = include_str!("process_reaper.rs");
        let spawn = source
            .split_once("fn spawn_command")
            .expect("spawn_command helper must exist")
            .1
            .split_once("/// 在 reservation 已持有 Child")
            .expect("spawn_command helper boundary must exist")
            .0;
        let child = spawn
            .find("command.spawn()")
            .expect("spawn must create the child");
        let adoption = spawn
            .find("self.finish_adoption()")
            .expect("spawn must transfer the child into the owner");
        assert!(child < adoption);
        assert!(spawn.contains("self.child = Some(command.spawn()"));
        assert!(!spawn.contains("lock_reaper("));
        assert!(!spawn.contains("lock_process_reaper("));
    }

    /// 登记 ID 回绕时必须跳过仍 active 的键，不得覆盖其 shutdown sender。
    #[test]
    fn registration_id_wrap_skips_active_entries() {
        let mut state = ProcessReaperState {
            next_registration_id: u64::MAX,
            ..ProcessReaperState::default()
        };
        let (max_sender, _) = watch::channel(false);
        let (zero_sender, _) = watch::channel(false);
        state.active.insert(u64::MAX, max_sender);
        state.active.insert(0, zero_sender);

        assert_eq!(allocate_registration_id(&mut state), 1);
        assert_eq!(state.next_registration_id, 2);
        assert!(state.active.contains_key(&u64::MAX) && state.active.contains_key(&0));
    }

    /// reservation 已持有 Child 后的 Drop 必须在转 retained 前锁外发布终止。
    #[tokio::test]
    async fn dropped_spawn_reservation_kills_before_retaining_child() {
        let reaper = isolated_reaper();
        let mut reservation = ProcessSpawnReservation::reserve(reaper).expect("reserve slot");
        let mut command = sleeping_command();
        command.kill_on_drop(true);
        reservation.child = Some(command.spawn().expect("spawn outside owner mutex"));

        drop(reservation);
        let child = {
            let mut state = lock_reaper(reaper);
            assert_eq!(state.spawn_reservations, 0);
            assert_eq!(state.owned_processes, 1);
            state.retained.pop().expect("drop must retain child")
        };
        let mut owner = OwnedProcessChild::new_retained(child, reaper);
        let status = tokio::time::timeout(Duration::from_secs(1), owner.child_mut().wait())
            .await
            .expect("drop must request termination before retention")
            .expect("retained child wait");
        assert!(!status.success());
        owner.finish_reaped();
        assert_eq!(lock_reaper(reaper).owned_processes, 0);
    }

    /// shutdown 必须等待在途 reservation，其锁外 spawn 结果只能转入 retained 再回收。
    #[tokio::test]
    async fn shutdown_waits_for_in_flight_spawn_reservation() {
        let reaper = isolated_reaper();
        let reservation = ProcessSpawnReservation::reserve(reaper).expect("reserve slot");
        let shutdown = tokio::spawn(shutdown_process_reaper_in(reaper, Duration::from_secs(2)));
        for _ in 0..100 {
            if lock_reaper(reaper).shutting_down {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(lock_reaper(reaper).shutting_down);

        let mut command = sleeping_command();
        command.kill_on_drop(true);
        let child = command.spawn().expect("spawn outside owner mutex");
        assert!(matches!(
            reservation.adopt(child),
            Err(OwnedProcessError::ShuttingDown)
        ));

        assert_eq!(shutdown.await.expect("shutdown task"), Ok(1));
        let state = lock_reaper(reaper);
        assert_eq!(state.spawn_reservations, 0);
        assert_eq!(state.owned_processes, 0);
        assert!(state.active.is_empty() && state.retained.is_empty());
    }

    /// active 与 reservation 共用严格容量，满额时稳定拒绝且 Drop 归还槽位。
    #[tokio::test]
    async fn process_owner_capacity_includes_spawn_reservations() {
        let reaper = isolated_reaper();
        let mut owner = OwnedProcessChild::spawn_in(&mut sleeping_command(), reaper)
            .expect("active process slot");
        let reservations = (1..OWNED_PROCESS_CAPACITY)
            .map(|_| ProcessSpawnReservation::reserve(reaper).expect("capacity slot"))
            .collect::<Vec<_>>();
        assert!(matches!(
            ProcessSpawnReservation::reserve(reaper),
            Err(OwnedProcessError::CapacityExceeded)
        ));
        {
            let state = lock_reaper(reaper);
            assert_eq!(state.active.len(), 1);
            assert_eq!(state.spawn_reservations, OWNED_PROCESS_CAPACITY - 1);
            assert_eq!(state.owned_processes, OWNED_PROCESS_CAPACITY);
        }

        drop(reservations);
        owner
            .terminate_and_reap_until(tokio::time::Instant::now() + Duration::from_secs(1))
            .await
            .expect("active process must be reaped");
        let state = lock_reaper(reaper);
        assert_eq!(state.spawn_reservations, 0);
        assert_eq!(state.owned_processes, 0);
    }

    /// 重复 shutdown 不得重置首次建立的绝对截止时间。
    #[test]
    fn repeated_shutdown_keeps_the_first_deadline() {
        let reaper = isolated_reaper();
        let (first, _) = begin_process_shutdown_in(reaper, Duration::from_millis(50));
        let (second, _) = begin_process_shutdown_in(reaper, Duration::from_secs(30));
        assert_eq!(first, second);
    }
}
