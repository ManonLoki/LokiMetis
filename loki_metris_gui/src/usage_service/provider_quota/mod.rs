//! 提供商用量的后台读取与快照。窗口线程只复制结果，不读登录文件、不发网络。

mod credentials;
mod fetch;
mod local;
mod transport;
#[cfg(test)]
mod tests;

use std::{sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

use loki_metris_core::{ProviderQuota, QuotaFailure, QuotaProvider, supported_quota_providers};

use super::background::{BackgroundTaskOwner, BackgroundTaskShutdown};

/// 一张卡的读取阶段。刷新期间保留上一阶段，不先清空成等待。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum QuotaCardPhase {
    /// 还没有完成过读取。
    Pending,
    /// 至少有一个 5 小时或周窗口。
    Ready(ProviderQuota),
    /// 已连通，但上游没有这两种窗口。
    NoWindow { plan: Option<String> },
    /// 稳定失败类别。
    Failed(QuotaFailure),
}

/// 一个提供商的展示卡。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QuotaCard {
    /// 提供商。
    pub provider: QuotaProvider,
    /// 当前阶段。
    pub phase: QuotaCardPhase,
}

/// 活动提供商的最近快照，失败和缺窗口阶段保留在后台供下次刷新。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QuotaBoardSnapshot {
    /// 与 [`supported_quota_providers`] 相同的顺序。
    pub cards: [QuotaCard; supported_quota_providers().len()],
    /// 最近一次写回某张卡的 Unix 秒。
    pub fetched_at_unix: Option<i64>,
}

impl Default for QuotaBoardSnapshot {
    fn default() -> Self {
        Self {
            cards: supported_quota_providers().map(|provider| QuotaCard { provider, phase: QuotaCardPhase::Pending }),
            fetched_at_unix: None,
        }
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

    /// 请求后台刷新。五分钟内的重复请求和进行中的读取都会跳过。
    pub(crate) fn kick(self: &Arc<Self>, force: bool, background: &BackgroundTaskOwner) -> bool {
        if cfg!(test) {
            let _ = (force, background);
            return true;
        }
        self.kick_live(force, background)
    }

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

    async fn refresh_all(&self, shutdown: BackgroundTaskShutdown) {
        let mut cancel = shutdown.receiver();
        let Some(home) = crate::backend::local_index::current_user_home() else {
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

    fn publish(&self, provider: QuotaProvider, phase: QuotaCardPhase) {
        let mut snapshot = self.snapshot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(card) = snapshot.cards.iter_mut().find(|card| card.provider == provider) {
            card.phase = phase;
        }
        snapshot.fetched_at_unix = Some(unix_now());
    }

    fn fail_all(&self, failure: QuotaFailure) {
        for provider in supported_quota_providers() {
            self.publish(provider, QuotaCardPhase::Failed(failure));
        }
    }
}

fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_secs() as i64).unwrap_or(0)
}

struct FlightGuard<'a>(&'a AtomicBool);

impl Drop for FlightGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
