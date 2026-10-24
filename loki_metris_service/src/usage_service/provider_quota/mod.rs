//! 提供商用量的后台读取与快照。窗口线程只复制结果，不读登录文件、不发网络。

pub(super) mod credentials;
pub(super) mod fetch;
mod local;
pub(super) mod transport;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod cursor_session_tests;

use std::{sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

use loki_metris_core::{ProviderQuota, QuotaFailure, QuotaProvider, supported_quota_providers};

use super::background::{BackgroundTaskOwner, BackgroundTaskShutdown};

/// 一张卡的读取阶段。刷新期间保留上一阶段，不先清空成等待。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuotaCardPhase {
    /// 还没有完成过读取。
    Pending,
    /// 至少有一个 5 小时、周或月窗口。
    Ready(ProviderQuota),
    /// 已连通，但上游没有目标窗口。
    NoWindow { plan: Option<String> },
    /// 稳定失败类别。
    Failed(QuotaFailure),
}

/// 一个提供商的展示卡。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuotaCard {
    /// 提供商。
    pub provider: QuotaProvider,
    /// 当前阶段。
    pub phase: QuotaCardPhase,
}

/// 活动提供商的最近快照，失败和缺窗口阶段保留在后台供下次刷新。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuotaBoardSnapshot {
    /// 与 [`supported_quota_providers`] 相同的顺序。
    pub cards: [QuotaCard; supported_quota_providers().len()],
    /// 最近一次写回某张卡的 Unix 秒。
    pub fetched_at_unix: Option<i64>,
}

impl Default for QuotaBoardSnapshot {
    /// 按活动提供商顺序建立尚未读取的卡片，不伪造观测时间。
    fn default() -> Self {
        Self {
            cards: supported_quota_providers().map(|provider| QuotaCard { provider, phase: QuotaCardPhase::Pending }),
            fetched_at_unix: None,
        }
    }
}

impl QuotaBoardSnapshot {
    /// 只把成功解析的非敏感额度交给core统一筛选、排序并形成终端投影。
    pub(crate) fn for_terminal(&self) -> Option<loki_metris_core::ProviderQuotaSnapshot> {
        let quotas: Vec<_> = self.cards.iter().filter_map(|card| match &card.phase {
            QuotaCardPhase::Ready(quota) => Some(quota.clone()), _ => None,
        }).collect();
        loki_metris_core::ProviderQuotaSnapshot::project(self.fetched_at_unix, &quotas)
    }
}

/// 进程内唯一的用量板。测试构建的 kick 不访问本机登录态。
pub(crate) struct QuotaBoard {
    snapshot: Mutex<QuotaBoardSnapshot>,
    in_flight: AtomicBool,
    last_kick: Mutex<Option<Instant>>,
}

impl QuotaBoard {
    /// 按活动目录建立等待中的卡。
    pub(crate) fn new() -> Self {
        Self { snapshot: Mutex::new(QuotaBoardSnapshot::default()), in_flight: AtomicBool::new(false), last_kick: Mutex::new(None) }
    }

    /// 复制当前快照。
    pub(crate) fn snapshot(&self) -> QuotaBoardSnapshot {
        self.snapshot.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    /// 请求后台刷新；非 force 请求受五分钟节流，进行中的读取始终合并。
    pub(crate) fn kick(self: &Arc<Self>, force: bool, background: &BackgroundTaskOwner) -> bool {
        // 测试构建（含 GUI 测试启用的 test-support）不发起真实读取。
        if cfg!(any(test, feature = "test-support")) {
            let _ = (force, background);
            return true;
        }
        self.kick_live(force, background)
    }

    /// 合并在途刷新，对非 force 请求执行五分钟节流，并将实际读取登记到应用后台 owner。
    #[cfg_attr(test, allow(dead_code))]
    fn kick_live(self: &Arc<Self>, force: bool, background: &BackgroundTaskOwner) -> bool {
        if !force && self.last_kick.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_some_and(|at| at.elapsed() < Duration::from_secs(5 * 60)) {
            return true;
        }
        if self.in_flight.swap(true, Ordering::AcqRel) {
            return true;
        }
        *self.last_kick.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Instant::now());
        let board = Arc::clone(self);
        let spawned = background.spawn("provider-quota", move |shutdown| async move {
            let _guard = FlightGuard(&board.in_flight);
            board.refresh_all(shutdown).await;
        });
        if spawned.is_err() {
            self.in_flight.store(false, Ordering::Release);
            *self.last_kick.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
            return false;
        }
        true
    }

    /// 按活动目录顺序读取各提供商，复用同一取消信号与受限外部程序配置。
    async fn refresh_all(&self, shutdown: BackgroundTaskShutdown) {
        let mut cancel = shutdown.receiver();
        let Some(home) = loki_metris_host::local_index::current_user_home() else {
            self.fail_all(QuotaFailure::MissingCredentials);
            return;
        };
        let layout = credentials::HomeLayout::from_process(home);
        let curl = transport::find_program(if cfg!(windows) { "curl.exe" } else { "curl" });
        let sqlite = transport::find_program(if cfg!(windows) { "sqlite3.exe" } else { "sqlite3" });
        let limits = transport::CurlLimits::production();
        let now_unix = unix_now();
        let range = fetch::local_package_range();
        for provider in supported_quota_providers() {
            if *cancel.borrow() {
                break;
            }
            let phase = fetch::fetch_provider(provider, &layout, curl.as_deref(), sqlite.as_deref(), limits, now_unix, &range, &mut cancel).await;
            self.publish(provider, phase);
        }
    }

    /// 在短锁内替换单个提供商阶段，并记录真实写回时间。
    fn publish(&self, provider: QuotaProvider, phase: QuotaCardPhase) {
        let mut snapshot = self.snapshot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(card) = snapshot.cards.iter_mut().find(|card| card.provider == provider) {
            card.phase = phase;
        }
        snapshot.fetched_at_unix = Some(unix_now());
    }

    /// 将无法建立读取环境的失败统一写入全部活动卡片。
    fn fail_all(&self, failure: QuotaFailure) {
        for provider in supported_quota_providers() {
            self.publish(provider, QuotaCardPhase::Failed(failure));
        }
    }
}

/// 读取当前 Unix 秒，为额度快照记录完成时间。
fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_secs() as i64).unwrap_or(0)
}

/// 持有本轮刷新的在途标记，保证正常结束或取消后都能重新刷新。
struct FlightGuard<'a>(&'a AtomicBool);

impl Drop for FlightGuard<'_> {
    /// 释放在途标记，使后续刷新请求重新取得执行资格。
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
