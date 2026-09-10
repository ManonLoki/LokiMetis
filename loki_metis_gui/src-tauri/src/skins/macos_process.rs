//! 在 macOS 上以有界系统命令和签名复核管理换肤宿主进程。
//!
//! 所有保留的外部命令均使用可信绝对路径，并由本模块拥有、限时、终止与回收；
//! 启动、终止和重启前还会重新执行 Security.framework 身份验证与 PID 路径绑定。

use std::collections::{BTreeSet, HashMap};
use std::io;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use loki_metis_core::SkinHostKind;
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
    AppError, PlatformCodexProcess, ResolvedCodexInstance, matching_process_command_lines,
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
) -> Result<Vec<PlatformCodexProcess>, AppError> {
    let app = discover_host_app(host)?;
    Ok(command_lines_for_verified(host, &app.executable)
        .await?
        .into_iter()
        .map(|(pid, command_line)| PlatformCodexProcess {
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
    selected: &ResolvedCodexInstance,
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
    let Some(owner_pid) =
        unique_loopback_listener_owner(&String::from_utf8_lossy(&listener_output.stdout), port)
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
    if !process_descends_from(owner_pid, root_pid, &parents) {
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

/// 返回进程身份在操作前发生变化的稳定错误。
fn instance_changed_error(host: SkinHostKind) -> AppError {
    AppError::new(
        match host {
            SkinHostKind::Codex => "skin.codex_instance_changed",
            SkinHostKind::WorkBuddy => "skin.workbuddy_instance_changed",
        },
        format!(
            "所选 {} 实例身份已变化，请刷新后重试。",
            host.display_name()
        ),
    )
}

/// 返回无法完整终止可信宿主进程的稳定错误。
fn force_close_error(host: SkinHostKind) -> AppError {
    AppError::new(
        match host {
            SkinHostKind::Codex => "skin.codex_force_close_failed",
            SkinHostKind::WorkBuddy => "skin.workbuddy_force_close_failed",
        },
        format!("无法关闭 {}，请保存工作后手动退出。", host.display_name()),
    )
}

/// 返回宿主未能在强制关闭期限内退出的稳定错误。
fn force_close_timeout_error(host: SkinHostKind) -> AppError {
    AppError::new(
        match host {
            SkinHostKind::Codex => "skin.codex_force_close_timeout",
            SkinHostKind::WorkBuddy => "skin.workbuddy_force_close_timeout",
        },
        format!(
            "{} 未能在关闭期限内退出，请保存工作后手动关闭。",
            host.display_name()
        ),
    )
}

/// 返回不暴露端口、PID 或安装路径的宿主 owner 校验错误。
fn endpoint_owner_inspection_failed(host: SkinHostKind) -> AppError {
    if host == SkinHostKind::WorkBuddy {
        return super::workbuddy_owner_inspection_failed();
    }
    AppError::new(
        "skin.codex_cdp_owner_inspection_failed",
        format!(
            "无法验证 {} 调试端口所属进程，未应用皮肤。",
            host.display_name()
        ),
    )
}

/// 从 `lsof -Fpn` 输出中提取唯一的回环监听进程；任何通配或歧义绑定均拒绝。
fn unique_loopback_listener_owner(output: &str, port: u16) -> Option<u32> {
    let ipv4 = format!("127.0.0.1:{port}");
    let ipv6 = format!("[::1]:{port}");
    let mut current_pid = None;
    let mut owners = BTreeSet::new();
    for line in output.lines().filter(|line| !line.is_empty()) {
        match line.as_bytes()[0] {
            b'p' => current_pid = line[1..].parse::<u32>().ok(),
            b'n' => {
                let address = &line[1..];
                if address != ipv4 && address != ipv6 {
                    return None;
                }
                owners.insert(current_pid?);
            }
            _ => {}
        }
    }
    (owners.len() == 1)
        .then(|| owners.into_iter().next())
        .flatten()
}

/// 解析 `ps` 的 PID/PPID 快照；重复 PID 或畸形行使整份快照失效。
fn parse_process_parents(output: &str) -> Option<HashMap<u32, u32>> {
    let mut parents = HashMap::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let mut fields = line.split_whitespace();
        let pid = fields.next()?.parse::<u32>().ok()?;
        let parent_pid = fields.next()?.parse::<u32>().ok()?;
        if fields.next().is_some() || parents.insert(pid, parent_pid).is_some() {
            return None;
        }
    }
    (!parents.is_empty()).then_some(parents)
}

/// 沿不可循环的父进程链确认 listener owner 归属于指定已验证根进程。
fn process_descends_from(owner_pid: u32, root_pid: u32, parents: &HashMap<u32, u32>) -> bool {
    if !parents.contains_key(&owner_pid) || !parents.contains_key(&root_pid) {
        return false;
    }
    let mut current = owner_pid;
    for _ in 0..parents.len() {
        if current == root_pid {
            return true;
        }
        if current == 0 {
            return false;
        }
        let Some(parent) = parents.get(&current) else {
            return false;
        };
        current = *parent;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 唯一 owner 可同时监听 IPv4 与 IPv6 回环地址。
    #[test]
    fn listener_owner_accepts_one_process_on_loopback_only() {
        let output = "p42\nn127.0.0.1:9442\nn[::1]:9442\n";
        assert_eq!(unique_loopback_listener_owner(output, 9442), Some(42));
    }

    /// 通配地址、多个 owner 或缺失 owner 的监听记录必须保守拒绝。
    #[test]
    fn listener_owner_rejects_wildcard_ambiguous_and_orphan_rows() {
        assert_eq!(unique_loopback_listener_owner("p42\nn*:9442\n", 9442), None);
        assert_eq!(
            unique_loopback_listener_owner("p42\nn127.0.0.1:9442\np43\nn[::1]:9442\n", 9442),
            None
        );
        assert_eq!(
            unique_loopback_listener_owner("n127.0.0.1:9442\n", 9442),
            None
        );
    }

    /// listener owner 可以经过多个中间进程归属于已验证 WorkBuddy 根。
    #[test]
    fn process_owner_must_descend_from_verified_root() {
        let parents = parse_process_parents("10 1\n20 10\n30 20\n").expect("valid snapshot");
        assert!(process_descends_from(30, 10, &parents));
        assert!(process_descends_from(10, 10, &parents));
        assert!(!process_descends_from(30, 11, &parents));
        assert!(!process_descends_from(99, 99, &parents));
    }

    /// 缺失父进程与循环链不能伪装成已验证根的后代。
    #[test]
    fn process_owner_rejects_missing_and_cyclic_ancestry() {
        let missing = parse_process_parents("20 10\n30 20\n").expect("valid snapshot");
        assert!(!process_descends_from(30, 9, &missing));
        let cyclic = parse_process_parents("20 30\n30 20\n").expect("valid snapshot");
        assert!(!process_descends_from(30, 10, &cyclic));
    }

    /// 畸形或重复 PID 会使整个进程快照失效，避免部分解析后误接受。
    #[test]
    fn process_parent_parser_rejects_malformed_or_duplicate_rows() {
        assert!(parse_process_parents("10 1 extra\n").is_none());
        assert!(parse_process_parents("10 1\n10 2\n").is_none());
        assert!(parse_process_parents("not-a-pid 1\n").is_none());
    }

    /// 输出保留上限必须同时报告截断，不能让 `ps` 的部分快照进入宿主身份判断。
    #[test]
    fn limited_reader_marks_truncated_process_snapshots() {
        tauri::async_runtime::block_on(async {
            let exact = vec![b'x'; COMMAND_OUTPUT_LIMIT];
            let complete = read_limited(exact.as_slice())
                .await
                .expect("内存输入应可读取");
            assert_eq!(complete.stdout.len(), COMMAND_OUTPUT_LIMIT);
            assert!(!complete.truncated);

            let oversized = vec![b'x'; COMMAND_OUTPUT_LIMIT + 1];
            let truncated = read_limited(oversized.as_slice())
                .await
                .expect("内存输入应可读取");
            assert_eq!(truncated.stdout.len(), COMMAND_OUTPUT_LIMIT);
            assert!(truncated.truncated);
        });
    }

    /// Codex 与 WorkBuddy 都必须把截断 `ps` 视为权威进程检查失败，而不是未运行。
    #[test]
    fn truncated_process_snapshot_fails_closed_for_each_host() {
        assert_eq!(
            ensure_complete_process_snapshot(SkinHostKind::Codex, true)
                .expect_err("Codex 截断快照必须拒绝")
                .code,
            "skin.codex_process_inspection_failed"
        );
        assert_eq!(
            ensure_complete_process_snapshot(SkinHostKind::WorkBuddy, true)
                .expect_err("WorkBuddy 截断快照必须拒绝")
                .code,
            "skin.workbuddy_process_inspection_failed"
        );
        assert!(ensure_complete_process_snapshot(SkinHostKind::Codex, false).is_ok());
    }

    /// Codex 与 WorkBuddy 的 owner 读取失败必须保留可分辨的稳定错误合同。
    #[test]
    fn endpoint_owner_failure_is_host_specific() {
        assert_eq!(
            endpoint_owner_inspection_failed(SkinHostKind::Codex).code,
            "skin.codex_cdp_owner_inspection_failed"
        );
        assert_eq!(
            endpoint_owner_inspection_failed(SkinHostKind::WorkBuddy).code,
            "skin.workbuddy_cdp_owner_inspection_failed"
        );
    }

    /// 退出观察必须有固定期限，并能区分仍存活与已不存在的 PID。
    #[test]
    fn process_exit_wait_is_bounded_and_observable() {
        tauri::async_runtime::block_on(async {
            let current_pid = std::process::id();
            let current_executable = std::env::current_exe().expect("测试进程路径应可读取");
            assert!(process_matches_executable(current_pid, &current_executable));
            assert!(
                !process_exits_within(current_pid, &current_executable, Duration::from_millis(1))
                    .await
            );
            assert!(
                process_exits_within(u32::MAX, &current_executable, Duration::from_millis(1)).await
            );
        });
    }

    /// 强制关闭在返回成功前必须等待每个 PID，并重新读取可信宿主进程集。
    #[test]
    fn force_close_contract_waits_and_rechecks_processes() {
        let source = include_str!("macos_process.rs");
        let start = source
            .find("pub(crate) async fn force_close_host")
            .expect("必须保留强制关闭入口");
        let body = &source[start..];
        let end = body
            .find("\n}\n\n/// 验证指定端口")
            .expect("必须保留强制关闭函数边界");
        let body = &body[..end];
        assert!(body.contains("process_exits_within"));
        assert!(body.matches("command_lines_for_verified").count() >= 2);
        assert!(body.matches("verify_host_executable").count() >= 2);
        assert!(body.contains("close_deadline.saturating_duration_since"));
        assert!(body.contains("force_close_timeout_error"));
    }

    /// 无界输出命令必须在截止时间后被终止回收，并返回可区分的超时。
    #[test]
    fn bounded_command_times_out_and_reaps_child() {
        tauri::async_runtime::block_on(async {
            let command = Command::new("/usr/bin/yes");
            let error = run_bounded_command(command, Duration::from_millis(30))
                .await
                .expect_err("无界命令必须超时");
            assert_eq!(error, BoundedCommandError::TimedOut);
        });
    }

    /// 非零退出必须保留状态，供调用层映射为可观察失败而非伪装成功。
    #[test]
    fn bounded_command_preserves_failure_status() {
        tauri::async_runtime::block_on(async {
            let output = run_bounded_command(Command::new("/usr/bin/false"), COMMAND_TIMEOUT)
                .await
                .expect("系统命令应可启动并回收");
            assert!(!output.status.success());
        });
    }

    /// 进程 owner 容量耗尽应映射为稳定 I/O 故障，不得误报 spawn 或超时。
    #[test]
    fn process_capacity_error_maps_to_bounded_io_failure() {
        assert_eq!(
            BoundedCommandError::from(OwnedProcessError::CapacityExceeded),
            BoundedCommandError::Io
        );
    }

    /// macOS 宿主发现与命令执行不得回退到 PATH、mdfind 或 plist 自报信息。
    #[test]
    fn host_process_source_uses_only_fixed_system_boundaries() {
        let source = include_str!("macos_process.rs");
        let platform = include_str!("platform/mod.rs");
        assert!(!source.contains("Command::new(\"ps\")"));
        assert!(!source.contains("Command::new(\"lsof\")"));
        assert!(!source.contains("Command::new(\"open\")"));
        assert!(!platform.contains("/usr/bin/mdfind"));
        assert!(!platform.contains("/usr/bin/plutil"));
    }
}
