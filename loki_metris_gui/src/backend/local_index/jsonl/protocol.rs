//! 只声明 Codex rollout 的最小协议字段；正文由 serde 跳过。

use serde::Deserialize;
use serde_json::Value;

/// 只声明 JSONL 顶层时间、事件类型与最小 payload。
#[derive(Deserialize)]
pub(super) struct RolloutEnvelope {
    /// 该行事件的时间戳原始值；格式在别处校验。
    #[serde(default)]
    pub(super) timestamp: Option<Value>,
    /// 事件类型标签。
    #[serde(rename = "type", default)]
    pub(super) kind: Option<String>,
    /// 事件正文；只解析索引需要的最小字段集合。
    #[serde(default)]
    pub(super) payload: Option<RolloutPayload>,
}

/// 只声明 session、turn 与 token_count 所需字段，其余正文自动忽略。
#[derive(Deserialize)]
pub(super) struct RolloutPayload {
    /// 正文的事件子类型。
    #[serde(rename = "type", default)]
    pub(super) kind: Option<String>,
    /// `token_count` 事件携带的 Token 计数信息。
    #[serde(default)]
    pub(super) info: Option<TokenCountInfo>,
    /// 独立额度更新的存在证据；不保存额度正文。
    #[serde(default)]
    pub(super) rate_limits: Option<RateLimitsMarker>,
    /// 现代响应所属当前线程，用于核对 owner；不同于根 session_id。
    #[serde(default)]
    pub(super) thread_id: Option<String>,
    /// 现代单响应身份；只用于散列，不进入数据库或日志。
    #[serde(default)]
    pub(super) response_id: Option<String>,
    /// 现代单次事实；turn/thread 累计由 serde 跳过，不作为增量再相加。
    #[serde(default)]
    pub(super) usage: Option<RawTokenUsage>,
    /// 会话元数据 ID。
    #[serde(default)]
    pub(super) id: Option<String>,
    /// 会话 ID。
    #[serde(default, alias = "sessionId")]
    pub(super) session_id: Option<String>,
    /// fork 来源标记；只判断是否存在，不读取或保存原始 ID。
    #[serde(default)]
    pub(super) forked_from_id: Option<Value>,
    /// subagent 路径标记；只判断是否存在，不读取或保存原始路径。
    #[serde(default)]
    pub(super) agent_path: Option<Value>,
    /// 单次调用 ID。
    #[serde(default, alias = "callId")]
    pub(super) call_id: Option<String>,
    /// 所属回合 ID。
    #[serde(default, alias = "turnId")]
    pub(super) turn_id: Option<String>,
    /// 底层请求 ID。
    #[serde(default, alias = "requestId")]
    pub(super) request_id: Option<String>,
    /// 原始项目 ID；仅用于派生稳定项目键，不进入索引展示。
    #[serde(default, alias = "projectId")]
    pub(super) project_id: Option<String>,
    /// 原始项目键；仅用于派生稳定项目键，不进入索引展示。
    #[serde(default, alias = "projectKey")]
    pub(super) project_key: Option<String>,
    /// 原始工作目录；仅用于派生稳定项目键，不进入索引展示。
    #[serde(default)]
    pub(super) cwd: Option<String>,
    /// 智能体昵称。
    #[serde(default, alias = "agentNickname")]
    pub(super) agent_nickname: Option<String>,
    /// 线程展示名。
    #[serde(default, alias = "threadName")]
    pub(super) thread_name: Option<String>,
    /// 会话名。
    #[serde(default)]
    pub(super) name: Option<String>,
    /// 模型名。
    #[serde(default, alias = "modelName")]
    pub(super) model: Option<String>,
    /// 推理强度标签。
    #[serde(default, alias = "reasoningEffort", alias = "effort")]
    pub(super) reasoning_effort: Option<String>,
    /// payload 内嵌的时间戳原始值。
    #[serde(default)]
    pub(super) timestamp: Option<Value>,
    /// `task_started` 声明的任务起点，用于匹配当前 rollout 自身边界。
    #[serde(default, alias = "startedAt")]
    pub(super) started_at: Option<Value>,
}

/// 只验证额度更新是对象，不读取或保留额度字段内容。
pub(super) struct RateLimitsMarker;

impl<'de> Deserialize<'de> for RateLimitsMarker {
    /// 已识别的额度对象才足以说明 info:null 是正常额度更新。
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        /// 以 IgnoredAny 有界跳过普通行中的额度内容。
        struct MarkerVisitor;
        impl<'de> serde::de::Visitor<'de> for MarkerVisitor {
            type Value = RateLimitsMarker;
            /// 为非对象额度给出协议类型错误。
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("rate limits object")
            }
            /// 消费完整对象，只保留类型已确认的事实。
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Self::Value, M::Error> {
                while map
                    .next_entry::<serde::de::IgnoredAny, serde::de::IgnoredAny>()?
                    .is_some()
                {}
                Ok(RateLimitsMarker)
            }
        }
        deserializer.deserialize_map(MarkerVisitor)
    }
}

/// 只声明单次事实和累计回退两个 Token 对象。
#[derive(Deserialize)]
pub(super) struct TokenCountInfo {
    /// 本次单条事实的 Token 用量。
    #[serde(default, alias = "lastTokenUsage")]
    pub(super) last_token_usage: Option<RawTokenUsage>,
    /// 累计回退用的 Token 用量，`last_token_usage` 缺失时使用。
    #[serde(default, alias = "totalTokenUsage")]
    pub(super) total_token_usage: Option<RawTokenUsage>,
}

/// 只声明允许进入聚合索引的 Token 数值字段。
#[derive(Deserialize)]
pub(super) struct RawTokenUsage {
    /// 全部输入 Token（含缓存）。
    #[serde(default, alias = "inputTokens")]
    pub(super) input_tokens: Option<u64>,
    /// 缓存命中的输入 Token。
    #[serde(default, alias = "cachedInputTokens")]
    pub(super) cached_input_tokens: Option<u64>,
    /// 缓存写入的输入 Token。
    #[serde(default, alias = "cacheWriteInputTokens")]
    pub(super) cache_write_input_tokens: Option<u64>,
    /// 输出 Token。
    #[serde(default, alias = "outputTokens")]
    pub(super) output_tokens: Option<u64>,
    /// 推理输出 Token。
    #[serde(default, alias = "reasoningOutputTokens")]
    pub(super) reasoning_output_tokens: Option<u64>,
    /// 单次总 Token；缺失时由输入输出计算。
    #[serde(default, alias = "totalTokens")]
    pub(super) total_tokens: Option<u64>,
}
