//! 装配本机扫描协调与 app-data 索引位置的 Tauri 共享状态。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::agent_client::AgentClientCapabilities;
use crate::backend::local_index::ScanCoordinator as LocalScanCoordinator;
use loki_metis_core::{
    AgentClientRegistry, CoverageReport, RootDiscoveryCoordinator, SourceClientKind,
    initial_coverage, source_client_app_data_dir,
};
use tokio::sync::{Mutex, OwnedMutexGuard, RwLock};

mod settings_state;

use crate::dto::{AgentClientKindDto, SourceRootDto};
#[cfg(test)]
use crate::privacy_store::load_settings;
use crate::privacy_store::{LocalPrivacySettings, load_or_initialize_settings};
use crate::scan_state::ScanCoordinator;

/// 保存 GUI 生命周期内唯一的扫描 writer 与本地设置状态。
// 这是整个应用的“全局单例状态”，通过 `app.manage()` 注册进 Tauri，
// 所有 command 函数都以 `tauri::State<AppRuntimeState>` 形式共享同一份实例。
// 字段大量使用 tokio::sync::{Mutex, RwLock}（异步锁，不同于 std::sync 的同步锁，
// 在 .await 点持有也不会阻塞整个线程），因为多个前端请求可能并发调用同一个 command。
pub(crate) struct AppRuntimeState {
    /// 串行化 Codex 主根 registry 变更与本机扫描，避免同一根被并发改写。
    // `Mutex<()>`：不保护任何数据，纯粹当“互斥信号量”用——谁拿到这把锁，
    // 谁就独占执行一段临界区代码。
    codex_account_context_gate: Arc<Mutex<()>>,
    /// 三个本机客户端的索引绑定与扫描器。
    pub(crate) agent_clients: AgentClientRegistry<AgentClientCapabilities>,
    /// 分别维护各客户端的可见扫描状态。
    pub(crate) scans: AgentClientRegistry<Arc<ScanCoordinator>>,
    /// 分客户端防止并发 SQLite writer 的本机索引扫描协调器。
    pub(crate) local_scan: AgentClientRegistry<LocalScanCoordinator>,
    /// 当前全设备元数据发现状态与临时候选；应用退出即丢弃。
    pub(crate) root_discovery: Arc<RootDiscoveryCoordinator>,
    /// 本产品 app-data 目录；从不指向任一 `CODEX_HOME`。
    pub(crate) app_data_dir: PathBuf,
    /// 当前完整本地设置快照。
    privacy_settings: RwLock<LocalPrivacySettings>,
    /// 串行化首次设置读取，避免概览与来源页并发读写设置文件。
    privacy_settings_loaded: Mutex<bool>,
    /// 串行化完整设置文件的更新，防止两个字段互相覆盖。
    privacy_settings_update: Mutex<()>,
    /// 最近一次扫描覆盖结论。
    pub(crate) coverages: AgentClientRegistry<Arc<RwLock<CoverageReport>>>,
    /// 分客户端保存最近一次扫描生成的安全数据根展示记录。
    pub(crate) roots: AgentClientRegistry<Arc<RwLock<Vec<SourceRootDto>>>>,
    /// 分客户端保存最近一次清空本产品索引的时间。
    last_cleared_at_epoch_ms: AgentClientRegistry<RwLock<Option<i64>>>,
}

impl AppRuntimeState {
    /// 使用 Tauri app-data 下的显式索引文件构造共享状态，不立即触发扫描。
    pub(crate) fn new(app_data_dir: PathBuf) -> Self {
        let codex_app_data = source_client_app_data_dir(&app_data_dir, SourceClientKind::Codex);
        let claude_app_data =
            source_client_app_data_dir(&app_data_dir, SourceClientKind::ClaudeCode);
        let grok_app_data =
            source_client_app_data_dir(&app_data_dir, SourceClientKind::GrokBuildCli);
        let codex_account_context_gate = Arc::new(Mutex::new(()));
        let local_scan_coordinator = LocalScanCoordinator::default();
        Self {
            codex_account_context_gate: Arc::clone(&codex_account_context_gate),
            agent_clients: AgentClientRegistry::new(
                AgentClientCapabilities::codex(codex_app_data, codex_account_context_gate),
                AgentClientCapabilities::claude_code(app_data_dir.clone(), claude_app_data),
                AgentClientCapabilities::grok_build_cli(app_data_dir.clone(), grok_app_data),
            ),
            scans: AgentClientRegistry::new(
                Arc::new(ScanCoordinator::default()),
                Arc::new(ScanCoordinator::default()),
                Arc::new(ScanCoordinator::default()),
            ),
            local_scan: AgentClientRegistry::new(
                local_scan_coordinator.clone(),
                local_scan_coordinator.clone(),
                local_scan_coordinator,
            ),
            root_discovery: Arc::new(RootDiscoveryCoordinator::default()),
            app_data_dir,
            privacy_settings: RwLock::new(LocalPrivacySettings::default()),
            privacy_settings_loaded: Mutex::new(false),
            privacy_settings_update: Mutex::new(()),
            coverages: AgentClientRegistry::new(
                Arc::new(RwLock::new(initial_coverage())),
                Arc::new(RwLock::new(initial_coverage())),
                Arc::new(RwLock::new(initial_coverage())),
            ),
            roots: AgentClientRegistry::new(
                Arc::new(RwLock::new(Vec::new())),
                Arc::new(RwLock::new(Vec::new())),
                Arc::new(RwLock::new(Vec::new())),
            ),
            last_cleared_at_epoch_ms: AgentClientRegistry::new(
                RwLock::new(None),
                RwLock::new(None),
                RwLock::new(None),
            ),
        }
    }

    /// 取得 Codex 主根变更与本机扫描共用的独占门禁。
    pub(crate) async fn lock_codex_account_context(&self) -> OwnedMutexGuard<()> {
        Arc::clone(&self.codex_account_context_gate)
            .lock_owned()
            .await
    }

    /// 记录指定客户端索引已经清空的时间，不影响原始文件或登录状态。
    pub(crate) async fn mark_index_cleared(&self, client: AgentClientKindDto) {
        *self
            .last_cleared_at_epoch_ms
            .get(client.into())
            .write()
            .await = Some(now_epoch_ms());
    }

    /// 退出路径只回收本机扫描状态。
    pub(crate) async fn shutdown_application(&self) {}

    /// 首次读取设置时完成旧字段清理；损坏或 I/O 失败时保守回退。
    // “惰性初始化 + 加锁去重”模式：privacy_settings_loaded 是一个
    // Mutex<bool> 标记位，第一次调用时才真正从磁盘读取设置文件，
    // 之后的调用发现标记已是 true 就立刻返回，避免每次 command 都重复 I/O；
    // 用 Mutex 而不是简单的原子 bool，是为了确保并发首次调用只会真正加载一次。
    async fn ensure_privacy_settings_loaded(&self) {
        let mut loaded = self.privacy_settings_loaded.lock().await;
        if *loaded {
            return;
        }
        let app_data_dir = self.app_data_dir.clone();
        let settings = tauri::async_runtime::spawn_blocking(move || {
            load_or_initialize_settings(&app_data_dir)
                .unwrap_or_else(|_| LocalPrivacySettings::safe_fallback())
        })
        .await
        .unwrap_or_else(|_| LocalPrivacySettings::safe_fallback());
        *self.privacy_settings.write().await = settings;
        *loaded = true;
    }

    /// 读取本机索引绑定维护的索引体积。
    async fn index_size_bytes(&self, client: AgentClientKindDto) -> Option<u64> {
        let local_analysis = Arc::clone(&self.agent_clients.get(client.into()).local_analysis);
        local_analysis.index_size_bytes().await
    }
}

/// 按事实自身观测时间计算持久快照有效期，缓存命中不得滑动续期旧事实。
#[cfg(test)]
const fn snapshot_valid_until_epoch_ms(observed_at_epoch_ms: i64, ttl_ms: i64) -> i64 {
    observed_at_epoch_ms.saturating_add(ttl_ms)
}

/// 返回当前 Unix 毫秒时间戳；系统时钟异常时使用零值并让事实降低可解释性。
pub(crate) fn now_epoch_ms() -> i64 {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0_u128, |duration| duration.as_millis());
    i64::try_from(milliseconds).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests;
