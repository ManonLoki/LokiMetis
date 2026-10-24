//! 宿主编排保留确认授权和稳定实例身份；实际进程/CDP事务仍由SkinService负责。

use super::state::{Outcome, PendingApply, RestartMode};
use loki_metris_service::skins::{
    CodexRuntimeState, InstallSkinResult, SkinError, SkinHostInstance, SkinService,
};
use loki_metris_core::SkinHostKind;
use std::future::Future;

/// 用户动作进入三种独立阶段，确认后的直接安装不再次启动或选择宿主。
#[derive(Clone, Copy)]
pub(super) enum ApplyStep {
    /// 解析目标实例并检查外观。
    Resolve,
    /// 确认后重启选中的实例再应用。
    RestartSelected,
    /// 确认后直接安装，不再启动或选择宿主。
    InstallDirect,
}

/// 狭窄宿主端口便于验证编排合同，生产实现仍只调用真实服务。
pub(super) trait ApplyHost: Sync {
    /// 读取经过身份验证的实例快照。
    fn instances(
        &self,
        host: SkinHostKind,
    ) -> impl Future<Output = Result<Vec<SkinHostInstance>, SkinError>> + Send;
    /// 启动当前没有运行实例的宿主。
    fn launch(&self, host: SkinHostKind) -> impl Future<Output = Result<(), SkinError>> + Send;
    /// 重启明确确认的实例并返回新的稳定身份。
    fn restart(
        &self,
        host: SkinHostKind,
        id: &str,
    ) -> impl Future<Output = Result<SkinHostInstance, SkinError>> + Send;
    /// 原样传递外观、代码和整宿主恢复授权。
    fn install(
        &self,
        intent: &PendingApply,
    ) -> impl Future<Output = Result<InstallSkinResult, SkinError>> + Send;
}

impl ApplyHost for SkinService {
    /// 实例发现由宿主服务负责，不读取GUI缓存作为进程身份。
    async fn instances(&self, host: SkinHostKind) -> Result<Vec<SkinHostInstance>, SkinError> {
        self.scanned_host_instances(host).await
    }
    /// 使用受控启动入口，由同一服务拥有调试端口和退出收尾。
    async fn launch(&self, host: SkinHostKind) -> Result<(), SkinError> {
        self.launch_host(host).await.map(|_| ())
    }
    /// 服务负责验证所选实例，返回值直接用于后续安装。
    async fn restart(&self, host: SkinHostKind, id: &str) -> Result<SkinHostInstance, SkinError> {
        self.restart_host_instance(host, id).await
    }
    /// 确认后的安装不改变调用者授予的权限集合。
    async fn install(&self, intent: &PendingApply) -> Result<InstallSkinResult, SkinError> {
        self.install(
            intent.host,
            &intent.skin,
            intent.allow_appearance,
            intent.instance.as_deref(),
            intent.allow_workbuddy_recovery,
            intent.allow_third_party_code,
        )
        .await
    }
}

/// 只有准确恢复请求可变为确认；端口、进程身份和恢复失败均直接返回。
pub(super) async fn apply_host_action(
    service: &impl ApplyHost,
    intent: PendingApply,
    step: ApplyStep,
    windows: bool,
    valid: impl Fn() -> Result<(), SkinError> + Send + Sync,
) -> Result<Outcome, SkinError> {
    let result = run(service, intent.clone(), step, windows, &valid).await;
    match result {
        Err(error)
            if matches!(step, ApplyStep::Resolve)
                && intent.host == SkinHostKind::WorkBuddy
                && !intent.allow_workbuddy_recovery
                && error.code == "skin.workbuddy_recovery_required" =>
        {
            valid()?;
            let instances = service.instances(intent.host).await?;
            valid()?;
            restart_request(intent, &instances, windows)
        }
        other => other,
    }
}

/// 每次异步读回后核对页面授权；进入安装事务后补偿由服务自行完成。
async fn run(
    service: &impl ApplyHost,
    mut intent: PendingApply,
    step: ApplyStep,
    windows: bool,
    valid: &(impl Fn() -> Result<(), SkinError> + Send + Sync),
) -> Result<Outcome, SkinError> {
    valid()?;
    match step {
        ApplyStep::Resolve => {
            let mut instances = service.instances(intent.host).await?;
            valid()?;
            if instances.is_empty() {
                service.launch(intent.host).await?;
                valid()?;
                instances = service.instances(intent.host).await?;
                valid()?;
            }
            if instances.is_empty() {
                return Err(choose_instance());
            }
            if instances.len() != 1 {
                return restart_request(intent, &instances, windows);
            }
            let target = &instances[0];
            intent.instance = Some(target.id.clone());
            intent.instance_label = target.label.clone();
            if target.state == CodexRuntimeState::RunningWithoutCdp
                && intent.host != SkinHostKind::WorkBuddy
            {
                return Ok(Outcome::NeedsRestart(intent, RestartMode::RestartSelected));
            }
        }
        ApplyStep::RestartSelected => {
            let id = intent.instance.as_deref().ok_or_else(choose_instance)?;
            let target = service.restart(intent.host, id).await?;
            valid()?;
            intent.instance = Some(target.id);
            intent.instance_label = target.label;
        }
        ApplyStep::InstallDirect => {}
    }
    valid()?;
    let installed = service.install(&intent).await?;
    // 已提交事务保留真实成功；只有尚需确认的新副作用继续核对页面授权。
    if matches!(installed, InstallSkinResult::NeedsConfirmation { .. }) { valid()?; }
    Ok(Outcome::Applied(installed, intent))
}

/// 只有实际安装或真实失败通知；中间确认和用户取消不产生结果通知。
pub(super) fn notification_result(result: &Result<Outcome, SkinError>) -> Option<bool> {
    match result {
        Ok(Outcome::Applied(InstallSkinResult::Installed { .. }, _)) => Some(true),
        Err(error) if !matches!(error.code, "skin.operation_cancelled" | "skin.workbuddy_recovery_cancelled_after_close") => Some(false),
        _ => None,
    }
}

/// Windows WorkBuddy恢复允许无唯一目标；其他系统必须有明确实例供重启。
fn restart_request(
    mut intent: PendingApply,
    instances: &[SkinHostInstance],
    windows: bool,
) -> Result<Outcome, SkinError> {
    let target = if instances.len() == 1 {
        instances.first()
    } else {
        None
    };
    if intent.host == SkinHostKind::WorkBuddy && windows {
        intent.instance = target.map(|target| target.id.clone());
        intent.instance_label = target
            .map(|target| target.label.clone())
            .unwrap_or_default();
        return Ok(Outcome::NeedsRestart(
            intent,
            RestartMode::RecoverWindowsWorkBuddy,
        ));
    }
    let target = target.ok_or_else(choose_instance)?;
    intent.instance = Some(target.id.clone());
    intent.instance_label = target.label.clone();
    Ok(Outcome::NeedsRestart(intent, RestartMode::RestartSelected))
}

/// 无唯一身份时禁止自动命中任意进程。
fn choose_instance() -> SkinError {
    SkinError::new(
        "skin.host_instance_selection_required",
        "请选择唯一宿主实例，关闭多余实例后刷新。",
    )
}

#[cfg(test)]
mod tests;
