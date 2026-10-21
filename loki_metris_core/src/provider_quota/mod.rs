//! 提供商 5 小时与周用量窗口。只解释上游响应，不读取凭据、不发起网络。
//!
//! Claude 与 WorkBuddy 不在活动目录内。窗口只按持续时长归类：约 5 小时或约 7 日才进入对应槽，
//! 月度账单不会被标成周用量。

mod parse;

use crate::AiTool;
use serde::{Deserialize, Serialize};

/// 用量正文的提供商身份；实际读取范围由活动目录限制。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuotaProvider {
    /// Codex 订阅额度。
    Codex,
    /// Cursor 订阅额度。
    Cursor,
    /// Grok Build 订阅额度。
    Grok,
    /// 保留已有 WorkBuddy / CodeBuddy 正文解释，不进入活动读取目录。
    WorkBuddy,
}

/// 用量窗口种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QuotaWindowKind {
    /// 约 5 小时的短窗口。
    FiveHour,
    /// 约 7 日的周窗口。
    Weekly,
}

/// 读取或解释用量失败的稳定类别。不含路径、正文或凭据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuotaFailure {
    /// 本机没有可用的登录材料。
    MissingCredentials,
    /// 登录材料存在，但被宿主加密，当前无法解开。
    CredentialLocked,
    /// 上游拒绝了登录材料。
    Unauthorized,
    /// 读取程序缺失、超时或上游不可达。
    Transport,
    /// 上游返回了无法解释的正文。
    Malformed,
}

/// 一个已归类的用量窗口。已用比例以千分比保存，1000 表示 100%。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuotaWindow {
    /// 窗口种类。
    pub kind: QuotaWindowKind,
    /// 已用比例，0..=1000。
    pub used_tenths: u16,
    /// 窗口重置的 Unix 秒；上游没给则为空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_at_unix: Option<i64>,
}

/// 一次成功解释的提供商用量。两个窗口都可以为空。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderQuota {
    /// 提供商。
    pub provider: QuotaProvider,
    /// 已消毒的短方案名，不含邮箱。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    /// 5 小时窗口。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub five_hour: Option<QuotaWindow>,
    /// 周窗口。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weekly: Option<QuotaWindow>,
}

/// 终端只接收有界额度投影，不携带提供商登录材料或读取错误正文。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderQuotaSnapshot {
    /// 桌面最近一次读取完成时间，Unix秒，不代表终端查询发起时重新请求过上游。
    pub observed_at_unix: i64,
    /// 按活动目录排序，空列表明确撤下之前的额度。
    pub providers: Vec<ProviderQuota>,
}

impl ProviderQuotaSnapshot {
    /// 仅投影有效窗口并按活动目录排序；尚未完成读取时不伪造空结果。
    pub fn project(observed_at_unix: Option<i64>, quotas: &[ProviderQuota]) -> Option<Self> {
        let observed_at_unix = observed_at_unix.filter(|time| *time >= 0)?;
        let providers = supported_quota_providers().into_iter().filter_map(|provider| {
            quotas.iter().find(|quota| quota.provider == provider && quota.valid_for_terminal()).cloned()
        }).collect();
        Some(Self { observed_at_unix, providers })
    }

    /// 网络/缓存接收端共用值域：有序唯一提供商、正确窗口类型和整数千分比。
    pub fn validate(&self) -> Result<(), QuotaFailure> {
        let catalog = supported_quota_providers();
        let mut previous = None;
        if self.observed_at_unix < 0 || self.providers.len() > catalog.len() { return Err(QuotaFailure::Malformed); }
        for quota in &self.providers {
            let index = catalog.iter().position(|provider| *provider == quota.provider).ok_or(QuotaFailure::Malformed)?;
            if previous.is_some_and(|old| old >= index) || !quota.valid_for_terminal() { return Err(QuotaFailure::Malformed); }
            previous = Some(index);
        }
        Ok(())
    }
}

impl QuotaProvider {
    /// 对应的监控工具。
    pub const fn ai_tool(self) -> AiTool {
        match self {
            Self::Codex => AiTool::Codex,
            Self::Cursor => AiTool::Cursor,
            Self::Grok => AiTool::Grok,
            Self::WorkBuddy => AiTool::WorkBuddy,
        }
    }

    /// 稳定的界面标识，只含小写字母。
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Cursor => "cursor",
            Self::Grok => "grok",
            Self::WorkBuddy => "workbuddy",
        }
    }
}

impl QuotaWindow {
    /// 剩余比例的千分比。
    pub const fn remaining_tenths(self) -> u16 {
        1000 - self.used_tenths
    }
}

impl ProviderQuota {
    /// 任一真实额度窗口都能展示；缺少短窗不能遮掉已有周额度。
    pub const fn has_usage_window(&self) -> bool {
        matches!(self.five_hour, Some(QuotaWindow { kind: QuotaWindowKind::FiveHour, .. }))
            || matches!(self.weekly, Some(QuotaWindow { kind: QuotaWindowKind::Weekly, .. }))
    }

    /// 上游不完整值和不可信方案文本不能进入跨端投影。
    fn valid_for_terminal(&self) -> bool {
        self.has_usage_window()
            && [(self.five_hour, QuotaWindowKind::FiveHour), (self.weekly, QuotaWindowKind::Weekly)].into_iter().all(|(window, kind)| {
                window.is_none_or(|value| value.kind == kind && value.used_tenths <= 1000 && value.reset_at_unix.is_none_or(|time| time >= 0))
            })
            && self.plan.as_ref().is_none_or(|plan| !plan.is_empty() && plan.len() <= 24
                && plan.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '_' | '-' | '.')))
    }
}

/// 活动读取顺序为 Codex、Cursor、Grok；Claude 缺少本地额度来源，WorkBuddy 已排除。
pub const fn supported_quota_providers() -> [QuotaProvider; 3] {
    [
        QuotaProvider::Codex,
        QuotaProvider::Cursor,
        QuotaProvider::Grok,
    ]
}

/// 把上游正文解释成 5 小时与周窗口。
pub fn parse_provider_quota(provider: QuotaProvider, body: &str) -> Result<ProviderQuota, QuotaFailure> {
    parse::parse_provider_quota(provider, body)
}

/// 按持续秒数归类。落在边界外的月度或其它周期返回空。
pub const fn classify_window_seconds(seconds: i64) -> Option<QuotaWindowKind> {
    if seconds >= 14_400 && seconds <= 21_600 {
        Some(QuotaWindowKind::FiveHour)
    } else if seconds >= 518_400 && seconds <= 691_200 {
        Some(QuotaWindowKind::Weekly)
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
