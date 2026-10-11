//! 在 macOS 上以有界系统命令和签名复核管理换肤宿主进程。
//!
//! 所有保留的外部命令均使用可信绝对路径，并由本模块拥有、限时、终止与回收；
//! 启动、终止和重启前还会重新执行 Security.framework 身份验证与 PID 路径绑定。

use std::collections::{BTreeSet, HashMap};
use std::io;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use loki_metris_core::SkinHostKind;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use super::macos_identity::{
    VerifiedMacApp, discover_host_app, process_matches_executable, terminate_verified_process,
    verify_discovered_executable, verify_host_executable,
};
use super::process_reaper::{
    OwnedProcessChild, OwnedProcessError, prepare_process_spawn, process_shutdown_deadline,
    wait_for_process_shutdown,
};
use super::{
    AppError, PlatformHostProcess, ResolvedSkinHostInstance, matching_process_command_lines,
};

const OPEN_PATH: &str = "/usr/bin/open";
const LSOF_PATH: &str = "/usr/sbin/lsof";
const PS_PATH: &str = "/bin/ps";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(4);
const COMMAND_REAP_TIMEOUT: Duration = Duration::from_secs(1);
const COMMAND_OUTPUT_LIMIT: usize = 512 * 1024;
const PROCESS_EXIT_TIMEOUT: Duration = Duration::from_secs(5);
const PROCESS_EXIT_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// 保存有界命令的退出状态与经过大小限制的标准输出。
#[derive(Debug)]
struct BoundedCommandOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    truncated: bool,
}

/// 保留输出上限以及是否仍读到额外字节，供身份快照拒绝不完整数据。
struct LimitedCommandOutput {
    stdout: Vec<u8>,
    truncated: bool,
}

/// 描述外部命令启动、I/O、超时或回收阶段的可观察失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BoundedCommandError {
    Spawn,
    Io,
    TimedOut,
    ReapTimedOut,
}

impl From<OwnedProcessError> for BoundedCommandError {
    /// 将共享进程 owner 错误收敛为 macOS 命令稳定合同。
    fn from(error: OwnedProcessError) -> Self {
        match error {
            OwnedProcessError::Spawn => Self::Spawn,
            OwnedProcessError::ReapTimedOut => Self::ReapTimedOut,
            OwnedProcessError::Io
            | OwnedProcessError::ShuttingDown
            | OwnedProcessError::CapacityExceeded => Self::Io,
        }
    }
}

/// 运行一个已由调用方绑定可信绝对路径的命令，并在超时/取消时终止回收。
async fn run_bounded_command(
    mut command: Command,
    deadline: Duration,
) -> Result<BoundedCommandOutput, BoundedCommandError> {
    let started_at = tokio::time::Instant::now();
    let execution_deadline = started_at + deadline;
    let final_deadline = execution_deadline + COMMAND_REAP_TIMEOUT;
    prepare_process_spawn(execution_deadline).await?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut owner = OwnedProcessChild::spawn(&mut command)?;
    let stdout = owner
        .child_mut()
        .stdout
        .take()
        .ok_or(BoundedCommandError::Io)?;
    let mut shutdown = owner.shutdown_receiver();

    let completed = tokio::select! {
        biased;
        _ = wait_for_process_shutdown(&mut shutdown) => None,
        completed = tokio::time::timeout_at(execution_deadline, async {
            let (status, stdout) = tokio::join!(owner.child_mut().wait(), read_limited(stdout));
            let stdout = stdout.map_err(|_| BoundedCommandError::Io)?;
            Ok::<_, BoundedCommandError>(BoundedCommandOutput {
                status: status.map_err(|_| BoundedCommandError::Io)?,
                stdout: stdout.stdout,
                truncated: stdout.truncated,
            })
        }) => Some(completed),
    };
    match completed {
        Some(Ok(result)) => result,
        Some(Err(_)) => {
            owner.terminate_and_reap_until(final_deadline).await?;
            Err(BoundedCommandError::TimedOut)
        }
        None => {
            let shutdown_deadline = process_shutdown_deadline()
                .unwrap_or(final_deadline)
                .min(final_deadline);
            owner.terminate_and_reap_until(shutdown_deadline).await?;
            Err(BoundedCommandError::Io)
        }
    }
}

/// 让 macOS 签名回归测试复用同一套子进程超时、终止与回收边界。
#[cfg(test)]
pub(crate) async fn run_bounded_test_command(command: Command) -> bool {
    run_bounded_command(command, COMMAND_TIMEOUT)
        .await
        .is_ok_and(|output| output.status.success())
}

/// 持续排空子进程输出以避免管道阻塞，但只保留固定上限的字节。
async fn read_limited(mut reader: impl AsyncRead + Unpin) -> io::Result<LimitedCommandOutput> {
    let mut retained = Vec::new();
    let mut chunk = [0_u8; 8192];
    let mut truncated = false;
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        let remaining = COMMAND_OUTPUT_LIMIT.saturating_sub(retained.len());
        retained.extend_from_slice(&chunk[..read.min(remaining)]);
        truncated |= read > remaining;
    }
    Ok(LimitedCommandOutput {
        stdout: retained,
        truncated,
    })
}

/// 判断官方宿主是否存在可信主进程。
pub(crate) async fn host_is_running(host: SkinHostKind) -> Result<bool, AppError> {
    let app = discover_host_app(host)?;
    Ok(!command_lines_for_verified(host, &app.executable)
        .await?
        .is_empty())
}

/// 读取签名有效宿主主程序对应的 PID 与完整命令行。
pub(crate) async fn host_command_lines(host: SkinHostKind) -> Result<Vec<(u32, String)>, AppError> {
    let app = discover_host_app(host)?;
    command_lines_for_verified(host, &app.executable).await
}

/// 将可信命令行快照转换为统一的平台宿主进程。
pub(crate) async fn host_processes(
    host: SkinHostKind,
) -> Result<Vec<PlatformHostProcess>, AppError> {
    let app = discover_host_app(host)?;
    Ok(command_lines_for_verified(host, &app.executable)
        .await?
        .into_iter()
        .map(|(pid, command_line)| PlatformHostProcess {
            pid,
            executable: app.executable.clone(),
            command_line,
        })
        .collect())
}

/// 以宿主固定参数启动签名有效的官方应用。
pub(crate) async fn launch_host(host: SkinHostKind, port: u16) -> Result<(), AppError> {
    let app = discover_host_app(host)?;
    if !command_lines_for_verified(host, &app.executable)
        .await?
        .is_empty()
    {
        return Err(super::manual_close_required_for(host));
    }
    let arguments = match host {
        SkinHostKind::Codex => vec![
            "--remote-debugging-address=127.0.0.1".to_owned(),
            format!("--remote-debugging-port={port}"),
        ],
        SkinHostKind::WorkBuddy => Vec::new(),
    };
    launch_verified_app(host, &app, &arguments, port).await
}

/// 复核所选 PID 与应用签名，终止旧进程后以原参数和新端口重启。
pub(crate) async fn restart_host_instance(
    host: SkinHostKind,
    selected: &ResolvedSkinHostInstance,
    port: u16,
) -> Result<(), AppError> {
    let app = verify_host_executable(host, &selected.process.executable)?;
    if !terminate_verified_process(selected.process.pid, &app.executable) {
        return Err(instance_changed_error(host));
    }
    wait_for_process_exit(selected.process.pid, &app.executable).await?;
    launch_verified_app(host, &app, &selected.arguments, port).await
}

/// 仅终止当前仍绑定签名有效主程序路径的全部宿主进程。
pub(crate) async fn force_close_host(host: SkinHostKind) -> Result<(), AppError> {
    let app = discover_host_app(host)?;
    let processes = command_lines_for_verified(host, &app.executable).await?;
    let close_deadline = tokio::time::Instant::now() + PROCESS_EXIT_TIMEOUT;
    let mut terminated_pids = Vec::with_capacity(processes.len());
    for (pid, _) in processes {
        let current = verify_host_executable(host, &app.executable)?;
        if !terminate_verified_process(pid, &current.executable) {
            return Err(force_close_error(host));
        }
        terminated_pids.push(pid);
    }
    for pid in terminated_pids {
        let remaining = close_deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() || !process_exits_within(pid, &app.executable, remaining).await {
            return Err(force_close_timeout_error(host));
        }
    }

    let current = verify_host_executable(host, &app.executable)?;
    if !command_lines_for_verified(host, &current.executable)
        .await?
        .is_empty()
    {
        return Err(force_close_error(host));
    }
    Ok(())
}

/// 验证指定端口只有一个回环 listener，且 owner 是严格验证宿主根进程或其后代。
pub(crate) async fn host_endpoint_owned_by_root(
    host: SkinHostKind,
    port: u16,
    root_pid: u32,
) -> Result<bool, AppError> {
    let discovered = discover_host_app(host).map_err(|_| endpoint_owner_inspection_failed(host))?;
    let trusted = verify_host_executable(host, &discovered.executable)
        .map_err(|_| endpoint_owner_inspection_failed(host))?;
    if !process_matches_executable(root_pid, &trusted.executable) {
        return Ok(false);
    }

    let mut lsof = Command::new(LSOF_PATH);
    lsof.args(["-nP", "-a"])
        .arg(format!("-iTCP:{port}"))
        .args(["-sTCP:LISTEN", "-Fpn"]);
    let listener_output = run_bounded_command(lsof, COMMAND_TIMEOUT)
        .await
        .map_err(|_| endpoint_owner_inspection_failed(host))?;
    if listener_output.truncated {
        return Err(endpoint_owner_inspection_failed(host));
    }
    if !listener_output.status.success() {
        return if listener_output.stdout.is_empty() {
            Ok(false)
        } else {
            Err(endpoint_owner_inspection_failed(host))
        };
    }
    let Some(owners) =
        loopback_listener_owners(&String::from_utf8_lossy(&listener_output.stdout), port)
    else {
        return Ok(false);
    };

    let mut ps = Command::new(PS_PATH);
    ps.args(["-axo", "pid=,ppid="]);
    let process_output = run_bounded_command(ps, COMMAND_TIMEOUT)
        .await
        .map_err(|_| endpoint_owner_inspection_failed(host))?;
    if !process_output.status.success() || process_output.truncated {
        return Err(endpoint_owner_inspection_failed(host));
    }
    let Some(parents) = parse_process_parents(&String::from_utf8_lossy(&process_output.stdout))
    else {
        return Err(endpoint_owner_inspection_failed(host));
    };
    // ChatGPT 主进程及其 Computer Use 子进程可以共同监听同一回环端口；
    // 每一个监听者都必须属于已验证的宿主进程树，不能因其中一个可信就放行其它进程。
    if !listener_owners_belong_to_root(&owners, root_pid, &parents) {
        return Ok(false);
    }
    let current = verify_host_executable(host, &trusted.executable)
        .map_err(|_| endpoint_owner_inspection_failed(host))?;
    Ok(process_matches_executable(root_pid, &current.executable))
}

/// 复核应用身份后使用系统 `open` 启动，并等待 launcher 有界退出。
async fn launch_verified_app(
    host: SkinHostKind,
    app: &VerifiedMacApp,
    arguments: &[String],
    port: u16,
) -> Result<(), AppError> {
    let current = verify_host_executable(host, &app.executable)?;
    let mut command = Command::new(OPEN_PATH);
    command.args(["-na"]).arg(&current.bundle);
    if !arguments.is_empty() {
        command.arg("--args").args(arguments);
    }
    if host == SkinHostKind::WorkBuddy {
        command.env("WORKBUDDY_REMOTE_DEBUGGING_PORT", port.to_string());
    }
    let output = run_bounded_command(command, COMMAND_TIMEOUT)
        .await
        .map_err(|error| launch_command_error(host, error))?;
    if !output.status.success() {
        return Err(launch_error(host));
    }
    Ok(())
}

/// 通过有界 `ps` 快照列出主程序，并在快照后再次确认 PID 当前路径。
async fn command_lines_for_verified(
    host: SkinHostKind,
    executable: &Path,
) -> Result<Vec<(u32, String)>, AppError> {
    let current = verify_discovered_executable(host, executable)?;
    let mut command = Command::new(PS_PATH);
    command.args(["-axo", "pid=,command="]);
    let output = run_bounded_command(command, COMMAND_TIMEOUT)
        .await
        .map_err(|error| process_inspection_error(host, error))?;
    if !output.status.success() {
        return Err(process_inspection_error(host, BoundedCommandError::Io));
    }
    ensure_complete_process_snapshot(host, output.truncated)?;
    Ok(matching_process_command_lines(
        &String::from_utf8_lossy(&output.stdout),
        &current.executable,
    )
    .into_iter()
    .filter(|(pid, _)| process_matches_executable(*pid, &current.executable))
    .collect())
}

/// 不完整的 `ps` 快照不能证明宿主未运行，也不能进入身份或终止决策。
fn ensure_complete_process_snapshot(host: SkinHostKind, truncated: bool) -> Result<(), AppError> {
    if truncated {
        Err(process_inspection_error(host, BoundedCommandError::Io))
    } else {
        Ok(())
    }
}

/// 在重启路径中等待旧 PID 消失，避免新旧实例并存或 launcher 误判。
async fn wait_for_process_exit(pid: u32, executable: &Path) -> Result<(), AppError> {
    if process_exits_within(pid, executable, PROCESS_EXIT_TIMEOUT).await {
        return Ok(());
    }
    Err(AppError::new(
        "skin.host_restart_close_timeout",
        "宿主进程未能在重启期限内退出，请手动关闭后再试。",
    ))
}

/// 在固定期限内观察 PID 不再映射预期程序；PID 消失或被复用均视为旧进程已退出。
async fn process_exits_within(pid: u32, executable: &Path, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if !process_matches_executable(pid, executable) {
            return true;
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return false;
        }
        tokio::time::sleep(PROCESS_EXIT_POLL_INTERVAL.min(remaining)).await;
    }
}

/// 把命令失败映射为不泄漏路径或参数的宿主进程读取错误。
fn process_inspection_error(host: SkinHostKind, error: BoundedCommandError) -> AppError {
    let timed_out = matches!(
        error,
        BoundedCommandError::TimedOut | BoundedCommandError::ReapTimedOut
    );
    AppError::new(
        match (host, timed_out) {
            (SkinHostKind::Codex, true) => "skin.codex_process_inspection_timeout",
            (SkinHostKind::Codex, false) => "skin.codex_process_inspection_failed",
            (SkinHostKind::WorkBuddy, true) => "skin.workbuddy_process_inspection_timeout",
            (SkinHostKind::WorkBuddy, false) => "skin.workbuddy_process_inspection_failed",
        },
        if timed_out {
            format!("{} 进程检查超时。", host.display_name())
        } else {
            format!("无法读取 {} 进程信息。", host.display_name())
        },
    )
}

/// 把 launcher 失败映射为可区分超时与普通启动失败的稳定错误。
fn launch_command_error(host: SkinHostKind, error: BoundedCommandError) -> AppError {
    if matches!(
        error,
        BoundedCommandError::TimedOut | BoundedCommandError::ReapTimedOut
    ) {
        return AppError::new(
            match host {
                SkinHostKind::Codex => "skin.codex_launch_timeout",
                SkinHostKind::WorkBuddy => "skin.workbuddy_launch_timeout",
            },
            format!("{} 系统启动器响应超时。", host.display_name()),
        );
    }
    launch_error(host)
}

/// 返回宿主启动失败的稳定错误。
fn launch_error(host: SkinHostKind) -> AppError {
    AppError::new(
        match host {
            SkinHostKind::Codex => "skin.codex_launch_failed",
            SkinHostKind::WorkBuddy => "skin.workbuddy_launch_failed",
        },
        format!("无法启动 {}。", host.display_name()),
    )
}

// 为控制单个文件的长度，下列内容拆到同目录的分片文件中；它们用 include! 并入本模块，
// 与本文件共用同一个作用域，改动时不需要额外导入或调整可见性。
// - macos_process_tests.rs：本模块的单元测试
include!("macos_process_tests.rs");
