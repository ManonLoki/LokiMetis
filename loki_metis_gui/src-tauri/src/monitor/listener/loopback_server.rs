//! 环回 TCP listener、连接接收与有界队列入口。

use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use loki_metis_core::{HOOK_RELAY_EPHEMERAL_PORT, hook_relay_loopback_address};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
use tokio::time::timeout;

use super::super::relay;
use super::control::{HookListenerPolicy, hook_listener_policy, write_hook_relay_status};
use super::http_protocol::{fail, handle_connection, write_http_response};
use super::{ConnectionOutcome, HookRelayStatus, QueuedHookEvent};

/// 单条本机 HTTP 请求从建立连接到读完正文的最长时间。
const HOOK_HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
/// listener 回写短响应的最长时间。
const HOOK_HTTP_RESPONSE_TIMEOUT: Duration = Duration::from_secs(3);

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
) -> Result<(), String> {
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
    loop {
        let (mut stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
        let status = Arc::clone(&status);
        let sender = sender.clone();
        let policy = Arc::clone(&policy);
        let instance_id = rendezvous.instance_id.clone();
        tauri::async_runtime::spawn(async move {
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
        });
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
