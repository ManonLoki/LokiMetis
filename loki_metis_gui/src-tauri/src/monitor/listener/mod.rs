//! 本机环回 Hook listener：接受受支持 AI 工具的最小信封并记账。

mod control;
mod http_protocol;
mod lifecycle;
mod loopback_server;

use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

use loki_metis_core::{
    AiTool, HOOK_RELAY_EPHEMERAL_PORT, PetOverlayToolState, hook_relay_loopback_address,
};
use serde::Serialize;
use tauri::AppHandle;
use tokio::sync::mpsc;

pub use control::HookListenerControl;
use control::{HookListenerPolicy, write_hook_relay_status};
use http_protocol::ConnectionOutcome;
use lifecycle::run_hook_worker;
use loopback_server::run_listener;

#[cfg(test)]
use control::hook_listener_policy;
#[cfg(test)]
use http_protocol::{encode_http_response, handle_connection, parse_hook_request};
#[cfg(test)]
use lifecycle::{expire_inactive_hook_sessions, process_hook_event};
#[cfg(test)]
use loopback_server::{
    bind_local_hook_relay_listener, bound_hook_relay_address, enqueue_connection_outcome,
    hook_relay_bind_addr,
};

/// listener 到状态机 worker 的有界事件队列容量。
const HOOK_EVENT_QUEUE_CAPACITY: usize = 256;

/// listener 接收时附带的启用代数；禁用或重启用后旧排队事件会失效。
#[derive(Clone, Debug, PartialEq, Eq)]
struct QueuedHookEvent {
    event: IncomingHookEvent,
    generation: u64,
}

/// 通过 HTTP 边界校验后送入生命周期状态机的完整事件。
#[derive(Clone, Debug, PartialEq, Eq)]
struct IncomingHookEvent {
    tool: AiTool,
    hook_type: String,
    session_id: Option<String>,
    turn_id: Option<String>,
    status: Option<String>,
}

/// 工作台展示的最近一次 Hook 事件。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookRelayLastEvent {
    /// 工具。
    pub tool: AiTool,
    /// 事件名。
    pub hook_type: String,
}

/// 本机 Hook 中继运行状态。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookRelayStatus {
    /// 是否正在监听。
    pub listening: bool,
    /// 绑定地址。
    pub bind_address: String,
    /// 已接受事件数。
    pub received_count: u64,
    /// 失败数。
    pub failed_count: u64,
    /// 最近一次成功事件。
    pub last_event: Option<HookRelayLastEvent>,
    /// 桌宠状态的全局递增版本；每次有效展示或释放迁移都增长。
    pub revision: u64,
    /// 各位置最近一次展示或释放状态，供同位置按版本决胜。
    pub pet_states: Vec<PetOverlayToolState>,
    /// 最近一次错误。
    pub last_error: Option<String>,
}

impl Default for HookRelayStatus {
    /// 返回尚未监听且计数、状态均为空的中继状态。
    fn default() -> Self {
        Self {
            listening: false,
            bind_address: hook_relay_loopback_address(HOOK_RELAY_EPHEMERAL_PORT),
            received_count: 0,
            failed_count: 0,
            last_event: None,
            revision: 0,
            pet_states: Vec::new(),
            last_error: None,
        }
    }
}

/// 启动环回 listener，返回可查询状态与可同步更新的启用门禁。
pub fn spawn_hook_listener(
    app: AppHandle,
    config_dir: PathBuf,
    enabled_tools: Vec<AiTool>,
) -> (Arc<RwLock<HookRelayStatus>>, HookListenerControl) {
    let status = Arc::new(RwLock::new(HookRelayStatus::default()));
    let policy = Arc::new(RwLock::new(HookListenerPolicy::new(&enabled_tools)));
    let (sender, receiver) = mpsc::channel(HOOK_EVENT_QUEUE_CAPACITY);
    tauri::async_runtime::spawn(run_hook_worker(
        receiver,
        Arc::clone(&status),
        app,
        config_dir,
        Arc::clone(&policy),
    ));
    let shared = Arc::clone(&status);
    let listener_policy = Arc::clone(&policy);
    tauri::async_runtime::spawn(async move {
        if let Err(error) = run_listener(Arc::clone(&shared), sender, listener_policy).await {
            tracing::error!(target: "loki_metis::hook_listener", %error, "hook listener stopped");
            let mut current = write_hook_relay_status(&shared);
            current.listening = false;
            current.last_error = Some(error);
        }
    });
    let control = HookListenerControl {
        policy,
        status: Arc::clone(&status),
    };
    (status, control)
}

#[cfg(test)]
mod protocol_tests;

#[cfg(test)]
mod slot_tests;
