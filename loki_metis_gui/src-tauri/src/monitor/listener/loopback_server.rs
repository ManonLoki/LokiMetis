//! 环回 TCP listener、连接接收与有界队列入口。

use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use loki_metis_core::{HOOK_RELAY_EPHEMERAL_PORT, hook_relay_loopback_address};
use tokio::net::TcpListener;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinSet;
use tokio::time::timeout;

use super::super::relay;
use super::control::{
    HookListenerPolicy, HookListenerShutdown, hook_listener_policy, write_hook_relay_status,
};
use super::http_protocol::{fail, handle_connection, write_http_response};
use super::{ConnectionOutcome, HookRelayStatus, QueuedHookEvent};

/// 单条本机 HTTP 请求从建立连接到读完正文的最长时间。
const HOOK_HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
/// listener 回写短响应的最长时间。
const HOOK_HTTP_RESPONSE_TIMEOUT: Duration = Duration::from_secs(3);
/// 未认证连接也计入的固定任务上限，避免慢连接耗尽异步运行时资源。
pub(super) const HOOK_CONNECTION_TASK_LIMIT: usize = 32;

/// 返回只允许操作系统分配端口的回环绑定地址。
pub(super) fn hook_relay_bind_addr() -> std::net::SocketAddr {
    std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, HOOK_RELAY_EPHEMERAL_PORT))
}

/// 始终绑定操作系统分配的回环空闲端口，不抢占其他监控程序的固定端口。
pub async fn bind_local_hook_relay_listener() -> std::io::Result<TcpListener> {
    TcpListener::bind(hook_relay_bind_addr()).await
}

/// 绑定后的实际回环地址，供状态与 relay 投递共用。
pub fn bound_hook_relay_address(listener: &TcpListener) -> std::io::Result<String> {
    Ok(hook_relay_loopback_address(listener.local_addr()?.port()))
}

/// 绑定并接受本机 Hook POST。
pub(super) async fn run_listener(
    status: Arc<RwLock<HookRelayStatus>>,
    sender: mpsc::Sender<QueuedHookEvent>,
    policy: Arc<RwLock<HookListenerPolicy>>,
    shutdown: HookListenerShutdown,
) -> Result<(), String> {
    if shutdown.is_cancelled() {
        return Ok(());
    }
    let listener = bind_local_hook_relay_listener()
        .await
        .map_err(|error| error.to_string())?;
    let bind_address = bound_hook_relay_address(&listener).map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    let rendezvous = relay::HookRelayRendezvous::new(port);
    let rendezvous_path = relay::hook_relay_rendezvous_path().map_err(|error| error.to_string())?;
    relay::persist_hook_relay_rendezvous_at(&rendezvous_path, &rendezvous)
        .map_err(|error| error.to_string())?;
    tracing::info!(
        target: "loki_metis::hook_listener",
        port,
        "hook listener started"
    );
    {
        let mut current = write_hook_relay_status(&status);
        current.listening = true;
        current.bind_address = bind_address;
    }
    let connection_slots = Arc::new(Semaphore::new(HOOK_CONNECTION_TASK_LIMIT));
    let listener_result = run_connection_loop(
        listener,
        Arc::clone(&status),
        sender,
        policy,
        rendezvous.instance_id.clone(),
        shutdown,
        connection_slots,
    )
    .await;
    remove_rendezvous_if_current(&rendezvous_path, &rendezvous);
    write_hook_relay_status(&status).listening = false;
    listener_result
}

/// 接受连接并在固定并发槽内持有全部连接任务，关闭前中止并回收每个任务与许可。
pub(super) async fn run_connection_loop(
    listener: TcpListener,
    status: Arc<RwLock<HookRelayStatus>>,
    sender: mpsc::Sender<QueuedHookEvent>,
    policy: Arc<RwLock<HookListenerPolicy>>,
    instance_id: String,
    mut shutdown: HookListenerShutdown,
    connection_slots: Arc<Semaphore>,
) -> Result<(), String> {
    let mut connections = JoinSet::new();
    let listener_result = loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => break Ok(()),
            completed = connections.join_next(), if !connections.is_empty() => {
                if let Some(Err(error)) = completed
                    && !error.is_cancelled()
                {
                    tracing::warn!(
                        target: "loki_metis::hook_listener",
                        %error,
                        "hook listener connection task stopped unexpectedly"
                    );
                }
            }
            accepted = listener.accept() => {
                let (stream, _) = match accepted {
                    Ok(accepted) => accepted,
                    Err(error) => break Err(error.to_string()),
                };
                let permit = match Arc::clone(&connection_slots).try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => {
                        record_listener_failure(&status, "Hook listener connection limit reached");
                        drop(stream);
                        continue;
                    }
                };
                let connection_status = Arc::clone(&status);
                let connection_sender = sender.clone();
                let connection_policy = Arc::clone(&policy);
                let instance_id = instance_id.clone();
                connections.spawn(async move {
                    // Owned permit 与任务同寿命，正常结束和 abort 都会释放连接槽。
                    let _permit = permit;
                    serve_connection(
                        stream,
                        instance_id,
                        connection_sender,
                        connection_status,
                        connection_policy,
                    )
                    .await;
                });
            }
        }
    };
    connections.abort_all();
    while let Some(completed) = connections.join_next().await {
        if let Err(error) = completed
            && !error.is_cancelled()
        {
            tracing::warn!(
                target: "loki_metis::hook_listener",
                %error,
                "hook listener connection task did not stop cleanly"
            );
        }
    }
    listener_result
}

/// 处理单条已接受连接；其句柄始终由 listener 内部 JoinSet 拥有。
async fn serve_connection(
    mut stream: tokio::net::TcpStream,
    instance_id: String,
    sender: mpsc::Sender<QueuedHookEvent>,
    status: Arc<RwLock<HookRelayStatus>>,
    policy: Arc<RwLock<HookListenerPolicy>>,
) {
    let outcome = match timeout(
        HOOK_HTTP_REQUEST_TIMEOUT,
        handle_connection(&mut stream, &instance_id),
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(_) => fail("408 Request Timeout", "Request Timeout"),
    };
    let (response_status, response_body, authenticated) =
        enqueue_connection_outcome(outcome, &sender, &status, &policy);
    if let Err(error) = timeout(
        HOOK_HTTP_RESPONSE_TIMEOUT,
        write_http_response(
            &mut stream,
            response_status,
            response_body,
            authenticated.then_some(instance_id.as_str()),
        ),
    )
    .await
    .map_err(|_| "response timeout".to_owned())
    .and_then(|result| result.map_err(|error| error.to_string()))
    {
        tracing::warn!(
            target: "loki_metis::hook_listener",
            %error,
            "failed to write hook listener response"
        );
    }
}

/// 仅当磁盘文件仍属于当前实例时删除，避免误删已重启实例的端点。
fn remove_rendezvous_if_current(path: &std::path::Path, expected: &relay::HookRelayRendezvous) {
    let is_current = std::fs::read(path)
        .ok()
        .and_then(|payload| serde_json::from_slice::<relay::HookRelayRendezvous>(&payload).ok())
        .is_some_and(|current| current == *expected);
    if is_current
        && let Err(error) = std::fs::remove_file(path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(
            target: "loki_metis::hook_listener",
            %error,
            "failed to remove the stopped hook listener rendezvous"
        );
    }
}

/// 把解析成功的事件放入有界队列，并为解析或背压失败记账。
pub(super) fn enqueue_connection_outcome(
    outcome: ConnectionOutcome,
    sender: &mpsc::Sender<QueuedHookEvent>,
    status: &Arc<RwLock<HookRelayStatus>>,
    policy: &Arc<RwLock<HookListenerPolicy>>,
) -> (&'static str, &'static str, bool) {
    if !outcome.ok {
        tracing::warn!(
            target: "loki_metis::hook_listener",
            status = outcome.status,
            reason = outcome.body,
            "hook request rejected"
        );
        record_listener_failure(status, outcome.body);
        return (outcome.status, outcome.body, outcome.authenticated);
    }
    let Some(event) = outcome.event else {
        record_listener_failure(status, "Invalid payload");
        return ("400 Bad Request", "Invalid payload", outcome.authenticated);
    };
    let tool = event.tool;
    let hook_type = event.hook_type.clone();
    let Some(generation) = hook_listener_policy(policy).admission(tool) else {
        tracing::debug!(
            target: "loki_metis::hook_listener",
            tool = ?tool,
            %hook_type,
            "hook request ignored because the tool is disabled"
        );
        return (outcome.status, outcome.body, true);
    };
    match sender.try_send(QueuedHookEvent { event, generation }) {
        Ok(()) => {
            tracing::debug!(
                target: "loki_metis::hook_listener",
                tool = ?tool,
                %hook_type,
                "hook request accepted"
            );
            (outcome.status, outcome.body, true)
        }
        Err(TrySendError::Full(_)) => {
            tracing::warn!(
                target: "loki_metis::hook_listener",
                tool = ?tool,
                %hook_type,
                "hook event queue is full"
            );
            record_listener_failure(status, "Hook event queue is full");
            ("503 Service Unavailable", "Service Unavailable", true)
        }
        Err(TrySendError::Closed(_)) => {
            tracing::error!(
                target: "loki_metis::hook_listener",
                tool = ?tool,
                %hook_type,
                "hook event worker is unavailable"
            );
            record_listener_failure(status, "Hook event worker is unavailable");
            ("503 Service Unavailable", "Service Unavailable", true)
        }
    }
}

/// 记录 HTTP 边界或队列失败，不伪造任何桌宠展示迁移。
fn record_listener_failure(status: &Arc<RwLock<HookRelayStatus>>, error: &str) {
    let mut current = write_hook_relay_status(status);
    current.failed_count += 1;
    current.last_error = Some(error.to_owned());
}
