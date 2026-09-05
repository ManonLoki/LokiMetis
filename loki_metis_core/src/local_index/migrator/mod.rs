//! 本机索引 schema 的 SeaORM 迁移集合，按引入顺序单调追加。

mod m1_initial;
mod m2_call_availability_flags;
mod m3_adapter_consistency_key;
mod m4_primary_root;
mod m5_display_labels;
mod m6_root_activation;
mod m7_token_snapshots;
mod m8_adapter_parse_state;

use sea_orm_migration::prelude::*;

/// 本机索引 schema 的迁移入口，供 [`super::open_database`] 在打开时调用。
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    /// 按 schema 演进顺序返回全部本机索引迁移。
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m1_initial::Migration),
            Box::new(m2_call_availability_flags::Migration),
            Box::new(m3_adapter_consistency_key::Migration),
            Box::new(m4_primary_root::Migration),
            Box::new(m5_display_labels::Migration),
            Box::new(m6_root_activation::Migration),
            Box::new(m7_token_snapshots::Migration),
            Box::new(m8_adapter_parse_state::Migration),
        ]
    }
}

/// 按顺序排列的迁移名称，供旧版 `PRAGMA user_version` 数据库桥接使用；
/// 下标即“版本号 - 1”。直接从 [`Migrator::migrations`] 派生，不需要
/// 再手动维护一份平行数组、也不会因为忘记同步而与真实迁移列表脱节。
pub(crate) fn migration_names() -> Vec<String> {
    Migrator::migrations()
        .iter()
        .map(|migration| migration.name().to_owned())
        .collect()
}
