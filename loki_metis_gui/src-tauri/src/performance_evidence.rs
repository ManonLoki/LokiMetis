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
use tauri::{Emitter, Manager, Runtime};
use thiserror::Error;

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

const EVIDENCE_PATH_ENVIRONMENT_VARIABLE: &str = "LOKI_METIS_PERFORMANCE_EVIDENCE_PATH";
const JAVASCRIPT_MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_MONOTONIC_TIME_MS: f64 = 604_800_000.0;
const MAX_WALL_TIME_MS: f64 = 10_000_000_000_000.0;
const MAX_MEASURED_DURATION_MS: f64 = 60_000.0;
const MIN_BLOCKING_INTERVAL_MS: f64 = 50.0;
/// 测试专用主窗口可见性事件；载荷固定为布尔值。
pub(crate) const PERFORMANCE_EVIDENCE_WINDOW_VISIBILITY_EVENT: &str =
    "loki-metis-performance-window-visibility";

/// 标识实际用于当前会话的渲染阻塞观测源。
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RendererTimingSource {
    AnimationFrameGap,
    PerformanceObserver,
}

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
        long_task_supported: bool,
        timing_source: RendererTimingSource,
    },
    MainWindowReady {
        wall_time_ms: f64,
        monotonic_time_ms: f64,
    },
    Interaction {
        target: String,
        result: String,
        duration_ms: f64,
    },
    RendererBlockingInterval {
        timing_source: RendererTimingSource,
        start_time_ms: f64,
        duration_ms: f64,
    },
}

impl PerformanceEvidenceMetric {
    /// 在落盘前验证数值范围和唯一允许的固定交互目标。
    fn validate(&self) -> Result<(), PerformanceEvidenceError> {
        match self {
            Self::RendererCapabilities {
                long_task_supported,
                timing_source,
            } => match (*long_task_supported, timing_source) {
                (true, RendererTimingSource::PerformanceObserver)
                | (false, RendererTimingSource::AnimationFrameGap) => Ok(()),
                _ => Err(PerformanceEvidenceError::InvalidMetric),
            },
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
                result,
                duration_ms,
                ..
            } => {
                if expected_interaction_result(target) != Some(result.as_str()) {
                    return Err(PerformanceEvidenceError::InvalidMetric);
                }
                validate_finite_range(*duration_ms, 0.0, MAX_MEASURED_DURATION_MS)
            }
            Self::RendererBlockingInterval {
                start_time_ms,
                duration_ms,
                ..
            } => {
                validate_finite_range(*start_time_ms, 0.0, MAX_MONOTONIC_TIME_MS)?;
                validate_finite_range(
                    *duration_ms,
                    MIN_BLOCKING_INTERVAL_MS,
                    MAX_MEASURED_DURATION_MS,
                )
            }
        }
    }

    /// 返回只在能力或阻塞区间载荷中出现的观测源。
    fn timing_source(&self) -> Option<RendererTimingSource> {
        match self {
            Self::RendererCapabilities { timing_source, .. }
            | Self::RendererBlockingInterval { timing_source, .. } => Some(*timing_source),
            _ => None,
        }
    }
}

/// 固定导航动作与其必须可见的固定页面结果一一对应。
fn expected_interaction_result(target: &str) -> Option<&'static str> {
    match target {
        "navigation-dashboard" => Some("dashboard-page"),
        "navigation-monitor" => Some("monitor-page"),
        "navigation-settings" => Some("settings-page"),
        _ => None,
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
    #[error("performance-evidence-finalized")]
    Finalized,
    #[error("performance-evidence-write-failed")]
    WriteFailed,
    #[error("performance-evidence-window-rejected")]
    WindowRejected,
}

/// 持有独占证据文件及最后成功落盘的序号。
struct PerformanceEvidenceWriter {
    file: File,
    last_sequence: u64,
    record_count: u64,
    timing_source: Option<RendererTimingSource>,
    main_window_ready_written: bool,
    finalization: Option<PerformanceEvidenceFinalization>,
}

impl PerformanceEvidenceWriter {
    /// 验证并同步写入单条 JSONL；序号只能在 Rust 成功落盘后推进。
    fn write(
        &mut self,
        metric: PerformanceEvidenceMetric,
    ) -> Result<u64, PerformanceEvidenceError> {
        if self.finalization.is_some() {
            return Err(PerformanceEvidenceError::Finalized);
        }
        metric.validate()?;
        let metric_timing_source = metric.timing_source();
        let is_capabilities = matches!(
            &metric,
            PerformanceEvidenceMetric::RendererCapabilities { .. }
        );
        let is_main_window_ready =
            matches!(&metric, PerformanceEvidenceMetric::MainWindowReady { .. });
        if !is_capabilities && self.timing_source.is_none() {
            return Err(PerformanceEvidenceError::InvalidMetric);
        }
        if matches!(
            &metric,
            PerformanceEvidenceMetric::RendererBlockingInterval { .. }
        ) && metric_timing_source != self.timing_source
        {
            return Err(PerformanceEvidenceError::InvalidMetric);
        }
        if is_capabilities && (self.last_sequence != 0 || self.timing_source.is_some()) {
            return Err(PerformanceEvidenceError::InvalidMetric);
        }
        if is_main_window_ready && self.main_window_ready_written {
            return Err(PerformanceEvidenceError::InvalidMetric);
        }
        let sequence = self
            .last_sequence
            .checked_add(1)
            .filter(|value| *value <= JAVASCRIPT_MAX_SAFE_INTEGER)
            .ok_or(PerformanceEvidenceError::WriteFailed)?;
        let mut stored =
            serde_json::to_value(&metric).map_err(|_| PerformanceEvidenceError::WriteFailed)?;
        stored
            .as_object_mut()
            .ok_or(PerformanceEvidenceError::WriteFailed)?
            .insert("sequence".to_string(), serde_json::Value::from(sequence));
        let mut line =
            serde_json::to_vec(&stored).map_err(|_| PerformanceEvidenceError::WriteFailed)?;
        line.push(b'\n');
        self.file
            .write_all(&line)
            .and_then(|()| self.file.flush())
            .map_err(|_| PerformanceEvidenceError::WriteFailed)?;
        self.last_sequence = sequence;
        self.record_count = sequence;
        if is_capabilities {
            self.timing_source = metric_timing_source;
        }
        if is_main_window_ready {
            self.main_window_ready_written = true;
        }
        Ok(sequence)
    }

    /// 幂等写入最终确认记录并把全部字节同步到文件系统。
    fn finish(&mut self) -> Result<PerformanceEvidenceFinalization, PerformanceEvidenceError> {
        if let Some(finalization) = self.finalization {
            return Ok(finalization);
        }
        if self.timing_source.is_none() || !self.main_window_ready_written {
            return Err(PerformanceEvidenceError::InvalidMetric);
        }
        let final_sequence = self
            .last_sequence
            .checked_add(1)
            .filter(|value| *value <= JAVASCRIPT_MAX_SAFE_INTEGER)
            .ok_or(PerformanceEvidenceError::WriteFailed)?;
        let finalization = PerformanceEvidenceFinalization {
            final_sequence,
            record_count: self.record_count,
        };
        let mut line = serde_json::to_vec(&serde_json::json!({
            "kind": "session-finalized",
            "sequence": final_sequence,
            "recordCount": self.record_count,
        }))
        .map_err(|_| PerformanceEvidenceError::WriteFailed)?;
        line.push(b'\n');
        self.file
            .write_all(&line)
            .and_then(|()| self.file.flush())
            .and_then(|()| self.file.sync_data())
            .map_err(|_| PerformanceEvidenceError::WriteFailed)?;
        self.last_sequence = final_sequence;
        self.finalization = Some(finalization);
        Ok(finalization)
    }
}

/// 返回给前端结束握手的无路径、无业务内容摘要。
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PerformanceEvidenceFinalization {
    final_sequence: u64,
    record_count: u64,
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
                record_count: 0,
                timing_source: None,
                main_window_ready_written: false,
                finalization: None,
            })),
        })
    }

    /// 返回通道是否由有效的显式环境变量启用。
    pub(crate) fn is_enabled(&self) -> bool {
        self.writer.is_some()
    }

    /// 把验证后的指标串行写入独占文件，避免并发 IPC 打乱序号。
    fn record(&self, metric: PerformanceEvidenceMetric) -> Result<u64, PerformanceEvidenceError> {
        let writer = self
            .writer
            .as_ref()
            .ok_or(PerformanceEvidenceError::Disabled)?;
        writer
            .lock()
            .map_err(|_| PerformanceEvidenceError::WriteFailed)?
            .write(metric)
    }

    /// 排空并同步证据文件，返回幂等的最终序号和指标条数。
    fn finish(&self) -> Result<PerformanceEvidenceFinalization, PerformanceEvidenceError> {
        let writer = self
            .writer
            .as_ref()
            .ok_or(PerformanceEvidenceError::Disabled)?;
        writer
            .lock()
            .map_err(|_| PerformanceEvidenceError::WriteFailed)?
            .finish()
    }
}

/// 仅在本机性能证据启用时向主 WebView 发布固定窗口可见性布尔值。
pub(crate) fn emit_performance_evidence_main_window_visibility<R: Runtime>(
    app: &tauri::AppHandle<R>,
    is_visible: bool,
) -> tauri::Result<()> {
    let Some(state) = app.try_state::<PerformanceEvidenceState>() else {
        return Ok(());
    };
    if !state.is_enabled() {
        return Ok(());
    }
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    window.emit(PERFORMANCE_EVIDENCE_WINDOW_VISIBILITY_EVENT, is_visible)
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
    #[cfg(unix)]
    if parent_metadata.permissions().mode() & 0o777 != 0o700 {
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
    if fs::canonicalize(requested_parent)
        .map(|current_parent| current_parent != canonical_parent)
        .unwrap_or(true)
    {
        let _ = fs::remove_file(&canonical_target);
        return Err(PerformanceEvidenceError::InvalidPath);
    }
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
) -> Result<u64, String> {
    if window.label() != "main" {
        return Err(PerformanceEvidenceError::WindowRejected.to_string());
    }
    state.record(payload).map_err(|error| error.to_string())
}

/// 在测试主动退出前完成队尾 IPC、文件 flush 与 sync_data，并写入最终确认记录。
#[tauri::command]
pub(crate) fn finish_performance_evidence(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, PerformanceEvidenceState>,
) -> Result<PerformanceEvidenceFinalization, String> {
    if window.label() != "main" {
        return Err(PerformanceEvidenceError::WindowRejected.to_string());
    }
    state.finish().map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "performance_evidence_tests.rs"]
mod tests;
