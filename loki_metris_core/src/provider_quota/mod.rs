//! 提供商 5 小时、周与月用量窗口。只解释上游响应，不读取凭据、不发起网络。
//!
//! WorkBuddy 不在活动目录内。窗口优先按明确时长归类，缺失时仅采用提供商已知的窗口约定；
//! 月度账单不会被标成周用量。Claude 只来自 Claude Code statusline 交给本机的 `rate_limits`。

mod parse;

use crate::AiTool;
use serde::{Deserialize, Serialize};

/// 用量正文的提供商身份；实际读取范围由活动目录限制。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuotaProvider {
    /// Codex 订阅额度。
    Codex,
    /// Claude 订阅额度，来自 Claude Code statusline。
    Claude,
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
    /// 约一个账单月的窗口。
    Monthly,
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

/// 一次成功解释的提供商用量。各窗口都可以为空。
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
    /// 账单月窗口。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monthly: Option<QuotaWindow>,
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
    /// 终端额度仅展示设置中启用的Agent，保留观测时间与真实窗口；全关闭时显式清空。
    pub fn for_enabled_agents(mut self, enabled: &[AiTool]) -> Self {
        self.providers.retain(|quota| enabled.contains(&quota.provider.ai_tool()));
        self
    }

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
            Self::Claude => AiTool::ClaudeCode,
            Self::Cursor => AiTool::Cursor,
            Self::Grok => AiTool::Grok,
            Self::WorkBuddy => AiTool::WorkBuddy,
        }
    }

    /// 稳定的界面标识，只含小写字母。
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
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
    /// 任一真实额度窗口都能展示；缺少短窗不能遮掉已有周或月额度。
    pub const fn has_usage_window(&self) -> bool {
        matches!(self.five_hour, Some(QuotaWindow { kind: QuotaWindowKind::FiveHour, .. }))
            || matches!(self.weekly, Some(QuotaWindow { kind: QuotaWindowKind::Weekly, .. }))
            || matches!(self.monthly, Some(QuotaWindow { kind: QuotaWindowKind::Monthly, .. }))
    }

    /// 展示顺序固定为 5 小时、周、月，缺失窗口跳过。
    pub fn windows(&self) -> impl Iterator<Item = QuotaWindow> {
        [self.five_hour, self.weekly, self.monthly].into_iter().flatten()
    }

    /// 本机缓存的窗口过了重置时刻就不再代表当前额度，丢弃后由调用方判断是否仍有窗口。
    pub fn drop_expired_windows(&mut self, now_unix: i64) {
        for slot in [&mut self.five_hour, &mut self.weekly, &mut self.monthly] {
            if slot.is_some_and(|window| window.reset_at_unix.is_some_and(|reset| reset <= now_unix)) {
                *slot = None;
            }
        }
    }

    /// 上游不完整值和不可信方案文本不能进入跨端投影。
    fn valid_for_terminal(&self) -> bool {
        self.has_usage_window()
            && [(self.five_hour, QuotaWindowKind::FiveHour), (self.weekly, QuotaWindowKind::Weekly), (self.monthly, QuotaWindowKind::Monthly)].into_iter().all(|(window, kind)| {
                window.is_none_or(|value| value.kind == kind && value.used_tenths <= 1000 && value.reset_at_unix.is_none_or(|time| time >= 0))
            })
            && self.plan.as_ref().is_none_or(|plan| !plan.is_empty() && plan.len() <= 24
                && plan.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '_' | '-' | '.')))
    }
}

/// 活动读取顺序为 Codex、Claude、Cursor、Grok；WorkBuddy 已排除。
pub const fn supported_quota_providers() -> [QuotaProvider; 4] {
    [
        QuotaProvider::Codex,
        QuotaProvider::Claude,
        QuotaProvider::Cursor,
        QuotaProvider::Grok,
    ]
}

/// 把上游正文解释成 5 小时、周与月窗口。
pub fn parse_provider_quota(provider: QuotaProvider, body: &str) -> Result<ProviderQuota, QuotaFailure> {
    parse::parse_provider_quota(provider, body)
}

/// 按持续秒数归类。账单月按 27～32 日容纳 2 月和时区偏差，其它周期返回空。
pub const fn classify_window_seconds(seconds: i64) -> Option<QuotaWindowKind> {
    if seconds >= 14_400 && seconds <= 21_600 {
        Some(QuotaWindowKind::FiveHour)
    } else if seconds >= 518_400 && seconds <= 691_200 {
        Some(QuotaWindowKind::Weekly)
    } else if seconds >= 2_332_800 && seconds <= 2_764_800 {
        Some(QuotaWindowKind::Monthly)
    } else {
        None
    }
}

/// Claude Code statusline 输入里只保留 `rate_limits` 的 5 小时和 7 日窗口，形成可落盘的最小记录。
///
/// 没有订阅额度、正文过大或格式不对时返回空，调用方保留上一份记录。记录不含会话、路径或模型等信息。
pub fn claude_statusline_quota_record(input: &[u8]) -> Option<Vec<u8>> {
    parse::claude_statusline_record(input)
}

/// statusline 记录在 Hook relay 私有目录下的文件名。
pub const CLAUDE_STATUSLINE_QUOTA_FILENAME: &str = "claude-rate-limits.json";

#[cfg(test)]
mod tests;
