//! 为本机发布性能验收提供默认关闭的临时 JSONL 观测通道。
//!
//! 本模块不属于产品遥测：只有测试进程显式传入环境变量时才创建文件，且只接受
//! 主窗口发送的固定无隐私指标。路径和载荷都在 Rust 边界再次收窄，正常启动不会
//! 创建文件、连接网络或改变任何用户可见状态。

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

const EVIDENCE_PATH_ENVIRONMENT_VARIABLE: &str = "LOKI_METIS_PERFORMANCE_EVIDENCE_PATH";
const JAVASCRIPT_MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_MONOTONIC_TIME_MS: f64 = 604_800_000.0;
const MAX_WALL_TIME_MS: f64 = 10_000_000_000_000.0;
const MAX_MEASURED_DURATION_MS: f64 = 60_000.0;
const MIN_LONG_TASK_DURATION_MS: f64 = 50.0;
const GENERIC_INTERACTION_TARGET: &str = "generic";
const ALLOWED_INTERACTION_TARGETS: &[&str] = &[
    "image-picker-trigger",
    "image-picker-upload",
    "navigation-icon-dashboard",
    "navigation-icon-monitor",
    "navigation-icon-settings",
    "navigation-label-dashboard",
    "navigation-label-monitor",
    "navigation-label-settings",
    "settings-capability-autostart",
    "settings-capability-system_notification",
];

/// 描述前端可写入的四类无隐私性能指标。
#[derive(Debug, Deserialize, Serialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub(crate) enum PerformanceEvidenceMetric {
    RendererCapabilities {
        sequence: u64,
        long_task_supported: bool,
    },
    MainWindowReady {
        sequence: u64,
        wall_time_ms: f64,
        monotonic_time_ms: f64,
    },
    Interaction {
        sequence: u64,
        target: String,
        duration_ms: f64,
    },
    LongTask {
        sequence: u64,
        start_time_ms: f64,
        duration_ms: f64,
    },
}

impl PerformanceEvidenceMetric {
    /// 返回每条指标共享的前端递增序号。
    fn sequence(&self) -> u64 {
        match self {
            Self::RendererCapabilities { sequence, .. }
            | Self::MainWindowReady { sequence, .. }
            | Self::Interaction { sequence, .. }
            | Self::LongTask { sequence, .. } => *sequence,
        }
    }

    /// 在落盘前验证数值范围和唯一允许的固定交互目标。
    fn validate(&self) -> Result<(), PerformanceEvidenceError> {
        let sequence = self.sequence();
        if sequence == 0 || sequence > JAVASCRIPT_MAX_SAFE_INTEGER {
            return Err(PerformanceEvidenceError::InvalidMetric);
        }
        match self {
            Self::RendererCapabilities { .. } => Ok(()),
            Self::MainWindowReady {
                wall_time_ms,
                monotonic_time_ms,
                ..
            } => {
                validate_finite_range(*wall_time_ms, 0.0, MAX_WALL_TIME_MS)?;
                validate_finite_range(*monotonic_time_ms, 0.0, MAX_MONOTONIC_TIME_MS)
            }
            Self::Interaction {
                target,
                duration_ms,
                ..
            } => {
                if target != GENERIC_INTERACTION_TARGET
                    && !ALLOWED_INTERACTION_TARGETS.contains(&target.as_str())
                {
                    return Err(PerformanceEvidenceError::InvalidMetric);
                }
                validate_finite_range(*duration_ms, 0.0, MAX_MEASURED_DURATION_MS)
            }
            Self::LongTask {
                start_time_ms,
                duration_ms,
                ..
            } => {
                validate_finite_range(*start_time_ms, 0.0, MAX_MONOTONIC_TIME_MS)?;
                validate_finite_range(
                    *duration_ms,
                    MIN_LONG_TASK_DURATION_MS,
                    MAX_MEASURED_DURATION_MS,
                )
            }
        }
    }
}

/// 表示性能证据通道可公开给前端的稳定失败类别，不包含本机路径或底层 I/O 文本。
#[derive(Debug, Error, PartialEq, Eq)]
enum PerformanceEvidenceError {
    #[error("performance-evidence-disabled")]
    Disabled,
    #[error("performance-evidence-invalid-path")]
    InvalidPath,
    #[error("performance-evidence-invalid-metric")]
    InvalidMetric,
    #[error("performance-evidence-sequence-rejected")]
    SequenceRejected,
    #[error("performance-evidence-write-failed")]
    WriteFailed,
    #[error("performance-evidence-window-rejected")]
    WindowRejected,
}

/// 持有独占证据文件及最后成功落盘的序号。
struct PerformanceEvidenceWriter {
    file: File,
    last_sequence: u64,
}

impl PerformanceEvidenceWriter {
    /// 验证并同步写入单条 JSONL；失败时不推进序号。
    fn write(&mut self, metric: PerformanceEvidenceMetric) -> Result<(), PerformanceEvidenceError> {
        metric.validate()?;
        if metric.sequence() <= self.last_sequence {
            return Err(PerformanceEvidenceError::SequenceRejected);
        }
        let mut line =
            serde_json::to_vec(&metric).map_err(|_| PerformanceEvidenceError::WriteFailed)?;
        line.push(b'\n');
        self.file
            .write_all(&line)
            .and_then(|()| self.file.flush())
            .map_err(|_| PerformanceEvidenceError::WriteFailed)?;
        self.last_sequence = metric.sequence();
        Ok(())
    }
}

/// 由 Tauri 拥有的性能证据状态；无环境变量时内部 writer 永远为空。
pub(crate) struct PerformanceEvidenceState {
    writer: Option<Mutex<PerformanceEvidenceWriter>>,
}

impl PerformanceEvidenceState {
    /// 从唯一显式环境变量初始化通道；非法目标直接让显式测试启动失败。
    pub(crate) fn from_environment() -> Result<Self, Box<dyn std::error::Error>> {
        let Some(path) = std::env::var_os(EVIDENCE_PATH_ENVIRONMENT_VARIABLE) else {
            return Ok(Self::disabled());
        };
        Self::from_path(PathBuf::from(path), std::env::temp_dir())
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
    }

    /// 构造不会创建文件、也拒绝所有写入的默认状态。
    fn disabled() -> Self {
        Self { writer: None }
    }

    /// 使用显式目标和系统临时目录构造已启用状态，便于隔离验证路径边界。
    fn from_path(
        requested_path: PathBuf,
        system_temp_directory: PathBuf,
    ) -> Result<Self, PerformanceEvidenceError> {
        let file = open_new_evidence_file(&requested_path, &system_temp_directory)?;
        Ok(Self {
            writer: Some(Mutex::new(PerformanceEvidenceWriter {
                file,
                last_sequence: 0,
            })),
        })
    }

    /// 返回通道是否由有效的显式环境变量启用。
    fn is_enabled(&self) -> bool {
        self.writer.is_some()
    }

    /// 把验证后的指标串行写入独占文件，避免并发 IPC 打乱序号。
    fn record(&self, metric: PerformanceEvidenceMetric) -> Result<(), PerformanceEvidenceError> {
        let writer = self
            .writer
            .as_ref()
            .ok_or(PerformanceEvidenceError::Disabled)?;
        writer
            .lock()
            .map_err(|_| PerformanceEvidenceError::WriteFailed)?
            .write(metric)
    }
}

/// 验证浮点数有限且处于闭区间内。
fn validate_finite_range(
    value: f64,
    minimum: f64,
    maximum: f64,
) -> Result<(), PerformanceEvidenceError> {
    if value.is_finite() && value >= minimum && value <= maximum {
        Ok(())
    } else {
        Err(PerformanceEvidenceError::InvalidMetric)
    }
}

/// 在真实系统临时目录内以独占新建方式打开普通证据文件。
fn open_new_evidence_file(
    requested_path: &Path,
    system_temp_directory: &Path,
) -> Result<File, PerformanceEvidenceError> {
    if !requested_path.is_absolute() {
        return Err(PerformanceEvidenceError::InvalidPath);
    }
    let canonical_temp_directory = fs::canonicalize(system_temp_directory)
        .map_err(|_| PerformanceEvidenceError::InvalidPath)?;
    let requested_parent = requested_path
        .parent()
        .ok_or(PerformanceEvidenceError::InvalidPath)?;
    let parent_metadata = fs::symlink_metadata(requested_parent)
        .map_err(|_| PerformanceEvidenceError::InvalidPath)?;
    if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
        return Err(PerformanceEvidenceError::InvalidPath);
    }
    let canonical_parent =
        fs::canonicalize(requested_parent).map_err(|_| PerformanceEvidenceError::InvalidPath)?;
    if !canonical_parent.starts_with(&canonical_temp_directory) {
        return Err(PerformanceEvidenceError::InvalidPath);
    }
    let file_name = requested_path
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or(PerformanceEvidenceError::InvalidPath)?;
    let canonical_target = canonical_parent.join(file_name);
    if fs::symlink_metadata(&canonical_target).is_ok() {
        return Err(PerformanceEvidenceError::InvalidPath);
    }

    let mut options = OpenOptions::new();
    options.append(true).create_new(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let file = options
        .open(&canonical_target)
        .map_err(|_| PerformanceEvidenceError::InvalidPath)?;
    if !file
        .metadata()
        .map_err(|_| PerformanceEvidenceError::InvalidPath)?
        .is_file()
    {
        let _ = fs::remove_file(&canonical_target);
        return Err(PerformanceEvidenceError::InvalidPath);
    }
    #[cfg(unix)]
    {
        if file
            .set_permissions(fs::Permissions::from_mode(0o600))
            .is_err()
        {
            let _ = fs::remove_file(&canonical_target);
            return Err(PerformanceEvidenceError::InvalidPath);
        }
        let permissions_are_private = file
            .metadata()
            .map(|metadata| metadata.permissions().mode() & 0o777 == 0o600)
            .unwrap_or(false);
        if !permissions_are_private {
            let _ = fs::remove_file(&canonical_target);
            return Err(PerformanceEvidenceError::InvalidPath);
        }
    }
    Ok(file)
}

/// 告知主视图是否应安装浏览器侧性能 Observer。
#[tauri::command]
pub(crate) fn get_performance_evidence_status(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, PerformanceEvidenceState>,
) -> Result<bool, String> {
    if window.label() != "main" {
        return Err(PerformanceEvidenceError::WindowRejected.to_string());
    }
    Ok(state.is_enabled())
}

/// 仅接收主窗口发送的固定强类型指标并写入本机临时证据文件。
#[tauri::command]
pub(crate) fn record_performance_evidence(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, PerformanceEvidenceState>,
    payload: PerformanceEvidenceMetric,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err(PerformanceEvidenceError::WindowRejected.to_string());
    }
    state.record(payload).map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "performance_evidence_tests.rs"]
mod tests;
