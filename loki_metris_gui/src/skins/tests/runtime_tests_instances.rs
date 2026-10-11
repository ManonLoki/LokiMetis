// 本文件是 runtime.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：验证换皮迁移中的 `codex_instance_serializes_avatar_and_exact_active_skin` 回归场景。；Windows WorkBuddy 恢复后，新 PID 安装必须回收该宿主全部旧监视任务并保留 Codex 状态。；枚举结果不再包含旧 PID 时，应删除已结束任务，但仍保留尚在清理中的任务。

#[test]
/// 验证换皮迁移中的 `codex_instance_serializes_avatar_and_exact_active_skin` 回归场景。
fn codex_instance_serializes_avatar_and_exact_active_skin() -> Result<(), serde_json::Error> {
    let value = serde_json::to_value(SkinHostInstance {
        id: "instance-1".into(),
        pid: 42,
        label: "Codex · company".into(),
        profile: Some("company".into()),
        state: CodexRuntimeState::Ready,
        debug_port: Some(9341),
        active_skin_name: Some("共享名称".into()),
        active_skin: Some(SkinReference {
            source: SkinSource::User,
            id: "exact-theme".into(),
        }),
        account_label: Some("公司账号".into()),
        avatar_data_url: Some("data:image/png;base64,AQID".into()),
    })?;
    assert_eq!(
        value["activeSkin"],
        json!({ "source": "user", "id": "exact-theme" })
    );
    assert_eq!(value["avatarDataUrl"], "data:image/png;base64,AQID");
    Ok(())
}

#[cfg(target_os = "windows")]
#[tokio::test]
/// Windows WorkBuddy 恢复后，新 PID 安装必须回收该宿主全部旧监视任务并保留 Codex 状态。
async fn windows_workbuddy_install_replaces_old_pid_watchers() {
    let watch_task = |host| {
        let (cancel, mut cancelled) = tokio::sync::watch::channel(false);
        let join = tokio::spawn(async move {
            let _ = cancelled.changed().await;
            Ok::<usize, super::AppError>(0)
        });
        let handler_abort = join.abort_handle();
        super::WatchTask::new_test(
            host,
            cancel,
            join,
            handler_abort,
            super::CdpEndpoint::default(),
        )
    };
    let mut runtime = super::RuntimeState::default();
    for key in ["workBuddy:old-pid", "workBuddy:new-pid"] {
        runtime.instances.insert(
            key.into(),
            super::InstanceRuntime {
                active: None,
                compatibility: None,
                task: Some(watch_task(super::SkinHostKind::WorkBuddy)),
            },
        );
    }
    runtime.instances.insert(
        "codex:preserved".into(),
        super::InstanceRuntime {
            active: None,
            compatibility: None,
            task: None,
        },
    );

    let tasks = super::take_replaced_watch_tasks(
        &mut runtime,
        super::SkinHostKind::WorkBuddy,
        "workBuddy:new-pid",
    );

    assert_eq!(tasks.len(), 2);
    assert!(
        runtime
            .instances
            .keys()
            .all(|key| !super::runtime_instance_belongs_to_host(
                super::SkinHostKind::WorkBuddy,
                key,
            ))
    );
    assert!(runtime.instances.contains_key("codex:preserved"));
    for task in tasks {
        task.abort();
    }

    runtime.instances.insert(
        "workBuddy:stale-last-target".into(),
        super::InstanceRuntime {
            active: None,
            compatibility: None,
            task: Some(watch_task(super::SkinHostKind::WorkBuddy)),
        },
    );
    runtime.last_targets.insert(
        super::SkinHostKind::WorkBuddy,
        "workBuddy:stale-last-target".into(),
    );
    let uninstall_tasks = super::take_uninstall_watch_tasks(
        &mut runtime,
        super::SkinHostKind::WorkBuddy,
        Some("workBuddy:current-pid"),
    );
    assert_eq!(uninstall_tasks.len(), 1);
    assert!(
        !runtime
            .last_targets
            .contains_key(&super::SkinHostKind::WorkBuddy)
    );
    assert!(
        runtime
            .instances
            .keys()
            .all(|key| !super::runtime_instance_belongs_to_host(
                super::SkinHostKind::WorkBuddy,
                key,
            ))
    );
    for task in uninstall_tasks {
        task.abort();
    }
}

#[tokio::test]
/// 枚举结果不再包含旧 PID 时，应删除已结束任务，但仍保留尚在清理中的任务。
async fn recovered_runtime_prunes_finished_old_pid_watcher() {
    let root = temp_directory("prune-finished-workbuddy-watcher");
    let service = SkinService::new(root.join("builtin"), root.join("user"));
    let (finished_cancel, _) = tokio::sync::watch::channel(false);
    let finished_join = tokio::spawn(async { Ok::<usize, super::AppError>(0) });
    while !finished_join.is_finished() {
        tokio::task::yield_now().await;
    }
    let finished_abort = finished_join.abort_handle();
    let (running_cancel, mut running_cancelled) = tokio::sync::watch::channel(false);
    let running_join = tokio::spawn(async move {
        let _ = running_cancelled.changed().await;
        Ok::<usize, super::AppError>(0)
    });
    let running_abort = running_join.abort_handle();
    {
        let mut runtime = service.runtime.lock().await;
        runtime.instances.insert(
            "workBuddy:finished-old-pid".into(),
            super::InstanceRuntime {
                active: None,
                compatibility: None,
                task: Some(super::WatchTask::new_test(
                    super::SkinHostKind::WorkBuddy,
                    finished_cancel,
                    finished_join,
                    finished_abort,
                    super::CdpEndpoint::default(),
                )),
            },
        );
        runtime.instances.insert(
            "workBuddy:running-old-pid".into(),
            super::InstanceRuntime {
                active: None,
                compatibility: None,
                task: Some(super::WatchTask::new_test(
                    super::SkinHostKind::WorkBuddy,
                    running_cancel,
                    running_join,
                    running_abort,
                    super::CdpEndpoint::default(),
                )),
            },
        );
    }

    service
        .retain_recovered_instance_runtimes(
            super::SkinHostKind::WorkBuddy,
            std::iter::empty::<&str>(),
        )
        .await;

    let running_task = {
        let mut runtime = service.runtime.lock().await;
        assert!(!runtime.instances.contains_key("workBuddy:finished-old-pid"));
        runtime
            .instances
            .remove("workBuddy:running-old-pid")
            .and_then(|instance| instance.task)
            .expect("尚未结束的监视任务必须保留")
    };
    running_task.abort();
    std::fs::remove_dir_all(root).expect("应清理测试目录");
}

#[tokio::test]
/// 跨越一次显式宿主变更才返回的旧页面探针，不得重新写回已经失效的活动皮肤。
async fn recovered_runtime_rejects_probe_from_previous_generation() {
    let root = temp_directory("reject-stale-runtime-probe");
    create_fixture(&root.join("user/old-skin"), "old-skin");
    let service = SkinService::new(root.join("builtin"), root.join("user"));
    let descriptor = load_descriptor(&root.join("user/old-skin"), "old-skin", SkinSource::User)
        .expect("应读取旧皮肤测试描述");
    let observed_generation = service.host_runtime_generation(SkinHostKind::WorkBuddy);
    let mutation = service.begin_host_runtime_mutation(SkinHostKind::WorkBuddy);
    drop(mutation);

    service
        .reconcile_recovered_instance_runtime(
            SkinHostKind::WorkBuddy,
            "workbuddy-current",
            Some(descriptor),
            observed_generation,
        )
        .await;

    assert!(service.runtime.lock().await.instances.is_empty());
    std::fs::remove_dir_all(root).expect("应清理测试目录");
}

#[tokio::test]
/// 当前 PID 的监视任务已结束且页面已无皮肤时，页面探针必须清掉旧活动状态。
async fn recovered_runtime_none_clears_finished_current_pid_watcher() {
    let root = temp_directory("clear-finished-current-runtime");
    create_fixture(&root.join("user/old-skin"), "old-skin");
    let service = SkinService::new(root.join("builtin"), root.join("user"));
    let descriptor = load_descriptor(&root.join("user/old-skin"), "old-skin", SkinSource::User)
        .expect("应读取旧皮肤测试描述");
    let (cancel, _) = tokio::sync::watch::channel(false);
    let join = tokio::spawn(async { Ok::<usize, super::AppError>(0) });
    while !join.is_finished() {
        tokio::task::yield_now().await;
    }
    let handler_abort = join.abort_handle();
    let key = super::runtime_instance_key(SkinHostKind::WorkBuddy, "workbuddy-current");
    {
        let mut runtime = service.runtime.lock().await;
        runtime.instances.insert(
            key.clone(),
            super::InstanceRuntime {
                active: Some(descriptor),
                compatibility: None,
                task: Some(super::WatchTask::new_test(
                    SkinHostKind::WorkBuddy,
                    cancel,
                    join,
                    handler_abort,
                    super::CdpEndpoint::default(),
                )),
            },
        );
    }
    let observed_generation = service.host_runtime_generation(SkinHostKind::WorkBuddy);

    service
        .reconcile_recovered_instance_runtime(
            SkinHostKind::WorkBuddy,
            "workbuddy-current",
            None,
            observed_generation,
        )
        .await;

    assert!(!service.runtime.lock().await.instances.contains_key(&key));
    std::fs::remove_dir_all(root).expect("应清理测试目录");
}
