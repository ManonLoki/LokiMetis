//! 今日摘要的索引侧结果缓存：索引文件、覆盖与民用日都未变时，不再重读并重算全部调用。
//!
//! 每 30 秒的后台摘要原先都会读出保留期内的全部调用、重新命名空间化并聚合六个窗口。
//! 索引只会被本进程扫描事务写入，因此以索引主文件、`-wal` 文件的文件戳作为"内容未变"
//! 的判据；`-journal` 存在表示写事务正在进行，此时不使用也不保存缓存。读取前后文件戳
//! 必须一致才保存，避免把并发写入中途读到的数据当作稳定结果。
//!
//! 缓存只保存窗口汇总（几十个标量事实），不保存调用本身；命中时只刷新观测时间，
//! WorkBuddy 合并、readiness 与来源身份仍由 `today::load` 每轮重新计算。

use std::sync::{Arc, Mutex};

use loki_metris_core::{
    CoverageReport, LocalRecordsSummary, SourceClientKind, UsageDay, source_client_usage_index_path,
};

use super::{ServiceContext, query};
use crate::backend::file_stamp::FileStamp;

/// 一个 Agent 索引的文件戳：主库、预写日志与回滚日志。
type IndexFiles = [Option<FileStamp>; 3];

/// 汇总口径：联合"全部"与单个物理 Agent 的窗口 provider 不同，不能共用条目。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Scope {
    /// “全部”联合口径。
    Combined,
    /// 单个物理 Agent 口径。
    Single,
}

/// 缓存键：同一民用日内，所选 Agent 的索引文件与覆盖报告都相同。
#[derive(Clone, PartialEq, Eq)]
pub(super) struct IndexKey {
    /// 汇总口径。
    scope: Scope,
    /// 所属民用日。
    day: UsageDay,
    /// 所选 Agent 的索引文件标识与覆盖报告。
    clients: Vec<(SourceClientKind, IndexFiles, CoverageReport)>,
}

/// 命中后可直接复用的索引侧产物，`signature` 与 `all_ready` 取自当时读到的根与索引状态。
pub(super) struct Cached {
    /// 命中条件：口径、民用日与各 Agent 的索引文件和覆盖报告。
    key: IndexKey,
    /// 读取时各 Agent 的 parser 版本与各数据根“ID:启用”组成的签名，用于判断来源是否变化。
    pub(super) signature: Vec<String>,
    /// 读取时所选 Agent 的索引是否全部就绪。
    pub(super) all_ready: bool,
    /// 缓存的当日窗口摘要。
    pub(super) records: LocalRecordsSummary,
}

/// 条目数硬上限：联合口径一份，加每个物理 Agent 一份，不为历史键组合保留内容。
const MAX_ENTRIES: usize = 5;

/// 按键保存最近的索引侧汇总；同一"口径 + Agent 集合"只保留最新一份。
#[derive(Default)]
pub(super) struct TodayCache(Mutex<Vec<Arc<Cached>>>);

impl TodayCache {
    /// 键相同才返回缓存；任何不一致都按未命中处理。
    pub(super) fn lookup(&self, key: &IndexKey) -> Option<Arc<Cached>> {
        // 仅测量构建：关闭缓存做受控 A/B，用于归因缓存对 CPU 与内存的贡献。
        #[cfg(feature = "isolated-measurement")]
        if std::env::var_os("LOKI_METRIS_DISABLE_INDEX_CACHE").is_some() {
            return None;
        }
        let entries = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        entries.iter().find(|cached| cached.key == *key).cloned()
    }

    /// 测试用：缓存当前是否已保存过产物。
    #[cfg(test)]
    pub(super) fn is_populated(&self) -> bool {
        !self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_empty()
    }

    /// 以读取前取得的键保存本轮产物，调用方负责确认读取期间索引未变化。
    pub(super) fn store(
        &self,
        key: IndexKey,
        signature: Vec<String>,
        all_ready: bool,
        records: LocalRecordsSummary,
    ) {
        let cached = Arc::new(Cached { key, signature, all_ready, records });
        let mut entries = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let same_slot = |other: &Arc<Cached>| {
            other.key.scope == cached.key.scope
                && other.key.clients.iter().map(|c| c.0).eq(cached.key.clients.iter().map(|c| c.0))
        };
        entries.retain(|other| !same_slot(other));
        if entries.len() >= MAX_ENTRIES {
            entries.remove(0);
        }
        entries.push(cached);
    }
}

/// 读取某个 Agent 索引的文件戳；主库缺失本身是稳定状态（读取返回未扫描），
/// 回滚日志存在或任何 I/O 错误都返回 `None`。
fn index_files(directory: &std::path::Path, client: SourceClientKind) -> Option<IndexFiles> {
    let main = source_client_usage_index_path(directory, client);
    let sibling = |suffix: &str| {
        let mut name = main.clone().into_os_string();
        name.push(suffix);
        FileStamp::of_path(std::path::Path::new(&name))
    };
    let main_stamp = FileStamp::of_path(&main).ok()?;
    let wal = sibling("-wal").ok()?;
    let journal = sibling("-journal").ok()?;
    if journal.is_some() {
        return None;
    }
    Some([main_stamp, wal, journal])
}

/// 在 blocking 线程取得键；覆盖报告来自内存，与文件戳一同决定缓存是否仍有效。
pub(super) async fn key_for(
    context: &Arc<ServiceContext>,
    scope: Scope,
    clients: &[SourceClientKind],
    day: &UsageDay,
) -> Option<IndexKey> {
    let directory = context.directory.clone();
    let owned: Vec<_> = clients.to_vec();
    let files = tokio::task::spawn_blocking(move || {
        owned
            .into_iter()
            .map(|client| index_files(&directory, client).map(|files| (client, files)))
            .collect::<Option<Vec<_>>>()
    })
    .await
    .ok()??;
    Some(IndexKey {
        scope,
        day: day.clone(),
        clients: files
            .into_iter()
            .map(|(client, files)| (client, files, query::coverage(context, client)))
            .collect(),
    })
}

/// 命中时窗口事实沿用旧数值，但观测时间必须是本轮时刻。
pub(super) fn restamp(records: &mut LocalRecordsSummary, now: i64) {
    for window in &mut records.windows {
        window.fact.observed_at_epoch_ms = now;
    }
}
