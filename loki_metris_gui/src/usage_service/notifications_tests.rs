// 本文件是 notifications.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：本模块的单元测试

/// 文件事务在 blocking 岛执行，返回原 owner；失败时不提交候选状态。
async fn persist(mut ledger: Ledger, next: MilestoneState) -> (Ledger, Result<bool, &'static str>) {
    if ledger.state == next {
        return (ledger, Ok(false));
    }
    tokio::task::spawn_blocking(move || {
        let result = ledger.commit(next);
        (ledger, result)
    })
    .await
    .expect("通知文件驱动必须保留 owner")
}
/// 每次只发布语义变化，成功观测时间内部更新但不制造相同值的 UI 刷新风暴。
fn publish(
    tx: &watch::Sender<Progress>,
    ledger: &Ledger,
    snapshot: &Option<TodayUsageSnapshot>,
    error: Option<&'static str>,
    today: &TodayReadStatus,
) {
    let day = ledger.state.active();
    let mut next = Progress {
        pending: day.map_or(0, MilestoneDay::pending_count),
        submitted: ledger
            .state
            .days
            .iter()
            .fold(0_u64, |n, d| n.saturating_add(d.submitted)),
        baseline: day.map_or(0, |d| d.baseline),
        suppressed: ledger
            .state
            .days
            .iter()
            .fold(0_u64, |n, d| n.saturating_add(d.suppressed)),
        expired: ledger
            .state
            .days
            .iter()
            .fold(0_u64, |n, d| n.saturating_add(d.expired)),
        unknown: ledger
            .state
            .days
            .iter()
            .fold(0_u64, |n, d| n.saturating_add(d.unknown_count)),
        unknown_id: day.and_then(|d| {
            d.reservation
                .as_ref()
                .filter(|r| r.unknown)
                .map(|r| milestone_request_id(&d.day.key(), r.bucket))
        }),
        issue: ledger.state.issue,
        error: error.or(if ledger.read_error {
            Some("notification_history_rebuilt")
        } else {
            None
        }),
        snapshot: snapshot.clone(),
        today: today.clone(),
        revision: tx.borrow().revision,
    };
    let mut old = tx.borrow().clone();
    if let (Some(a), Some(b)) = (&mut old.snapshot, &mut next.snapshot) {
        a.observed_ms = b.observed_ms;
    }
    if old != next {
        next.revision = next.revision.wrapping_add(1);
    }
    // 观测时间仍更新内存；否则长期同值后的权限恢复会被前台永久判作过期。
    tx.send_replace(next);
}
/// 单在途查询与预约不展开大跨度；唤醒后每轮重算本地时区与日界。
async fn run(
    context: Arc<ServiceContext>,
    mut controls: watch::Receiver<Control>,
    progress: watch::Sender<Progress>,
    requests: mpsc::Sender<(u64, MilestoneRequest)>,
    mut receipts: mpsc::Receiver<Receipt>,
    overflow: Arc<AtomicBool>,
) {
    let path = context.directory.join("usage-notification-state.json");
    let mut ledger = tokio::task::spawn_blocking(move || Ledger::load(path))
        .await
        .expect("通知读取 owner");
    let mut recovered = ledger.state.clone();
    recovered.recover();
    let (loaded, result) = persist(ledger, recovered).await;
    ledger = loaded;
    let mut error = result.err();
    let mut snapshot: Option<TodayUsageSnapshot> = None;
    let mut next_refresh = 0_i64;
    let mut retry = 0;
    let mut epoch = context.notification_source_epoch.load(Ordering::Acquire);
    let mut read = TodayReadStatus {
        source_epoch: epoch,
        ..Default::default()
    };
    loop {
        let control = controls.borrow_and_update().clone();
        if !control.initialized && !control.visible && !control.closing {
            let _ = controls.changed().await;
            continue;
        }
        let now = current_epoch_ms();
        // 关闭后拒收新回执，但已经被有界通道接受的结果必须全部落盘。
        if control.closing {
            receipts.close();
        }
        let mut next = ledger.state.clone();
        next.set_enabled(control.enabled);
        if control.retry != retry {
            retry = control.retry;
            next.retry(now);
            error = None;
        }
        let reset = context.notification_source_epoch.load(Ordering::Acquire);
        if epoch != reset {
            epoch = reset;
            next.source_reset();
            snapshot = None;
            read.source_epoch = epoch;
            read.failed = false;
            next_refresh = 0;
        }
        while let Ok(receipt) = receipts.try_recv() {
            match receipt {
                Receipt::Outcome(id, outcome) => next.complete(&id, outcome, now),
                Receipt::LateFailure(id) => next.late_failure(&id, now),
                Receipt::Unsubmitted(id) => next.defer_unsubmitted(&id, now),
            }
        }
        let day = UsageDay::capture(now, &jiff::tz::TimeZone::system());
        read.day = day.clone();
        // 桌宠独立于通知账本；即使通知从未开启，也要撤下旧日快照。
        if snapshot
            .as_ref()
            .is_some_and(|value| day.as_ref() != Some(&value.day))
        {
            snapshot = None;
            read.failed = false;
            next_refresh = 0;
        }
        // 不论有无可信新数据，旧日队列都过期；绝不能用昨天快照发送“今天”。
        if next
            .days
            .last()
            .is_some_and(|d| day.as_ref() != Some(&d.day))
        {
            for old in &mut next.days {
                old.cancel_for_expiry();
            }
            next.issue = Some(MilestoneIssue::Expired);
        }
        if overflow.swap(false, Ordering::AcqRel) {
            error = Some("notification_callback_capacity");
        }
        if let Some(reservation) = next.days.last().and_then(|d| d.reservation.as_ref())
            && !reservation.unknown && now.saturating_sub(reservation.started_ms) >= 15_000 {
                let id =
                    milestone_request_id(&next.days.last().unwrap().day.key(), reservation.bucket);
                next.complete(&id, MilestoneOutcome::Unknown, now);
            }
        let (saved, result) = persist(ledger, next).await;
        ledger = saved;
        if let Err(failure) = result {
            error = Some(failure);
        }
        if control.closing {
            read.closed = true;
            publish(&progress, &ledger, &snapshot, error, &read);
            tracing::info!(event = "notification_worker_stopped", issue = error);
            break;
        }
        let demand = control.enabled || control.visible;
        if demand && now >= next_refresh {
            publish(&progress, &ledger, &snapshot, error, &read);
            let query_epoch = epoch;
            let zone = jiff::tz::TimeZone::system();
            let cancellation = ScanCancellation::default();
            let loaded = tokio::select! {
                // 只取消只读摘要的等待；persist 不在此 select 内，已接受事务仍由原 owner 收尾。
                biased;
                _ = controls.changed() => {
                    cancellation.cancel();
                    next_refresh = 0;
                    continue;
                },
                loaded = today::load(context.clone(), now, zone, cancellation.clone()) => loaded,
            };
            let after = current_epoch_ms();
            next_refresh = after.saturating_add(30_000);
            if query_epoch != context.notification_source_epoch.load(Ordering::Acquire) {
                continue;
            }
            read.day = UsageDay::capture(after, &jiff::tz::TimeZone::system());
            match loaded {
                Ok((_, value)) if read.day.as_ref() == Some(&value.day) => {
                    snapshot = Some(value);
                    read.source_epoch = query_epoch;
                    read.failed = false;
                    if error == Some("notification_snapshot_unavailable") {
                        error = None;
                    }
                }
                _ => {
                    read.failed = true;
                    error = Some("notification_snapshot_unavailable");
                }
            }
        }
        // 关闭、来源变化在查询期间到来时先处理最新控制，不准预约旧意图。
        if controls.has_changed().unwrap_or(true) {
            continue;
        }
        let now = current_epoch_ms();
        let mut next = ledger.state.clone();
        let value = snapshot.as_ref().filter(|s| day.as_ref() == Some(&s.day));
        if let Some(value) = value
            && error.is_none() {
                next.observe(value, now);
            }
        // 先持久入队，再单独持久预约；前一步失败绝不能派发系统调用。
        let (saved, result) = persist(ledger, next).await;
        ledger = saved;
        if let Err(failure) = result {
            error = Some(failure);
        }
        let mut next = ledger.state.clone();
        let request = value
            .filter(|_| error.is_none())
            .and_then(|value| next.reserve(value, control.permission, now));
        let (saved, result) = persist(ledger, next).await;
        ledger = saved;
        match result {
            Ok(_) => {
                if let Some(request) = request
                    && requests.try_send((epoch, request.clone())).is_err() {
                        let mut next = ledger.state.clone();
                        next.complete(&request.id, MilestoneOutcome::Unknown, now);
                        let (saved, result) = persist(ledger, next).await;
                        ledger = saved;
                        error = result.err();
                    }
            }
            Err(failure) => error = Some(failure),
        }
        publish(&progress, &ledger, &snapshot, error, &read);
        // 空闲最多30秒；活跃预约15秒到期、成功10秒节流，确定失败用core截止。
        let mut due = if demand {
            next_refresh
        } else {
            day.as_ref().map_or(now + 30_000, |d| d.end_ms)
        };
        if let Some(d) = ledger.state.active() {
            if let Some(r) = &d.reservation {
                if !r.unknown {
                    due = due.min(r.started_ms + 15_000);
                }
            } else if control.enabled
                && control.permission == NotificationPermission::Allowed
                && error.is_none()
                && !d.retries.iter().any(|r| r.failures >= 4)
            {
                if let Some(r) = d
                    .retries
                    .iter()
                    .filter(|r| r.failures < 4)
                    .min_by_key(|r| r.bucket)
                {
                    due = due.min(r.due_ms.max(now + 1_000));
                } else if !d.pending.is_empty() {
                    due = due.min(ledger.state.next_send_ms.max(now + 1_000));
                }
            }
        }
        tokio::select! {
            _=controls.changed()=>{next_refresh=0;},
            _=context.today_refresh.notified()=>{tokio::time::sleep(Duration::from_millis(250)).await;next_refresh=0;},
            receipt=receipts.recv()=>{if let Some(receipt)=receipt {
                let mut next=ledger.state.clone();let now=current_epoch_ms();
                match receipt {Receipt::Outcome(id,value)=>next.complete(&id,value,now),Receipt::LateFailure(id)=>next.late_failure(&id,now),Receipt::Unsubmitted(id)=>next.defer_unsubmitted(&id,now)}
                let (saved,result)=persist(ledger,next).await;ledger=saved;if let Err(failure)=result {error=Some(failure);}
            }},
            _=tokio::time::sleep(Duration::from_millis(due.saturating_sub(current_epoch_ms()).max(1_000) as u64))=>{},
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loki_metris_core::{Completeness, Confidence, TodayAvailability};
    /// 相同总量每次真实读取都更新新鲜度，但不能制造UI revision风暴。
    #[test]
    fn notification_projection_refreshes_age_without_ui_revision() {
        let root = tempfile::tempdir().unwrap();
        let ledger = Ledger::load(root.path().join("state.json"));
        let now = "2026-10-10T12:00:00Z"
            .parse::<jiff::Timestamp>()
            .unwrap()
            .as_millisecond();
        let mut value = TodayUsageSnapshot {
            day: UsageDay::capture(now, &jiff::tz::TimeZone::UTC).unwrap(),
            source: "all".into(),
            total_tokens: 350_000_000,
            observed_ms: now,
            availability: TodayAvailability::Available,
            completeness: Completeness::Complete,
            confidence: Confidence::Exact,
        };
        let (tx, rx) = watch::channel(Progress::default());
        publish(
            &tx,
            &ledger,
            &Some(value.clone()),
            None,
            &TodayReadStatus::default(),
        );
        let revision = rx.borrow().revision;
        value.observed_ms += 120_000;
        publish(
            &tx,
            &ledger,
            &Some(value.clone()),
            None,
            &TodayReadStatus::default(),
        );
        assert_eq!(rx.borrow().revision, revision);
        assert_eq!(
            rx.borrow().snapshot.as_ref().unwrap().observed_ms,
            value.observed_ms
        );
        assert!(rx.borrow().snapshot.as_ref().unwrap().eligible(
            value.observed_ms,
            &value.day,
            &value.source
        ));
    }
}

#[cfg(test)]
#[path = "notification_shutdown_tests.rs"]
mod shutdown_tests;

#[cfg(test)]
#[path = "notification_display_tests.rs"]
mod display_tests;
