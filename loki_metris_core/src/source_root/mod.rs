//! 来源根查找与持久层编排的共享边界。

mod catalog;
mod lookup;

pub use catalog::{
    CatalogFuture, SourceRootCandidate, SourceRootCatalog, SourceRootCatalogError,
    SourceRootCatalogOperationError, SourceRootCatalogRecord, SourceRootReindexRequest,
    source_root_add, source_root_reindex_request, source_root_remove, source_root_rename,
    source_root_set_enabled, source_root_set_primary,
};
pub use lookup::{
    SourceRootLookupError, resolve_enabled_source_root_by_id, resolve_source_root_by_id,
};
