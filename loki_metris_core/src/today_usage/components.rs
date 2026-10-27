//! 今日摘要的部分可用回退；严格联合查询仍保留原索引状态与分页口径。

use super::{TodayAvailability, TodayUsageSnapshot};
use crate::{AiTool, Completeness, aggregate::lower_confidence};

impl TodayUsageSnapshot {
    /// 联合索引被未就绪成员阻断时，保留同轮独立 Agent 的有效部分和。
    /// 不混入跨日、跨来源、迟到或重复分量；错误不补零，部分和永不具备通知资格。
    pub fn with_available_components(mut self, components: &[(AiTool, Self)]) -> Option<Self> {
        let mut seen = std::collections::HashSet::new();
        if components.iter().any(|(agent, value)| {
            !seen.insert(*agent) || value.day != self.day || value.source != self.source
                || value.observed_ms != self.observed_ms
        }) {
            return None;
        }
        if self.availability != TodayAvailability::NoRecords {
            return Some(self);
        }
        if components.iter().any(|(_, value)| value.availability == TodayAvailability::Unavailable) {
            self.availability = TodayAvailability::Unavailable;
            return Some(self);
        }
        let mut found = false;
        let mut total = 0_u64;
        let mut completeness = if self.completeness == Completeness::Unknown {
            Completeness::Unknown
        } else {
            Completeness::Partial
        };
        let mut confidence = self.confidence;
        for (_, value) in components {
            if value.availability != TodayAvailability::Available {
                continue;
            }
            found = true;
            total = total.checked_add(value.total_tokens)?;
            if value.completeness == Completeness::Unknown {
                completeness = Completeness::Unknown;
            }
            confidence = lower_confidence(confidence, value.confidence);
        }
        if found {
            self.total_tokens = total;
            self.availability = TodayAvailability::Available;
            self.completeness = completeness;
            self.confidence = confidence;
        }
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Confidence, UsageDay};

    /// 创建同日同来源事实；缺失分量故意保留旧数字，以验证不会参与求和。
    fn snapshot(tokens: u64, availability: TodayAvailability) -> TodayUsageSnapshot {
        let now = 1_790_000_000_000;
        TodayUsageSnapshot { day: UsageDay::capture(now, &jiff::tz::TimeZone::UTC).unwrap(),
            source: "selection".into(), total_tokens: tokens, observed_ms: now, availability,
            completeness: Completeness::Complete, confidence: Confidence::Exact }
    }

    /// 有效零值与正数均保留，空或待重扫成员不污染部分和，也不能触发通知。
    #[test]
    fn unready_member_keeps_available_components_and_wire_amount() {
        for tokens in [0, 110] {
            let all = snapshot(0, TodayAvailability::NoRecords);
            let components = vec![(AiTool::Codex, snapshot(tokens, TodayAvailability::Available)),
                (AiTool::ClaudeCode, snapshot(999, TodayAvailability::NoRecords))];
            let value = all.with_available_components(&components).unwrap();
            assert_eq!(value.total_tokens, tokens);
            assert_eq!(value.availability, TodayAvailability::Available);
            assert_eq!(value.completeness, Completeness::Partial);
            assert!(!value.eligible(value.observed_ms, &value.day, &value.source));
            let wire = crate::devices::link::data::TokenSnapshot::from_observation(&value, &components).unwrap();
            assert_eq!(wire.all.total, Some(tokens.to_string()));
            assert_eq!(wire.agents[1].amount.total, None);
        }
    }

    /// 各独立来源累加但保留最弱质量，全部缺失时仍无数字，真实失败优先不可用。
    #[test]
    fn partial_sum_preserves_quality_and_failure_boundaries() {
        let absent = snapshot(0, TodayAvailability::NoRecords);
        let mut derived = snapshot(25, TodayAvailability::Available);
        derived.confidence = Confidence::Derived;
        derived.completeness = Completeness::Unknown;
        let components = vec![(AiTool::Codex, snapshot(110, TodayAvailability::Available)),
            (AiTool::WorkBuddy, derived)];
        let sum = absent.clone().with_available_components(&components).unwrap();
        assert_eq!(sum.total_tokens, 135);
        assert_eq!(sum.completeness, Completeness::Unknown);
        assert_eq!(sum.confidence, Confidence::Derived);
        let mut unknown = absent.clone();
        unknown.completeness = Completeness::Unknown;
        unknown.confidence = Confidence::Suspected;
        let sum = unknown.with_available_components(&components).unwrap();
        assert_eq!(sum.completeness, Completeness::Unknown);
        assert_eq!(sum.confidence, Confidence::Suspected);
        assert_eq!(absent.clone().with_available_components(&[(AiTool::ClaudeCode, absent.clone())]).unwrap(), absent);
        assert_eq!(absent.clone().with_available_components(&[]).unwrap(), absent);
        for availability in [TodayAvailability::Available, TodayAvailability::Unavailable] {
            let original = snapshot(456, availability);
            assert_eq!(original.clone().with_available_components(&components).unwrap(), original);
        }
        let failed = vec![(AiTool::Codex, snapshot(110, TodayAvailability::Available)),
            (AiTool::ClaudeCode, snapshot(0, TodayAvailability::Unavailable))];
        assert_eq!(absent.with_available_components(&failed).unwrap().availability, TodayAvailability::Unavailable);
    }

    /// 拒绝混合观察、重复成员及整数溢出，防止把错误分量包装成可用总量。
    #[test]
    fn partial_sum_rejects_inconsistent_or_overflowing_components() {
        let all = snapshot(0, TodayAvailability::NoRecords);
        let valid = snapshot(110, TodayAvailability::Available);
        for case in 0..4 {
            let mut changed = valid.clone();
            match case {
                0 => changed.day.date = "other-day".into(),
                1 => changed.day.zone = "other-zone".into(),
                2 => changed.observed_ms += 1,
                _ => changed.source = "other-selection".into(),
            }
            assert!(all.clone().with_available_components(&[(AiTool::Codex, changed)]).is_none());
        }
        assert!(all.clone().with_available_components(&[(AiTool::Codex, valid.clone()), (AiTool::Codex, valid)]).is_none());
        assert!(all.with_available_components(&[(AiTool::Codex, snapshot(u64::MAX, TodayAvailability::Available)),
            (AiTool::WorkBuddy, snapshot(1, TodayAvailability::Available))]).is_none());
    }
}
