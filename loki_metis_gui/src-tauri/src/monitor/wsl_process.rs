//! WSL 子进程的受信启动、有界轮询、管道 owner 与诊断输出。

use std::{
    io::{Read, Write},
    process::{Command, ExitStatus, Stdio},
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use loki_metis_core::HookError;
use windows::Win32::{
    System::Com::CoTaskMemFree,
    UI::Shell::{FOLDERID_System, KF_FLAG_DEFAULT, SHGetKnownFolderPath},
};

use super::process_owner::{reap_retained_wsl_children, retain_wsl_child};
use super::{
    OwnedWslIoTask, WSL_COMMAND_POLL, WslIoTaskError, reap_finished_wsl_io_threads,
    wsl_command_deadlines,
};

/// 每条输出流最多保留的诊断字节；超出部分继续排空但不进入内存。
const WSL_OUTPUT_LIMIT: usize = 64 * 1024;
/// 独立临时文件清理的总预算，不复用已超时或已取消的原操作期限。
const WSL_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

/// 一次参数化 WSL 命令的完整结果。
pub(super) struct WslCommandOutput {
    pub(super) status: ExitStatus,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
    pub(super) stdout_truncated: bool,
    pub(super) stderr_truncated: bool,
}

/// 受管的 WSL stdout 或 stderr 有界读取线程。
type WslReadTask = OwnedWslIoTask<std::io::Result<(Vec<u8>, bool)>>;
/// 受管的 WSL stdin 完整写入线程。
type WslWriteTask = OwnedWslIoTask<std::io::Result<()>>;

/// 一个 WSL 子进程的全部 stdio owner；构造中途失败也由 RAII 移交已启动线程。
struct WslIoOwners {
    input: Option<WslWriteTask>,
    stdout: Option<WslReadTask>,
    stderr: WslReadTask,
}

impl WslIoOwners {
    /// 从刚启动的 child 接管全部管道，任一步失败都由字段 Drop 移交已建线程。
    fn start(
        child: &mut std::process::Child,
        input: Option<Vec<u8>>,
        distribution: &str,
        error_code: &'static str,
    ) -> Result<Self, HookError> {
        let stdout = if input.is_none() {
            let reader = child.stdout.take().ok_or_else(|| {
                wsl_lifecycle_error(
                    distribution,
                    error_code,
                    "cannot capture WSL command output",
                )
            })?;
            Some(
                spawn_bounded_reader(reader, "wsl-stdout")
                    .map_err(|error| wsl_io_spawn_error(distribution, error_code, &error))?,
            )
        } else {
            None
        };
        let stderr = child.stderr.take().ok_or_else(|| {
            wsl_lifecycle_error(
                distribution,
                error_code,
                "cannot capture WSL command diagnostics",
            )
        })?;
        let stderr = spawn_bounded_reader(stderr, "wsl-stderr")
            .map_err(|error| wsl_io_spawn_error(distribution, error_code, &error))?;
        let input = input
            .map(|content| {
                let mut stdin = child.stdin.take().ok_or_else(|| {
                    wsl_lifecycle_error(distribution, error_code, "cannot open WSL command input")
                })?;
                OwnedWslIoTask::spawn("wsl-stdin", move || stdin.write_all(&content))
                    .map_err(|error| wsl_io_spawn_error(distribution, error_code, &error))
            })
            .transpose()?;
        Ok(Self {
            input,
            stdout,
            stderr,
        })
    }

    /// 在最终截止前依次确认 stdin/stdout/stderr 终态并返回有界输出。
    fn finish(
        self,
        deadline: Instant,
        distribution: &str,
        error_code: &'static str,
    ) -> Result<(Vec<u8>, bool, Vec<u8>, bool), HookError> {
        let input = self
            .input
            .map(|task| finish_wsl_io_task(task, deadline, distribution, error_code))
            .transpose();
        let stdout = self
            .stdout
            .map(|task| finish_wsl_io_task(task, deadline, distribution, error_code))
            .transpose();
        let stderr = finish_wsl_io_task(self.stderr, deadline, distribution, error_code);
        input?;
        let (stdout, stdout_truncated) = stdout?.unwrap_or_default();
        let (stderr, stderr_truncated) = stderr?;
        Ok((stdout, stdout_truncated, stderr, stderr_truncated))
    }

    /// 异常退出时尝试收敛全部管道；超时线程自动移交长期 owner。
    fn settle(self, deadline: Instant, distribution: &str, error_code: &'static str) {
        settle_wsl_io_task(self.input, deadline, distribution, error_code);
        settle_wsl_io_task(self.stdout, deadline, distribution, error_code);
        settle_wsl_io_task(Some(self.stderr), deadline, distribution, error_code);
    }
}

/// 启动、限时并完整回收一个参数化 WSL 子进程。
pub(super) fn run_wsl_process<const N: usize>(
    distribution: &str,
    command: [&str; N],
    input: Option<Vec<u8>>,
    error_code: &'static str,
    cancellation: &AtomicBool,
    shared_deadline: &OnceLock<Instant>,
) -> Result<WslCommandOutput, HookError> {
    reap_retained_wsl_children();
    reap_finished_wsl_io_threads();
    let (operation_deadline, final_deadline) = wsl_command_deadlines(shared_deadline);
    ensure_wsl_command_may_start(cancellation, operation_deadline, distribution, error_code)?;

    let capture_stdout = input.is_none();
    let executable = windows_system_wsl_executable(distribution, error_code)?;
    let mut process = Command::new(executable);
    process
        .args(["-d", distribution, "--"])
        .args(command)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(if capture_stdout {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stderr(Stdio::piped());
    let mut child = process
        .spawn()
        .map_err(|error| wsl_launch_error(distribution, error_code, &error))?;
    let io = match WslIoOwners::start(&mut child, input, distribution, error_code) {
        Ok(io) => io,
        Err(error) => {
            cleanup_wsl_process(child, final_deadline, None, distribution, error_code);
            return Err(error);
        }
    };
    let status = match wait_for_wsl_child(
        &mut child,
        cancellation,
        operation_deadline,
        distribution,
        error_code,
    ) {
        Ok(status) => status,
        Err(error) => {
            cleanup_wsl_process(child, final_deadline, Some(io), distribution, error_code);
            return Err(error);
        }
    };
    let (stdout, stdout_truncated, stderr, stderr_truncated) =
        io.finish(final_deadline, distribution, error_code)?;
    Ok(WslCommandOutput {
        status,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    })
}

/// 运行一个不经 shell 插值的 WSL 文件操作，并限制生命周期与输出。
pub(super) fn run_wsl<const N: usize>(
    distribution: &str,
    command: [&str; N],
    cancellation: &AtomicBool,
    shared_deadline: &OnceLock<Instant>,
) -> Result<Vec<u8>, HookError> {
    let output = run_wsl_process(
        distribution,
        command,
        None,
        "error.hooks.writeFailed",
        cancellation,
        shared_deadline,
    )?;
    if !output.status.success() {
        return Err(wsl_command_error(
            distribution,
            "error.hooks.writeFailed",
            &output.stderr,
            output.stderr_truncated,
        ));
    }
    if output.stdout_truncated {
        return Err(wsl_output_limit_error(
            distribution,
            "error.hooks.writeFailed",
        ));
    }
    Ok(output.stdout)
}

/// 使用全新的未取消令牌和短截止时间执行临时文件清理，不继承失败操作的预算。
pub(super) fn run_wsl_cleanup<const N: usize>(
    distribution: &str,
    command: [&str; N],
) -> Result<Vec<u8>, HookError> {
    let cancellation = AtomicBool::new(false);
    let deadline = OnceLock::new();
    let _ = deadline.set(Instant::now() + WSL_CLEANUP_TIMEOUT);
    run_wsl(distribution, command, &cancellation, &deadline)
}

/// 启动前检查取消与整体时限，过期操作不再产生新子进程。
fn ensure_wsl_command_may_start(
    cancellation: &AtomicBool,
    operation_deadline: Instant,
    distribution: &str,
    error_code: &'static str,
) -> Result<(), HookError> {
    if cancellation.load(Ordering::Acquire) {
        return Err(wsl_lifecycle_error(
            distribution,
            error_code,
            "WSL command cancelled during application shutdown",
        ));
    }
    if Instant::now() >= operation_deadline {
        return Err(wsl_lifecycle_error(
            distribution,
            error_code,
            "WSL configuration operation exceeded its shared wall-clock timeout",
        ));
    }
    Ok(())
}

/// 只用非阻塞 `try_wait` 轮询子进程，每轮同时观察共享取消信号。
fn wait_for_wsl_child(
    child: &mut std::process::Child,
    cancellation: &AtomicBool,
    operation_deadline: Instant,
    distribution: &str,
    error_code: &'static str,
) -> Result<ExitStatus, HookError> {
    loop {
        if cancellation.load(Ordering::Acquire) {
            return Err(wsl_lifecycle_error(
                distribution,
                error_code,
                "WSL command cancelled during application shutdown",
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < operation_deadline => thread::sleep(
                WSL_COMMAND_POLL.min(operation_deadline.saturating_duration_since(Instant::now())),
            ),
            Ok(None) => {
                return Err(wsl_lifecycle_error(
                    distribution,
                    error_code,
                    "WSL configuration operation exceeded its shared wall-clock timeout",
                ));
            }
            Err(error) => {
                return Err(HookError::new(error_code)
                    .param("distribution", distribution)
                    .param("detail", error.to_string()));
            }
        }
    }
}

/// 只从 Windows System Known Folder 解析 `wsl.exe`，不信任 PATH 或环境变量。
fn windows_system_wsl_executable(
    distribution: &str,
    error_code: &'static str,
) -> Result<std::path::PathBuf, HookError> {
    let raw =
        unsafe { SHGetKnownFolderPath(&FOLDERID_System, KF_FLAG_DEFAULT, None) }.map_err(|_| {
            wsl_lifecycle_error(
                distribution,
                error_code,
                "Windows system directory is unavailable",
            )
        })?;
    let system_directory = unsafe { raw.to_string() }.map(std::path::PathBuf::from);
    unsafe { CoTaskMemFree(Some(raw.0.cast())) };
    let executable = system_directory
        .map_err(|_| {
            wsl_lifecycle_error(
                distribution,
                error_code,
                "Windows system directory is not valid Unicode",
            )
        })?
        .join("wsl.exe");
    if !executable.is_file() {
        return Err(wsl_lifecycle_error(
            distribution,
            error_code,
            "Windows system WSL executable is unavailable",
        ));
    }
    Ok(executable)
}

/// 持续排空子进程输出，只保留固定上限，避免管道反压和不受控内存增长。
fn spawn_bounded_reader(
    mut reader: impl Read + Send + 'static,
    label: &'static str,
) -> std::io::Result<WslReadTask> {
    OwnedWslIoTask::spawn(label, move || {
        let mut captured = Vec::new();
        let mut truncated = false;
        let mut chunk = [0_u8; 4096];
        loop {
            let read = reader.read(&mut chunk)?;
            if read == 0 {
                return Ok((captured, truncated));
            }
            let retained = WSL_OUTPUT_LIMIT.saturating_sub(captured.len()).min(read);
            captured.extend_from_slice(&chunk[..retained]);
            truncated |= retained < read;
        }
    })
}

/// 在共同截止时间前收集一项 stdio 任务，并把异常保留为稳定错误。
fn finish_wsl_io_task<T: Send + 'static>(
    task: OwnedWslIoTask<std::io::Result<T>>,
    deadline: Instant,
    distribution: &str,
    error_code: &'static str,
) -> Result<T, HookError> {
    let label = task.label;
    let result = match task.finish(deadline) {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => Err(HookError::new(error_code)
            .param("distribution", distribution)
            .param("detail", format!("{label} failed: {error}"))),
        Err(error) => Err(wsl_io_task_error(distribution, error_code, label, error)),
    };
    if let Err(error) = &result {
        tracing::error!(%error, task = label, "WSL stdio task did not finish cleanly");
    }
    result
}

/// 早退路径尝试在剩余时限内收敛 stdio；超时句柄由保留表继续持有。
fn settle_wsl_io_task<T: Send + 'static>(
    task: Option<OwnedWslIoTask<std::io::Result<T>>>,
    deadline: Instant,
    distribution: &str,
    error_code: &'static str,
) {
    if let Some(task) = task {
        let _ = finish_wsl_io_task(task, deadline, distribution, error_code);
    }
}

/// 异常退出先终止子进程，再在同一最终截止时间内回收所有 stdio owner。
fn cleanup_wsl_process(
    mut child: std::process::Child,
    final_deadline: Instant,
    io: Option<WslIoOwners>,
    distribution: &str,
    error_code: &'static str,
) {
    let remaining = final_deadline.saturating_duration_since(Instant::now());
    let child_deadline = Instant::now() + remaining / 2;
    if let Err(error) = terminate_wsl_child_until(&mut child, child_deadline) {
        let process_id = child.id();
        tracing::error!(
            distribution,
            process_id,
            %error,
            "WSL child did not reach a terminal state; retaining its process handle"
        );
        retain_wsl_child(child);
    }
    if let Some(io) = io {
        io.settle(final_deadline, distribution, error_code);
    }
    reap_finished_wsl_io_threads();
}

/// 取消或超时后只使用 `try_wait` 在有界截止时间内确认子进程终态。
fn terminate_wsl_child_until(
    child: &mut std::process::Child,
    deadline: Instant,
) -> Result<(), String> {
    let kill_error = child.kill().err();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) if Instant::now() < deadline => thread::sleep(
                WSL_COMMAND_POLL.min(deadline.saturating_duration_since(Instant::now())),
            ),
            Ok(None) => {
                return Err(kill_error
                    .map(|error| format!("kill failed ({error}); process reap timed out"))
                    .unwrap_or_else(|| "process reap timed out".to_owned()));
            }
            Err(error) => return Err(format!("process reap failed: {error}")),
        }
    }
}

/// 把可信 `wsl.exe` 启动失败映射为稳定 Hook 错误。
fn wsl_launch_error(distribution: &str, code: &'static str, error: &std::io::Error) -> HookError {
    HookError::new(code)
        .param("distribution", distribution)
        .param("detail", error.to_string())
}

/// 把 stdio owner 线程启动失败映射为不泄漏配置内容的错误。
fn wsl_io_spawn_error(distribution: &str, code: &'static str, error: &std::io::Error) -> HookError {
    HookError::new(code)
        .param("distribution", distribution)
        .param("detail", format!("cannot start WSL stdio owner: {error}"))
}

/// 把 stdio 超时、panic 或结果丢失归一为稳定生命周期错误。
fn wsl_io_task_error(
    distribution: &str,
    code: &'static str,
    label: &'static str,
    error: WslIoTaskError,
) -> HookError {
    let state = match error {
        WslIoTaskError::TimedOut => "did not finish before the shared deadline",
        WslIoTaskError::Panicked => "panicked",
        WslIoTaskError::ResultUnavailable => "finished without returning a result",
    };
    HookError::new(code)
        .param("distribution", distribution)
        .param("detail", format!("{label} {state}"))
}

/// 构造包含发行版但不包含配置正文的 WSL 生命周期错误。
fn wsl_lifecycle_error(distribution: &str, code: &'static str, detail: &'static str) -> HookError {
    HookError::new(code)
        .param("distribution", distribution)
        .param("detail", detail)
}

/// 输出超过固定诊断上限时返回稳定错误。
pub(super) fn wsl_output_limit_error(distribution: &str, code: &'static str) -> HookError {
    wsl_lifecycle_error(
        distribution,
        code,
        "WSL command output exceeded the diagnostic limit",
    )
}

/// 把非零退出与有界 stderr 映射为 Hook 错误，并明确标记截断。
pub(super) fn wsl_command_error(
    distribution: &str,
    code: &'static str,
    stderr: &[u8],
    truncated: bool,
) -> HookError {
    let mut detail = String::from_utf8_lossy(stderr).trim().to_owned();
    if truncated {
        detail.push_str(" [truncated]");
    }
    HookError::new(code)
        .param("distribution", distribution)
        .param("detail", detail)
}

#[cfg(test)]
#[path = "wsl_process_tests.rs"]
mod tests;
