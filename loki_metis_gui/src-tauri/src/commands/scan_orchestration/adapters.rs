//! 把 Codex/Claude 各自的本机发现结果类型接入 [`super::client::ScanClient`]
//! 骨架所需的 core 通用 trait，使两条管线能共享同一份编排代码。

use std::path::Path;

use loki_metis_core::{DiscoveredRootIdentity, ScanDiscoveryResult};

use crate::backend::local_index::{ClaudeDiscoveryResult, DiscoveredRoot, DiscoveryResult};

impl DiscoveredRootIdentity for DiscoveredRoot {
    /// 返回该 Codex 发现根的绝对路径。
    fn path(&self) -> &Path {
        &self.path
    }

    /// 返回该 Codex 发现根当前的稳定 ID。
    fn root_id(&self) -> &str {
        &self.root_id
    }

    /// 用登记后确定的 ID 与别名覆盖临时发现身份。
    fn adopt_identity(&mut self, root_id: &str, alias: &str) {
        self.root_id = root_id.to_owned();
        self.alias = alias.to_owned();
    }
}

impl DiscoveredRootIdentity for crate::backend::local_index::ClaudeDiscoveredRoot {
    /// 返回该 Claude 发现根的绝对路径。
    fn path(&self) -> &Path {
        &self.path
    }

    /// 返回该 Claude 发现根当前的稳定 ID。
    fn root_id(&self) -> &str {
        &self.root_id
    }

    /// 用登记后确定的 ID 与别名覆盖临时发现身份。
    fn adopt_identity(&mut self, root_id: &str, alias: &str) {
        self.root_id = root_id.to_owned();
        self.alias = alias.to_owned();
    }
}

impl ScanDiscoveryResult for DiscoveryResult {
    /// Codex 发现产出的根类型。
    type Root = crate::backend::local_index::DiscoveredRoot;

    /// 拆解为编排骨架统一处理的字段元组。
    fn into_parts(
        self,
    ) -> (
        Vec<Self::Root>,
        Vec<String>,
        Vec<String>,
        loki_metis_core::CoverageReport,
        u64,
        u64,
        u64,
    ) {
        (
            self.roots,
            self.confirmed_invalid_root_ids,
            self.unconfirmed_root_ids,
            self.coverage,
            self.directories_scanned,
            self.symlink_skipped_count,
            self.network_skipped_count,
        )
    }

    /// 从编排骨架的通用字段元组重建 Codex 发现结果。
    fn from_parts(
        roots: Vec<Self::Root>,
        confirmed_invalid_root_ids: Vec<String>,
        unconfirmed_root_ids: Vec<String>,
        coverage: loki_metis_core::CoverageReport,
        directories_scanned: u64,
        symlink_skipped_count: u64,
        network_skipped_count: u64,
    ) -> Self {
        Self {
            roots,
            confirmed_invalid_root_ids,
            unconfirmed_root_ids,
            coverage,
            directories_scanned,
            symlink_skipped_count,
            network_skipped_count,
        }
    }
}

impl DiscoveredRootIdentity for crate::backend::local_index::GrokDiscoveredRoot {
    /// 返回该 Grok 发现根的绝对路径。
    fn path(&self) -> &Path {
        &self.path
    }

    /// 返回该 Grok 发现根当前的稳定 ID。
    fn root_id(&self) -> &str {
        &self.root_id
    }

    /// 用登记后确定的 ID 与别名覆盖临时发现身份。
    fn adopt_identity(&mut self, root_id: &str, alias: &str) {
        self.root_id = root_id.to_owned();
        self.alias = alias.to_owned();
    }
}

impl ScanDiscoveryResult for crate::backend::local_index::GrokDiscoveryResult {
    /// Grok 发现产出的根类型。
    type Root = crate::backend::local_index::GrokDiscoveredRoot;

    /// 拆解为编排骨架统一处理的字段元组。
    fn into_parts(
        self,
    ) -> (
        Vec<Self::Root>,
        Vec<String>,
        Vec<String>,
        loki_metis_core::CoverageReport,
        u64,
        u64,
        u64,
    ) {
        (
            self.roots,
            self.confirmed_invalid_root_ids,
            self.unconfirmed_root_ids,
            self.coverage,
            self.directories_scanned,
            self.symlink_skipped_count,
            self.network_skipped_count,
        )
    }

    /// 从编排骨架的通用字段元组重建 Grok 发现结果。
    fn from_parts(
        roots: Vec<Self::Root>,
        confirmed_invalid_root_ids: Vec<String>,
        unconfirmed_root_ids: Vec<String>,
        coverage: loki_metis_core::CoverageReport,
        directories_scanned: u64,
        symlink_skipped_count: u64,
        network_skipped_count: u64,
    ) -> Self {
        Self {
            roots,
            confirmed_invalid_root_ids,
            unconfirmed_root_ids,
            coverage,
            directories_scanned,
            symlink_skipped_count,
            network_skipped_count,
        }
    }
}

impl ScanDiscoveryResult for ClaudeDiscoveryResult {
    /// Claude 发现产出的根类型。
    type Root = crate::backend::local_index::ClaudeDiscoveredRoot;

    /// 拆解为编排骨架统一处理的字段元组。
    fn into_parts(
        self,
    ) -> (
        Vec<Self::Root>,
        Vec<String>,
        Vec<String>,
        loki_metis_core::CoverageReport,
        u64,
        u64,
        u64,
    ) {
        (
            self.roots,
            self.confirmed_invalid_root_ids,
            self.unconfirmed_root_ids,
            self.coverage,
            self.directories_scanned,
            self.symlink_skipped_count,
            self.network_skipped_count,
        )
    }

    /// 从编排骨架的通用字段元组重建 Claude 发现结果。
    fn from_parts(
        roots: Vec<Self::Root>,
        confirmed_invalid_root_ids: Vec<String>,
        unconfirmed_root_ids: Vec<String>,
        coverage: loki_metis_core::CoverageReport,
        directories_scanned: u64,
        symlink_skipped_count: u64,
        network_skipped_count: u64,
    ) -> Self {
        Self {
            roots,
            confirmed_invalid_root_ids,
            unconfirmed_root_ids,
            coverage,
            directories_scanned,
            symlink_skipped_count,
            network_skipped_count,
        }
    }
}
