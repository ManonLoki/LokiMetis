//! 在旧快照与现代响应事实共存时，固定 Codex 单次用量的权威来源。

use serde::{Deserialize, Serialize};

use super::{
    IncrementalTokenUsageDecision, SelectedTokenUsage, TokenUsage, TokenUsageError,
    select_incremental_call_usage, select_single_call_usage,
};

/// 单个 rollout 自身区段采用的事实协议；复制前缀不得推进此状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexUsageAuthority {
    /// 旧客户端只有 token_count，沿用累计去膨胀与兼容回退。
    #[default]
    LegacySnapshots,
    /// 已证明客户端提供逐响应事实，旧快照仅为兼容或上下文估算。
    ResponseRecords,
}

impl CodexUsageAuthority {
    /// 接收已校验且属于当前线程的单响应事实，并切换后续权威来源。
    /// 上游 turn/thread 累计不参与此操作，压缩请求同样计入单次事实。
    pub fn observe_response(&mut self, usage: TokenUsage) -> SelectedTokenUsage {
        *self = Self::ResponseRecords;
        select_single_call_usage(Some(usage), None).expect("response supplied single-call usage")
    }

    /// 表示旧 token_count 是否已成为不能再计费的兼容影子。
    pub const fn ignores_legacy_shadow(self) -> bool {
        matches!(self, Self::ResponseRecords)
    }

    /// 对仍使用旧协议的区段应用既有增量选择规则；现代区段不重复计快照。
    pub fn select_legacy(
        self,
        last: Option<TokenUsage>,
        cumulative: Option<TokenUsage>,
        previous_cumulative: Option<&TokenUsage>,
        previous_last: Option<&TokenUsage>,
    ) -> Result<IncrementalTokenUsageDecision, TokenUsageError> {
        if self.ignores_legacy_shadow() {
            return Ok(IncrementalTokenUsageDecision::IgnoreNonIncremental);
        }
        select_incremental_call_usage(last, cumulative, previous_cumulative, previous_last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 现代单次事实不减累计，随后相同或变化的旧快照均不能再次计费。
    #[test]
    fn modern_authority_preserves_each_response_and_ignores_legacy() {
        let usage = TokenUsage::new(100, 0, Some(0), 20, 0, Some(120)).unwrap();
        let mut authority = CodexUsageAuthority::default();
        assert!(matches!(
            authority
                .select_legacy(Some(usage.clone()), None, None, None)
                .unwrap(),
            IncrementalTokenUsageDecision::Emit(_)
        ));
        assert_eq!(
            authority.observe_response(usage.clone()).usage.total_tokens,
            120
        );
        assert_eq!(
            authority.observe_response(usage.clone()).usage.total_tokens,
            120
        );
        assert_eq!(
            authority
                .select_legacy(Some(usage), None, None, None)
                .unwrap(),
            IncrementalTokenUsageDecision::IgnoreNonIncremental
        );
        assert_eq!(
            authority.select_legacy(None, None, None, None).unwrap(),
            IncrementalTokenUsageDecision::IgnoreNonIncremental
        );
    }

    /// 检查点必须显式保存阶段；未知阶段不能按旧格式默认为可信。
    #[test]
    fn authority_round_trip_rejects_unknown_stage() {
        for authority in [
            CodexUsageAuthority::LegacySnapshots,
            CodexUsageAuthority::ResponseRecords,
        ] {
            let encoded = serde_json::to_string(&authority).unwrap();
            assert_eq!(
                serde_json::from_str::<CodexUsageAuthority>(&encoded).unwrap(),
                authority
            );
        }
        assert!(serde_json::from_str::<CodexUsageAuthority>("\"future\"").is_err());
    }
}
