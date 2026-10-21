//! 提供商 5 小时与周用量窗口。只解释上游响应，不读取凭据、不发起网络。
//!
//! Claude 与 WorkBuddy 不在活动目录内。窗口只按持续时长归类：约 5 小时或约 7 日才进入对应槽，
//! 月度账单不会被标成周用量。

mod parse;

use crate::AiTool;

/// 用量正文的提供商身份；实际读取范围由活动目录限制。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuotaWindow {
    /// 窗口种类。
    pub kind: QuotaWindowKind,
    /// 已用比例，0..=1000。
    pub used_tenths: u16,
    /// 窗口重置的 Unix 秒；上游没给则为空。
    pub reset_at_unix: Option<i64>,
}

/// 一次成功解释的提供商用量。两个窗口都可以为空。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderQuota {
    /// 提供商。
    pub provider: QuotaProvider,
    /// 已消毒的短方案名，不含邮箱。
    pub plan: Option<String>,
    /// 5 小时窗口。
    pub five_hour: Option<QuotaWindow>,
    /// 周窗口。
    pub weekly: Option<QuotaWindow>,
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
    /// 只有真实短窗口才具备额度卡展示资格，周窗口不能替代五小时窗口。
    pub const fn has_five_hour_window(&self) -> bool {
        matches!(self.five_hour, Some(QuotaWindow { kind: QuotaWindowKind::FiveHour, .. }))
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
