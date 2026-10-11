//! 定义跨 adapter 共用的本机 provider、scope 与带质量元数据的事实信封。
//! 任何“可能缺失/不确定”的数据都要用类型强制表达出来，杜绝用 0 或空字符串默默代表“未知”。

use crate::{TokenUsage, TotalTokenAccounting};
use serde::{Deserialize, Serialize};

/// 按 provider 能力返回零窗口空单位元。
pub fn empty_token_usage_for_provider(provider: ProviderKind) -> TokenUsage {
    match provider {
        ProviderKind::ClaudeTranscriptJsonl => {
            TokenUsage::zero_with_component_availability(true, true, false)
        }
        ProviderKind::CombinedLocalAgents => TokenUsage::zero(),
        ProviderKind::GrokSessionJsonl => TokenUsage::zero(),
        ProviderKind::RolloutJsonl => TokenUsage::zero(),
        ProviderKind::WorkbuddyProjectJsonl => {
            TokenUsage::zero_with_component_availability(true, false, false)
        }
    }
}

/// 标识指标事实来自哪一类本机记录。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind {
    /// 从用户授权数据根中的 rollout JSONL 观察到的事实。
    RolloutJsonl,
    /// 从用户授权 Claude Code 数据根中的 transcript JSONL 观察到的事实。
    ClaudeTranscriptJsonl,
    /// 从用户授权 Grok home 中的 `sessions/**/updates.jsonl` 观察到的事实。
    GrokSessionJsonl,
    /// 从多个已开启本机 Agent 的独立 provider 联合得到的只读事实。
    CombinedLocalAgents,
    /// 从用户授权 WorkBuddy 本机根中的项目 JSONL 观察到的逐请求事实。
    WorkbuddyProjectJsonl,
}

impl ProviderKind {
    /// 返回本 provider 的本机调用总 Token 聚合口径。
    pub const fn local_total_token_accounting(self) -> TotalTokenAccounting {
        match self {
            Self::RolloutJsonl
            | Self::ClaudeTranscriptJsonl
            | Self::GrokSessionJsonl
            | Self::CombinedLocalAgents
            | Self::WorkbuddyProjectJsonl => TotalTokenAccounting::Observed,
        }
    }

    /// 返回来源根列表中使用的本地环境标签。
    pub const fn source_environment_label(self) -> &'static str {
        match self {
            Self::RolloutJsonl => "当前 CODEX_HOME",
            Self::ClaudeTranscriptJsonl => "当前 CLAUDE_CONFIG_DIR",
            Self::GrokSessionJsonl => "当前 GROK_HOME",
            Self::CombinedLocalAgents => "已开启 Agent 的独立数据根",
            Self::WorkbuddyProjectJsonl => "当前 WORKBUDDY_HOME",
        }
    }

    /// 返回本 provider 在本机快照中的解析器来源标签。
    pub fn parser_source_label(self, parser_version: u32) -> String {
        match self {
            Self::RolloutJsonl => format!("rollout-parser-v{parser_version}"),
            Self::ClaudeTranscriptJsonl => {
                format!("claude-transcript-parser-v{parser_version}")
            }
            Self::GrokSessionJsonl => format!("grok-session-parser-v{parser_version}"),
            Self::CombinedLocalAgents => format!("combined-local-agents-v{parser_version}"),
            Self::WorkbuddyProjectJsonl => {
                format!("workbuddy-project-jsonl-v{parser_version}")
            }
        }
    }
}

/// 标识指标覆盖的业务范围，禁止不同范围的数值自动混加。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MetricScope {
    /// 当前设备中已经索引并去重的可观察记录。
    DeviceObserved,
    /// 单一用户授权数据根内可观察的记录。
    RootObserved,
    /// 单一线程内可观察的记录。
    ThreadObserved,
}

/// 描述事实相对其刷新策略的时效状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Freshness {
    /// 事实仍处于 provider 声明的有效期内。
    Fresh,
    /// 事实来自最近成功结果，但已经超过正常刷新间隔。
    Stale,
    /// 事实已经超过允许展示为当前值的最长有效期。
    Expired,
    /// adapter 无法确定上游时间语义。
    Unknown,
}

/// 描述事实覆盖是否完整，避免把部分扫描冒充完整总量。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Completeness {
    /// provider 声明本次范围完整且没有已知跳过项。
    Complete,
    /// 存在权限、取消、格式漂移或其他可见跳过项。
    Partial,
    /// provider 无法证明覆盖边界。
    Unknown,
}

/// 描述指标值是直接事实、受控推算还是疑似冲突结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Confidence {
    /// 值直接来自明确的上游单次事实或无冲突聚合。
    Exact,
    /// 值由批准的回退规则推算，并且必须向用户标注。
    Derived,
    /// 值存在无法消除的重复或格式冲突，只能谨慎展示。
    Suspected,
}

/// 包装一个带来源、范围和数据质量元数据的业务事实。
// `<T>` 泛型：MetricFact 是一个通用“信封”，可以装 u64 数量或本机聚合等
// 任意具体业务值，外面统一附带 provider/scope/freshness/confidence 等质量元信息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricFact<T> {
    /// 经过 adapter 规范化的业务值。
    pub value: T,
    /// 产生该值的 provider。
    pub provider: ProviderKind,
    /// 该值覆盖的业务范围。
    pub scope: MetricScope,
    /// 观测完成时的 Unix 毫秒时间戳。
    pub observed_at_epoch_ms: i64,
    /// 相对 provider 刷新策略的时效状态。
    pub freshness: Freshness,
    /// 已知覆盖边界的完整程度。
    pub completeness: Completeness,
    /// 值的事实或推算置信度。
    pub confidence: Confidence,
    /// 不含敏感内容的来源协议或解析器版本。
    pub source_version: Option<String>,
}

impl<T> MetricFact<T> {
    /// 创建一个字段齐全的事实，调用方必须显式提供 scope 与质量元数据。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        value: T,
        provider: ProviderKind,
        scope: MetricScope,
        observed_at_epoch_ms: i64,
        freshness: Freshness,
        completeness: Completeness,
        confidence: Confidence,
        source_version: Option<String>,
    ) -> Self {
        Self {
            value,
            provider,
            scope,
            observed_at_epoch_ms,
            freshness,
            completeness,
            confidence,
            source_version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 三个本机 provider 与联合视图均保留上游 total，避免输入含缓存时总量反而更小。
    #[test]
    fn local_providers_all_use_observed_total_accounting() {
        for provider in [
            ProviderKind::RolloutJsonl,
            ProviderKind::ClaudeTranscriptJsonl,
            ProviderKind::GrokSessionJsonl,
            ProviderKind::CombinedLocalAgents,
        ] {
            assert_eq!(
                provider.local_total_token_accounting(),
                TotalTokenAccounting::Observed
            );
        }
    }
}
