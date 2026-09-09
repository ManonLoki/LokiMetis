//! Windows 宿主访问 WSL Hook 配置的窄适配层；不承担普通本机文件读写。

use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

use loki_metis_core::HookError;
#[cfg(any(target_os = "windows", test))]
use std::{
    sync::{
        OnceLock,
        mpsc::{Receiver, sync_channel},
    },
    thread,
    time::{Duration, Instant},
};

#[cfg(any(target_os = "windows", test))]
use super::thread_owner::RetainedThreadOwner;

#[cfg(target_os = "windows")]
#[path = "wsl_process.rs"]
mod process;
#[cfg(target_os = "windows")]
#[path = "wsl_process_owner.rs"]
mod process_owner;
#[cfg(target_os = "windows")]
use process::{
    run_wsl, run_wsl_cleanup, run_wsl_process, wsl_command_error, wsl_output_limit_error,
};
/// 已超过本轮截止时间的 I/O 线程仍由显式 owner 持有，不把句柄静默 detach。
#[cfg(any(target_os = "windows", test))]
static WSL_RETAINED_IO_THREADS: OnceLock<RetainedThreadOwner> = OnceLock::new();
/// 单次 WSL 文件操作的 wall-clock 上限，避免失联发行版永久占住 Hook writer。
#[cfg(any(target_os = "windows", test))]
const WSL_COMMAND_TIMEOUT: Duration = Duration::from_secs(15);
/// 总时限尾部保留给子进程终止与 I/O 线程收敛。
#[cfg(any(target_os = "windows", test))]
const WSL_COMMAND_FINAL_REAP_BUDGET: Duration = Duration::from_secs(1);
/// WSL 子进程退出与应用生命周期取消的轮询间隔。
#[cfg(any(target_os = "windows", test))]
const WSL_COMMAND_POLL: Duration = Duration::from_millis(10);
/// 固定脚本以 077 umask 与 noclobber 单次打开临时文件，路径只经位置参数传递。
#[cfg(any(target_os = "windows", test))]
const WSL_PRIVATE_TEMP_WRITE_SCRIPT: &str = "umask 077; set -C; { trap 'rm -f -- \"$1\"' EXIT HUP INT TERM; cat || exit; trap - EXIT HUP INT TERM; } > \"$1\"";

/// 描述一项 stdio 任务在共同截止时间内未能取得终态的原因。
#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WslIoTaskError {
    TimedOut,
    Panicked,
    ResultUnavailable,
}

/// 持有一项阻塞 stdio 任务；只在 `is_finished` 证实终态后才调用 `join`。
#[cfg(any(target_os = "windows", test))]
struct OwnedWslIoTask<T> {
    label: &'static str,
    result: Receiver<T>,
    handle: Option<thread::JoinHandle<()>>,
}

#[cfg(any(target_os = "windows", test))]
impl<T: Send + 'static> OwnedWslIoTask<T> {
    /// 使用有界单槽通道传递结果，使超时句柄可被非泛型 owner 继续持有。
    fn spawn(
        label: &'static str,
        task: impl FnOnce() -> T + Send + 'static,
    ) -> std::io::Result<Self> {
        let (sender, result) = sync_channel(1);
        let handle = thread::Builder::new()
            .name(format!("loki-metis-{label}"))
            .spawn(move || {
                let output = task();
                let _ = sender.send(output);
            })?;
        Ok(Self {
            label,
            result,
            handle: Some(handle),
        })
    }

    /// 在绝对截止时间前等待终态；超时时 Drop 会把句柄移交给全局 owner。
    fn finish(mut self, deadline: Instant) -> Result<T, WslIoTaskError> {
        while !self
            .handle
            .as_ref()
            .is_some_and(thread::JoinHandle::is_finished)
        {
            if Instant::now() >= deadline {
                return Err(WslIoTaskError::TimedOut);
            }
            thread::sleep(WSL_COMMAND_POLL.min(deadline.saturating_duration_since(Instant::now())));
        }
        let handle = self.handle.take().expect("WSL I/O task remains owned");
        handle.join().map_err(|_| WslIoTaskError::Panicked)?;
        self.result
            .try_recv()
            .map_err(|_| WslIoTaskError::ResultUnavailable)
    }
}

#[cfg(any(target_os = "windows", test))]
impl<T> Drop for OwnedWslIoTask<T> {
    /// 异常返回时仍回收已完成线程，未完成句柄转入显式保留表。
    fn drop(&mut self) {
        let Some(handle) = self.handle.take() else {
            return;
        };
        if handle.is_finished() {
            let _ = handle.join();
        } else {
            tracing::warn!(
                task = self.label,
                "WSL stdio task outlived its owning call and remains explicitly retained"
            );
            retain_wsl_io_thread(handle);
        }
    }
}

/// 返回跨 WSL 命令共享的 stdio 线程 owner。
#[cfg(any(target_os = "windows", test))]
fn wsl_io_thread_owner() -> &'static RetainedThreadOwner {
    WSL_RETAINED_IO_THREADS.get_or_init(|| RetainedThreadOwner::new("wsl-stdio"))
}

/// 把超过本轮截止时间的 stdio 句柄移交后台 owner，空闲时也会最终 join。
#[cfg(any(target_os = "windows", test))]
fn retain_wsl_io_thread(handle: thread::JoinHandle<()>) {
    wsl_io_thread_owner().retain(handle);
}

/// 非阻塞维护 stdio owner；真实回收不再依赖下一条 WSL 命令。
#[cfg(target_os = "windows")]
fn reap_finished_wsl_io_threads() {
    wsl_io_thread_owner().reap_finished();
}

/// 从 Windows WSL UNC 路径解析出的发行版与 Linux 目录。
#[derive(Clone, Debug)]
pub(super) struct WslDirectory {
    distribution: String,
    linux_path: String,
    cancellation: Arc<AtomicBool>,
    /// 首条 WSL 命令确定的整次配置写入共同截止时间。
    deadline: Arc<std::sync::OnceLock<std::time::Instant>>,
}

/// WSL 发行版内一个可读写的确定文件。
#[derive(Clone, Debug)]
pub(super) struct WslFile {
    distribution: String,
    linux_path: String,
    cancellation: Arc<AtomicBool>,
    deadline: Arc<std::sync::OnceLock<std::time::Instant>>,
}

impl PartialEq for WslDirectory {
    /// 目录身份只由发行版与 Linux 路径决定，运行时取消状态不属于值语义。
    fn eq(&self, other: &Self) -> bool {
        self.distribution == other.distribution && self.linux_path == other.linux_path
    }
}

impl Eq for WslDirectory {}

impl PartialEq for WslFile {
    /// 文件身份只由发行版与 Linux 路径决定，共享截止时间不影响相等性。
    fn eq(&self, other: &Self) -> bool {
        self.distribution == other.distribution && self.linux_path == other.linux_path
    }
}

impl Eq for WslFile {}

/// 一次 WSL 原子替换的临时路径 owner；失败与取消都使用独立预算清理。
#[cfg(target_os = "windows")]
struct WslTemporaryFile<'a> {
    distribution: &'a str,
    linux_path: String,
    armed: bool,
}

#[cfg(target_os = "windows")]
impl<'a> WslTemporaryFile<'a> {
    /// 创建尚未落盘但已由本次写入独占命名的临时路径 owner。
    fn new(distribution: &'a str, linux_path: String) -> Self {
        Self {
            distribution,
            linux_path,
            armed: true,
        }
    }

    /// 原子替换成功后解除清理；该路径已经由 `mv` 消费。
    fn disarm(&mut self) {
        self.armed = false;
    }
}

#[cfg(target_os = "windows")]
impl Drop for WslTemporaryFile<'_> {
    /// 用不共享原操作取消令牌或截止时间的短命令清理失败路径。
    fn drop(&mut self) {
        if self.armed
            && run_wsl_cleanup(
                self.distribution,
                ["rm", "-f", "--", self.linux_path.as_str()],
            )
            .is_err()
        {
            tracing::warn!(
                distribution = self.distribution,
                "failed to confirm cleanup of a WSL temporary hook file"
            );
        }
    }
}

impl WslDirectory {
    /// 识别 `\\wsl.localhost\发行版\...` 与 `\\wsl$\发行版\...`，并拒绝路径穿越。
    pub(super) fn parse(directory: &str) -> Option<Self> {
        let normalized = directory.replace('/', "\\");
        let lowercase = normalized.to_ascii_lowercase();
        let prefix_length = ["\\\\wsl.localhost\\", "\\\\wsl$\\"]
            .into_iter()
            .find(|prefix| lowercase.starts_with(prefix))?
            .len();
        let mut components = normalized[prefix_length..]
            .split('\\')
            .filter(|component| !component.is_empty());
        let distribution = components.next()?.to_owned();
        let remainder = components.collect::<Vec<_>>();
        if distribution == "."
            || distribution == ".."
            || remainder
                .iter()
                .any(|component| matches!(*component, "." | ".."))
        {
            return None;
        }
        Some(Self {
            distribution,
            linux_path: format!("/{}", remainder.join("/")),
            cancellation: Arc::new(AtomicBool::new(false)),
            deadline: Arc::new(std::sync::OnceLock::new()),
        })
    }

    /// 后台 writer 注入应用生命周期取消令牌；显式写入继续使用目录自己的未取消令牌。
    pub(super) fn with_cancellation(mut self, cancellation: Option<Arc<AtomicBool>>) -> Self {
        if let Some(cancellation) = cancellation {
            self.cancellation = cancellation;
        }
        self
    }

    /// 在已解析目录下拼接由受信任协议提供的相对配置文件名。
    pub(super) fn join(&self, relative_path: &str) -> WslFile {
        let relative_path = relative_path.replace('\\', "/");
        WslFile {
            distribution: self.distribution.clone(),
            linux_path: format!(
                "{}/{}",
                self.linux_path.trim_end_matches('/'),
                relative_path.trim_start_matches('/')
            ),
            cancellation: Arc::clone(&self.cancellation),
            deadline: Arc::clone(&self.deadline),
        }
    }

    /// 通过目标发行版的 `wslpath` 把 Windows relay 路径转换成 Linux 可执行路径。
    #[cfg(target_os = "windows")]
    pub(super) fn translate_windows_executable(
        &self,
        executable: &Path,
    ) -> Result<String, HookError> {
        let executable = executable.to_str().ok_or_else(|| {
            HookError::new("error.hooks.writeFailed")
                .param("detail", "relay executable path is not Unicode")
        })?;
        let normalized = wslpath_input(executable);
        let output = run_wsl(
            &self.distribution,
            ["wslpath", "-u", normalized.as_str()],
            &self.cancellation,
            &self.deadline,
        )?;
        let translated = String::from_utf8(output).map_err(|error| {
            HookError::new("error.hooks.writeFailed").param("detail", error.to_string())
        })?;
        let translated = translated.trim();
        if translated.is_empty() || !translated.starts_with('/') {
            return Err(HookError::new("error.hooks.writeFailed")
                .param("detail", "wslpath returned a non-absolute path"));
        }
        Ok(translated.to_owned())
    }

    /// 非 Windows 构建保留同一接口，但明确拒绝真正访问 WSL。
    #[cfg(not(target_os = "windows"))]
    pub(super) fn translate_windows_executable(
        &self,
        _executable: &Path,
    ) -> Result<String, HookError> {
        let _ = self;
        Err(HookError::new("error.hooks.wslWindowsHostOnly"))
    }
}

/// 归一化传给 `wslpath` 的 Windows 路径，避免反斜杠被第二层命令解析吞掉。
#[cfg(any(target_os = "windows", test))]
fn wslpath_input(executable: &str) -> String {
    executable.replace('\\', "/")
}

impl WslFile {
    /// 返回不含 Windows 用户目录的 Linux 侧路径，供结构化错误定位目标。
    pub(super) fn display(&self) -> &str {
        &self.linux_path
    }

    /// 读取 WSL 内配置；文件不存在是正常的首次写入状态。
    #[cfg(target_os = "windows")]
    pub(super) fn read_optional(&self) -> Result<Option<String>, HookError> {
        let output = run_wsl_process(
            &self.distribution,
            ["cat", "--", &self.linux_path],
            None,
            "error.hooks.existingReadFailed",
            &self.cancellation,
            &self.deadline,
        )?;
        if output.status.success() {
            if output.stdout_truncated {
                return Err(wsl_output_limit_error(
                    &self.distribution,
                    "error.hooks.existingReadFailed",
                ));
            }
            return String::from_utf8(output.stdout).map(Some).map_err(|error| {
                HookError::new("error.hooks.existingReadFailed").param("detail", error.to_string())
            });
        }

        let existence = run_wsl_process(
            &self.distribution,
            ["test", "-e", &self.linux_path],
            None,
            "error.hooks.existingReadFailed",
            &self.cancellation,
            &self.deadline,
        )?;
        if wsl_test_reports_missing(existence.status.code()) {
            return Ok(None);
        }
        Err(wsl_command_error(
            &self.distribution,
            "error.hooks.existingReadFailed",
            &output.stderr,
            output.stderr_truncated,
        ))
    }

    /// 非 Windows 构建不会尝试把 UNC 当作本机路径访问。
    #[cfg(not(target_os = "windows"))]
    pub(super) fn read_optional(&self) -> Result<Option<String>, HookError> {
        let _ = (&self.cancellation, &self.deadline);
        Err(HookError::new("error.hooks.wslWindowsHostOnly"))
    }

    /// 在 WSL 内用同目录临时文件和 `mv` 原子替换目标配置。
    #[cfg(target_os = "windows")]
    pub(super) fn write_atomic(&self, content: &str) -> Result<(), HookError> {
        let parent = self
            .linux_path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .filter(|parent| !parent.is_empty())
            .ok_or_else(|| {
                HookError::new("error.hooks.writeFailed")
                    .param("detail", "cannot determine WSL parent directory")
            })?;
        run_wsl(
            &self.distribution,
            ["mkdir", "-p", "--", parent],
            &self.cancellation,
            &self.deadline,
        )?;

        let mut temporary =
            WslTemporaryFile::new(&self.distribution, unique_temporary_path(&self.linux_path));
        let output = run_wsl_process(
            &self.distribution,
            [
                "sh",
                "-c",
                WSL_PRIVATE_TEMP_WRITE_SCRIPT,
                "loki-metis",
                temporary.linux_path.as_str(),
            ],
            Some(content.as_bytes().to_vec()),
            "error.hooks.writeFailed",
            &self.cancellation,
            &self.deadline,
        )?;
        if !output.status.success() {
            return Err(wsl_command_error(
                &self.distribution,
                "error.hooks.writeFailed",
                &output.stderr,
                output.stderr_truncated,
            ));
        }

        if let Err(error) = run_wsl(
            &self.distribution,
            [
                "mv",
                "-f",
                "--",
                temporary.linux_path.as_str(),
                &self.linux_path,
            ],
            &self.cancellation,
            &self.deadline,
        ) {
            return Err(error);
        }
        temporary.disarm();
        Ok(())
    }

    /// 非 Windows 构建不会启动 WSL 子进程。
    #[cfg(not(target_os = "windows"))]
    pub(super) fn write_atomic(&self, _content: &str) -> Result<(), HookError> {
        let _ = self;
        Err(HookError::new("error.hooks.wslWindowsHostOnly"))
    }
}

/// 为 WSL 同目录原子替换生成不可预测的 UUID 临时路径。
#[cfg(any(target_os = "windows", test))]
fn unique_temporary_path(target: &str) -> String {
    format!("{target}.lokimetis.{}.tmp", uuid::Uuid::new_v4().simple())
}

/// 应用退出时在同一短预算内请求 WSL 子进程与 stdio owner 收敛。
#[cfg(target_os = "windows")]
pub(super) fn shutdown_wsl_owners(timeout: Duration) {
    let deadline = Instant::now() + timeout;
    process_owner::shutdown_retained_wsl_children_until(deadline);
    let _ = wsl_io_thread_owner().wait_until(deadline);
}

/// 判断 POSIX `test -e` 的退出码是否明确表示目标不存在。
#[cfg(any(target_os = "windows", test))]
fn wsl_test_reports_missing(status_code: Option<i32>) -> bool {
    status_code == Some(1)
}

/// 第一条命令锁定整次配置写入时限，后续读写与迁移不得重置。
#[cfg(any(target_os = "windows", test))]
fn wsl_command_deadlines(shared_deadline: &OnceLock<Instant>) -> (Instant, Instant) {
    let final_deadline = *shared_deadline.get_or_init(|| Instant::now() + WSL_COMMAND_TIMEOUT);
    let operation_deadline = final_deadline
        .checked_sub(WSL_COMMAND_FINAL_REAP_BUDGET)
        .unwrap_or(final_deadline);
    (operation_deadline, final_deadline)
}

#[cfg(test)]
#[path = "wsl_tests.rs"]
mod tests;
