//! 迁移 Codex 本机换皮的资源库、宿主连接与有界生命周期。
//!
//! 平台无关规则逐步委托 `loki_metis_core`；本模块只持有文件系统、进程和 CDP 适配。

pub(crate) mod commands;
mod error;
#[cfg(target_os = "macos")]
mod macos_codex;
#[cfg(target_os = "windows")]
mod windows_codex;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::future::Future;
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, RwLock as StdRwLock};
use std::time::{Duration, Instant, SystemTime};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chromiumoxide::handler::HandlerConfig;
use chromiumoxide::{Browser, Page};
use futures::StreamExt;
use futures::future::join_all;
use loki_metis_core::{
    ColorMode, MAX_SKIN_DELETE_BATCH_ITEMS as MAX_DELETE_BATCH_ITEMS,
    MAX_SKIN_IMPORT_BATCH_FILES as MAX_IMPORT_BATCH_FILES, MAX_THEME_CSS_BYTES,
    MAX_THEME_IMAGE_BYTES as MAX_IMAGE_BYTES, SkinPackageType, SkinReference, SkinRuleError,
    SkinSource, normalize_skin_creator_text, validate_supported_color_modes,
    validate_theme_image_name as validate_core_image_name, validate_theme_metadata,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, OnceCell, watch};
use tokio::task::{AbortHandle, JoinHandle};

use self::error::AppError;

include!("constants.rs");
include!("model.rs");
include!("manifests.rs");
include!("service_state.rs");

include!("service_catalog_body.rs");
include!("service_import_body.rs");
include!("service_runtime_body.rs");
include!("service_install_body.rs");

include!("lifecycle.rs");
include!("catalog.rs");
include!("validation.rs");
include!("conversion.rs");
include!("payload.rs");
include!("instances.rs");
include!("cdp_connect.rs");
include!("appearance.rs");
include!("platform.rs");
include!("injection.rs");

#[cfg(test)]
mod tests {
    include!("tests/support.rs");
    include!("tests/runtime.rs");
    include!("tests/themes.rs");
    include!("tests/imports.rs");
    include!("tests/catalog.rs");
    include!("tests/host.rs");
    include!("tests/conversion.rs");
}
