//! 本机索引：调用方指定 app-data 目录内单个 SQLite 数据库的持久层。
//!
//! 从 `rusqlite` 迁移到 [`sea_orm`]（SQLite 驱动经 `sqlx`）之后，这一层
//! 挪进 `core`：它只依赖调用方传入的 `Path` 与本模块自身的 schema，
//! 不知道自己被 GUI、CLI 还是 MCP adapter 调用，天然满足 crate 顶部
//! “runtime-neutral”的约束，也让未来其他 adapter 不需要各自重新实现一份
//! SQLite 存取。真正与操作系统数据源格式相关的发现、JSONL 增量解析仍
//! 留在各 adapter 内，只通过 [`checkpoint`] 暴露的最小原语落盘。

mod call_store;
mod checkpoint;
mod error;
mod id_codec;
mod migrator;
mod path_codec;
mod pre_seaorm_schema;
mod root_registry;
mod snapshot;
mod snapshot_ops;
mod source_registry;

use std::path::{Path, PathBuf};

use sea_orm::DatabaseConnection;
use sea_orm_migration::MigratorTrait;

pub use call_store::ClaudeBatchOutcome;
pub use checkpoint::{SourceParseCheckpoint, StoredSourceFile};
pub use error::{LocalError, LocalErrorKind};
pub use id_codec::{path_key, stable_id};
pub use snapshot::UsageSnapshot;
pub use source_registry::{DiscoveryMethod, RegisteredRoot, RootRecord};

use crate::client_ports::USAGE_INDEX_FILE_NAME;
use crate::private_sqlite::{PrivateSqliteError, open_private_sqlite};
use migrator::Migrator;

/// 管理调用方指定 app-data 内的单一物理数据库连接。
// 池上限固定为 1（SeaORM 对 SQLite 后端在未显式设置时的默认值）：
// 效果等价于旧实现里“每次都新开一个 `rusqlite::Connection`”——不同的
// `LocalIndex` 实例（不同 Tauri command 调用）各自持有独立的单连接
// 连接池，靠 `busy_timeout` PRAGMA 而不是应用层显式串行化来处理短暂的
// 跨连接写锁竞争，与本模块一直以来“同一时刻只有一个扫描 writer”的
// 假设一致。
#[derive(Debug)]
pub struct LocalIndex {
    pub(crate) connection: DatabaseConnection,
    /// 当前客户端数据库只接受的 parser generation。
    pub(crate) parser_version: u32,
    #[cfg_attr(not(test), allow(dead_code))]
    database_path: PathBuf,
}

impl LocalIndex {
    /// 在调用方明确提供的 app-data 目录内创建或打开固定名称数据库，
    /// 使用调用方当前构建的 parser generation。
    pub async fn open_in_app_data(
        app_data_dir: &Path,
        parser_version: u32,
    ) -> Result<Self, LocalError> {
        let (connection, database_path) = open_database(app_data_dir).await?;
        Ok(Self {
            connection,
            database_path,
            parser_version,
        })
    }

    /// 返回数据库固定文件名所在位置。目前只有测试用它独立开一个连接核对
    /// 落盘内容；尚无隐私页/清理 UI 消费者，等真的接入那类界面时再放开
    /// 下面的 `allow(dead_code)`。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn database_path(&self) -> &Path {
        &self.database_path
    }
}

/// 在指定 app-data 内打开数据库、应用连接策略并完成迁移。
async fn open_database(app_data_dir: &Path) -> Result<(DatabaseConnection, PathBuf), LocalError> {
    let (connection, database_path) = open_private_sqlite(app_data_dir, USAGE_INDEX_FILE_NAME)
        .await
        .map_err(map_private_sqlite_error)?;

    pre_seaorm_schema::adopt_pre_seaorm_schema_if_present(&connection)
        .await
        .inspect_err(|error| {
            tracing::error!(
                kind = ?error.kind(),
                %error,
                "failed to adopt legacy local index schema"
            );
        })?;
    // 只在失败时记录：迁移成功（包括“已是最新版本、本次是空操作”）
    // 每次打开数据库都会执行一遍，是高频路径，不值得按 info 级别刷屏；
    // 失败则通常意味着数据库损坏或磁盘/权限问题，值得留痕。
    Migrator::up(&connection, None)
        .await
        .map_err(LocalError::from)
        .inspect_err(|error| {
            tracing::error!(
                kind = ?error.kind(),
                %error,
                "failed to apply local index schema migrations"
            );
        })?;

    Ok((connection, database_path))
}

/// 把共用 SQLite 打开错误映射为本机索引稳定错误。
fn map_private_sqlite_error(error: PrivateSqliteError) -> LocalError {
    match error {
        PrivateSqliteError::InvalidPath => {
            LocalError::new(LocalErrorKind::InvalidPath, "app data directory is invalid")
        }
        PrivateSqliteError::PermissionDenied => {
            LocalError::new(LocalErrorKind::PermissionDenied, "local access was denied")
        }
        PrivateSqliteError::StorageUnavailable => LocalError::new(
            LocalErrorKind::SourceUnavailable,
            "local source operation failed",
        ),
        PrivateSqliteError::Database => {
            LocalError::new(LocalErrorKind::Database, "local index operation failed")
        }
    }
}

#[cfg(test)]
mod tests;
