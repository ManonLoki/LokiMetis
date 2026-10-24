//! 维护 Codex 与 WorkBuddy 本机换皮的资源库、宿主连接与有界生命周期。
//!
//! 平台无关规则逐步委托 `loki_metris_core`；本模块只持有文件系统、进程和 CDP 适配。

pub use error::AppError as SkinError;
mod error;
#[cfg(target_os = "macos")]
mod macos_codex;
#[cfg(target_os = "macos")]
mod macos_identity;
#[cfg(target_os = "macos")]
mod macos_process;
#[cfg(unix)]
mod process_reaper;
#[cfg(target_os = "windows")]
mod windows_codex;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::future::Future;
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, RwLock as StdRwLock, Weak as StdWeak};
use std::time::{Duration, Instant, SystemTime};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chromiumoxide::handler::HandlerConfig;
use chromiumoxide::{Browser, Page};
use futures::StreamExt;
use futures::future::join_all;
use loki_metris_core::{
    ColorMode, ImageFormat, MAX_SKIN_DELETE_BATCH_ITEMS as MAX_DELETE_BATCH_ITEMS,
    MAX_SKIN_IMPORT_BATCH_FILES as MAX_IMPORT_BATCH_FILES, MAX_THEME_CSS_BYTES,
    MAX_THEME_IMAGE_BYTES as MAX_IMAGE_BYTES, SkinHostKind, SkinPackageType, SkinReference,
    SkinRuleError, SkinSource, image_data_url as gallery_image_data_url,
    normalize_skin_creator_text, validate_supported_color_modes,
    validate_theme_image_name as validate_core_image_name, validate_theme_metadata,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, OnceCell, watch};
use tokio::task::{AbortHandle, JoinHandle};

use self::error::AppError;

mod constants;
use constants::*;
mod model;
pub use model::*;
mod manifests;
use manifests::*;
mod manifests_runtime_types;
use manifests_runtime_types::*;
mod runtime_owners;
use runtime_owners::*;
mod handler_task_owner;
use handler_task_owner::*;
mod service_state;
pub use service_state::*;

mod service_catalog_body;
mod service_import_body;
mod service_runtime_body;
mod service_install_body;

mod watch_task_owner;
use watch_task_owner::*;
mod lifecycle;
pub(crate) use lifecycle::*;
mod catalog;
use catalog::*;
mod validation;
use validation::*;
#[cfg(test)]
mod test_seams;
#[cfg(test)]
use test_seams::*;
mod conversion;
use conversion::*;
mod archive_export;
use archive_export::*;
mod archive_import;
use archive_import::*;
mod file_manager;
use file_manager::*;
mod payload;
use payload::*;
pub mod isolation;
use isolation::*;
pub mod authority;
pub use authority::*;
mod instances;
use instances::*;
mod cdp_connect;
use cdp_connect::*;
mod appearance;
use appearance::*;
mod platform;
use platform::*;
mod injection;
use injection::*;

/// 宿主回归使用独立Tokio运行时，不依赖应用全局runtime。
#[cfg(test)]
fn test_block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap().block_on(future)
}

#[cfg(test)]
mod tests {
    include!("tests/support.rs");
    include!("tests/lifecycle.rs");
    include!("tests/handler_task_owner.rs");
    include!("tests/runtime.rs");
    include!("tests/runtime_observations.rs");
    include!("tests/themes.rs");
    include!("tests/imports.rs");
    include!("tests/catalog.rs");
    include!("tests/host.rs");
    include!("tests/conversion.rs");
    include!("tests/isolation.rs");
    include!("tests/authority.rs");
}
