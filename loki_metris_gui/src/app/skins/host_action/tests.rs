//! 用可观察调用轨迹验证确认后编排，不启动或关闭任何真实宿主。
use super::*;
use loki_metris_core::{ColorMode, SkinReference, SkinSource};
use std::{collections::VecDeque, sync::Mutex};

/// 有序快照和安装回执模拟进程竞态，记录真实编排入口及完整授权。
#[derive(Default)]
struct Host {
    /// 按顺序返回的实例快照。
    snapshots: Mutex<VecDeque<Vec<SkinHostInstance>>>,
    /// 按顺序返回的安装结果。
    installed: Mutex<VecDeque<Result<InstallSkinResult, SkinError>>>,
    /// 启动宿主时返回的错误。
    launch_error: Mutex<Option<SkinError>>,
    /// 被调用的编排入口。
    calls: Mutex<Vec<String>>,
    /// 收到的应用意图。
    intents: Mutex<Vec<PendingApply>>,
}
impl ApplyHost for Host {
    /// 每次读取消费一份快照，意外重读直接失败。
    async fn instances(&self, _: SkinHostKind) -> Result<Vec<SkinHostInstance>, SkinError> {
        self.calls.lock().unwrap().push("instances".into());
        Ok(self
            .snapshots
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected instance query"))
    }
    /// 启动可返回迟到的恢复请求，用于模拟启动期间出现其他进程。
    async fn launch(&self, _: SkinHostKind) -> Result<(), SkinError> {
        self.calls.lock().unwrap().push("launch".into());
        self.launch_error.lock().unwrap().take().map_or(Ok(()), Err)
    }
    /// 返回新ID，证明确认后安装不再次依赖旧快照。
    async fn restart(&self, _: SkinHostKind, id: &str) -> Result<SkinHostInstance, SkinError> {
        self.calls.lock().unwrap().push(format!("restart:{id}"));
        Ok(instance("new", CodexRuntimeState::Ready))
    }
    /// 捕获完整意图，空队列返回外观确认以避免构造虚假安装状态。
    async fn install(&self, intent: &PendingApply) -> Result<InstallSkinResult, SkinError> {
        self.calls.lock().unwrap().push("install".into());
        self.intents.lock().unwrap().push(intent.clone());
        self.installed
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Ok(appearance()))
    }
}
/// 只构造本测试使用的宿主身份和运行状态。
fn instance(id: &str, state: CodexRuntimeState) -> SkinHostInstance {
    SkinHostInstance {
        id: id.into(),
        pid: 1,
        label: format!("host {id}"),
        profile: None,
        state,
        debug_port: None,
        active_skin_name: None,
        active_skin: None,
        account_label: None,
        avatar_data_url: None,
    }
}
/// 初次应用没有任何重启或外观授权；纯主题不含第三方代码。
fn intent(host: SkinHostKind) -> PendingApply {
    PendingApply {
        host,
        skin: SkinReference {
            source: SkinSource::User,
            id: "theme".into(),
        },
        instance: None,
        instance_label: String::new(),
        allow_appearance: false,
        allow_workbuddy_recovery: false,
        allow_third_party_code: false,
    }
}
/// 外观差异回执要求后续再次确认，保留同一安装意图。
fn appearance() -> InstallSkinResult {
    InstallSkinResult::NeedsConfirmation {
        check: loki_metris_service::skins::SkinAppearanceCheck {
            effective_mode: ColorMode::Light,
            supported_color_modes: vec![ColorMode::Light],
            differences: vec![],
            unreadable: vec![],
        },
    }
}
/// 测试运行器只驱动异步编排，无实际GUI或系统进程。
fn execute(
    host: &Host,
    intent: PendingApply,
    step: ApplyStep,
    windows: bool,
) -> Result<Outcome, SkinError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(apply_host_action(host, intent, step, windows, || Ok(())))
}
/// Windows多实例恢复确认后只有直接安装，外观确认继续保留恢复和代码权限。
#[test]
fn windows_recovery_and_appearance_preserve_exact_authorizations() {
    let host = Host::default();
    host.snapshots.lock().unwrap().push_back(vec![
        instance("one", CodexRuntimeState::Ready),
        instance("two", CodexRuntimeState::RunningWithoutCdp),
    ]);
    let mut requested = intent(SkinHostKind::WorkBuddy);
    requested.allow_third_party_code = true;
    let Outcome::NeedsRestart(mut pending, mode) =
        execute(&host, requested, ApplyStep::Resolve, true).unwrap()
    else {
        panic!("recovery confirmation missing")
    };
    assert_eq!(mode, RestartMode::RecoverWindowsWorkBuddy);
    assert_eq!(*host.calls.lock().unwrap(), ["instances"]);
    pending.allow_workbuddy_recovery = true;
    pending.instance = None;
    let Outcome::Applied(InstallSkinResult::NeedsConfirmation { .. }, mut pending) =
        execute(&host, pending, ApplyStep::InstallDirect, true).unwrap()
    else {
        panic!("appearance confirmation missing")
    };
    pending.allow_appearance = true;
    execute(&host, pending, ApplyStep::InstallDirect, true).unwrap();
    assert_eq!(
        *host.calls.lock().unwrap(),
        ["instances", "install", "install"]
    );
    let intents = host.intents.lock().unwrap();
    assert!(
        intents.iter().all(|i| i.instance.is_none()
            && i.allow_workbuddy_recovery
            && i.allow_third_party_code)
    );
    assert!(!intents[0].allow_appearance);
    assert!(intents[1].allow_appearance);
}
/// 唯一WorkBuddy即使无CDP也先试装，准确错误才触发新快照和恢复确认。
#[test]
fn sole_workbuddy_attempts_install_before_recovery_confirmation() {
    let host = Host::default();
    host.snapshots.lock().unwrap().extend([
        vec![instance("one", CodexRuntimeState::RunningWithoutCdp)],
        vec![instance("one", CodexRuntimeState::Ready)],
    ]);
    host.installed.lock().unwrap().push_back(Err(SkinError::new(
        "skin.workbuddy_recovery_required",
        "recovery",
    )));
    let Outcome::NeedsRestart(pending, mode) = execute(
        &host,
        intent(SkinHostKind::WorkBuddy),
        ApplyStep::Resolve,
        true,
    )
    .unwrap() else {
        panic!("missing confirmation")
    };
    assert_eq!(mode, RestartMode::RecoverWindowsWorkBuddy);
    assert_eq!(pending.instance.as_deref(), Some("one"));
    assert_eq!(
        *host.calls.lock().unwrap(),
        ["instances", "install", "instances"]
    );
    let i = &host.intents.lock().unwrap()[0];
    assert!(!i.allow_workbuddy_recovery && !i.allow_third_party_code);
}
/// 启动期间并发出现WorkBuddy同样回到确认，不自动关闭进程。
#[test]
fn launch_race_can_request_windows_recovery() {
    let host = Host::default();
    host.snapshots.lock().unwrap().extend([
        vec![],
        vec![instance("one", CodexRuntimeState::RunningWithoutCdp)],
    ]);
    *host.launch_error.lock().unwrap() =
        Some(SkinError::new("skin.workbuddy_recovery_required", "race"));
    assert!(matches!(
        execute(
            &host,
            intent(SkinHostKind::WorkBuddy),
            ApplyStep::Resolve,
            true
        )
        .unwrap(),
        Outcome::NeedsRestart(_, RestartMode::RecoverWindowsWorkBuddy)
    ));
    assert_eq!(
        *host.calls.lock().unwrap(),
        ["instances", "launch", "instances"]
    );
}
/// 非Windows的唯一WorkBuddy恢复请求只允许重启明确实例。
#[test]
fn non_windows_workbuddy_uses_selected_restart() {
    let host = Host::default();
    host.snapshots.lock().unwrap().extend([
        vec![instance("one", CodexRuntimeState::RunningWithoutCdp)],
        vec![instance("one", CodexRuntimeState::RunningWithoutCdp)],
    ]);
    host.installed.lock().unwrap().push_back(Err(SkinError::new(
        "skin.workbuddy_recovery_required",
        "recovery",
    )));
    assert!(matches!(
        execute(
            &host,
            intent(SkinHostKind::WorkBuddy),
            ApplyStep::Resolve,
            false
        )
        .unwrap(),
        Outcome::NeedsRestart(_, RestartMode::RestartSelected)
    ));
}
/// Codex无CDP先确认，重启后只安装服务返回的新身份。
#[test]
fn codex_restart_installs_returned_instance_without_rescanning() {
    let host = Host::default();
    host.snapshots
        .lock()
        .unwrap()
        .push_back(vec![instance("old", CodexRuntimeState::RunningWithoutCdp)]);
    let Outcome::NeedsRestart(pending, RestartMode::RestartSelected) =
        execute(&host, intent(SkinHostKind::Codex), ApplyStep::Resolve, true).unwrap()
    else {
        panic!("restart confirmation missing")
    };
    assert_eq!(pending.instance_label, "host old");
    execute(&host, pending, ApplyStep::RestartSelected, true).unwrap();
    assert_eq!(
        *host.calls.lock().unwrap(),
        ["instances", "restart:old", "install"]
    );
    assert_eq!(
        host.intents.lock().unwrap()[0].instance.as_deref(),
        Some("new")
    );
}
/// 多Codex和非Windows多WorkBuddy均不能静默选择或启动任何实例。
#[test]
fn ambiguous_instances_fail_without_mutation() {
    for (kind, windows) in [
        (SkinHostKind::Codex, true),
        (SkinHostKind::WorkBuddy, false),
    ] {
        let host = Host::default();
        host.snapshots.lock().unwrap().push_back(vec![
            instance("one", CodexRuntimeState::Ready),
            instance("two", CodexRuntimeState::Ready),
        ]);
        assert_eq!(
            execute(&host, intent(kind), ApplyStep::Resolve, windows)
                .err()
                .unwrap()
                .code,
            "skin.host_instance_selection_required"
        );
        assert_eq!(*host.calls.lock().unwrap(), ["instances"]);
    }
}
/// 身份错误与已授权恢复失败不转为新的恢复确认或自动重试。
#[test]
fn unrelated_and_authorized_recovery_errors_propagate() {
    for code in [
        "skin.port_owner_mismatch",
        "skin.workbuddy_recovery_unstable",
        "skin.workbuddy_recovery_cancelled_after_close",
    ] {
        let host = Host::default();
        host.installed
            .lock()
            .unwrap()
            .push_back(Err(SkinError::new(code, "failure")));
        let mut pending = intent(SkinHostKind::WorkBuddy);
        pending.allow_workbuddy_recovery = true;
        assert_eq!(
            execute(&host, pending, ApplyStep::InstallDirect, true)
                .err()
                .unwrap()
                .code,
            code
        );
        assert_eq!(*host.calls.lock().unwrap(), ["install"]);
    }
}

/// 外观继续或明确重启后的恢复请求直接报错，不能再次弹窗形成确认循环。
#[test]
fn confirmed_steps_do_not_reenter_recovery_confirmation() {
    for step in [ApplyStep::InstallDirect, ApplyStep::RestartSelected] {
        let host=Host::default();
        host.installed.lock().unwrap().push_back(Err(SkinError::new("skin.workbuddy_recovery_required","late failure")));
        let mut pending=intent(SkinHostKind::WorkBuddy);pending.instance=Some("old".into());pending.allow_appearance=true;
        assert_eq!(execute(&host,pending,step,false).err().unwrap().code,"skin.workbuddy_recovery_required");
        assert!(!host.calls.lock().unwrap().iter().any(|call|call=="instances"));
    }
}

/// 构造真实安装回执的最小状态，避免把确认或目录操作当作更换成功。
fn installed() -> InstallSkinResult {
    InstallSkinResult::Installed { status: loki_metris_service::skins::SkinStatus {
        installed: true, skin_id: Some("theme".into()), source: Some(SkinSource::User),
        package_type: Some(loki_metris_core::SkinPackageType::Theme), skin_name: Some("Theme".into()),
        version: "1.0.0".into(), affected_pages: 1, compatibility: None, instance_id: Some("one".into()),
    } }
}
/// 成功与失败均形成通知，取消、重启/外观确认和其它资源操作保持静默。
#[test]
fn skin_notification_result_classifies_actual_terminal_outcomes() {
    assert_eq!(notification_result(&Ok(Outcome::Applied(installed(), intent(SkinHostKind::Codex)))), Some(true));
    assert_eq!(notification_result(&Err(SkinError::new("skin.install_failed", "failure"))), Some(false));
    for code in ["skin.operation_cancelled", "skin.workbuddy_recovery_cancelled_after_close"] {
        assert_eq!(notification_result(&Err(SkinError::new(code, "cancelled"))), None);
    }
    assert_eq!(notification_result(&Ok(Outcome::Applied(appearance(), intent(SkinHostKind::Codex)))), None);
    assert_eq!(notification_result(&Ok(Outcome::NeedsRestart(intent(SkinHostKind::Codex), RestartMode::RestartSelected))), None);
    assert_eq!(notification_result(&Ok(Outcome::Stopped)), None);
}
/// 安装完成后即使页面授权失效仍保留真实成功，界面自行拒收旧投影。
#[test]
fn skin_notification_keeps_committed_success_after_page_authority_changes() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let host = Host::default();
    host.installed.lock().unwrap().push_back(Ok(installed()));
    let checks = AtomicUsize::new(0);
    let result = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(
        apply_host_action(&host, intent(SkinHostKind::Codex), ApplyStep::InstallDirect, false, || {
            if checks.fetch_add(1, Ordering::SeqCst) < 2 { Ok(()) }
            else { Err(SkinError::new("skin.operation_cancelled", "page closed")) }
        })
    );
    assert_eq!(notification_result(&result), Some(true));
    assert_eq!(*host.calls.lock().unwrap(), ["install"]);
}
