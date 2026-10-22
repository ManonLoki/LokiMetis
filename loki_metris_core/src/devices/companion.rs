//! Watch经已认证iPhone报告自己的通信事实；没有独立TCP、IP或配对凭据。

use super::{valid_device_id, valid_device_name, link::LinkError};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// 当前认证快照广告后，手机才允许发送此扩展协议。
pub const WATCH_COMPANION_CAPABILITY: &str = "watchCompanionV1";
/// 年龄传输最多保留七天历史；更早事实省略，不伪造刚刚同步。
pub const MAX_OBSERVATION_AGE_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// 独立Watch应用身份，不接受手机IP、平台或密钥字段。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WatchInfo {
    /// Watch自己的安装身份。
    pub device_id: String,
    /// Watch设置中保存的名称。
    pub name: String,
    /// Watch应用版本。
    pub app_version: String,
}
impl WatchInfo {
    /// 元信息使用与手机相同的有界名称和版本合同，UUID必须独立有效。
    pub fn validate(&self) -> Result<(), LinkError> {
        if !valid_device_id(&self.device_id) || !valid_device_name(&self.name) || self.app_version.trim().is_empty()
            || self.app_version.len() > 64 || self.app_version.chars().any(char::is_control) { return Err(LinkError::Invalid); }
        Ok(())
    }
}

/// 实际回执的单调序号与发送时年龄；相同序号不能延长桌面证明。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompanionObservation {
    /// 同一手机owner内每次真实Watch回执递增，完整快照同步引用同一回执序号。
    pub sequence: u64,
    /// 手机本机单调时钟计算，不能用远端墙钟决定在线。
    pub age_ms: u64,
}

/// 每次报告最多一只系统配套Watch，来源资格绑定父手机当前认证租约。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WatchCompanionReport {
    /// 经过实际probe验证的手表身份。
    pub info: WatchInfo,
    /// false立即撤销；true仍须新回执及独立45秒有效期。
    pub reachable: bool,
    /// 最近实际通信，后台/失联不能用手机心跳代替。
    pub contact: Option<CompanionObservation>,
    /// 完整快照校验并接纳到当前前台内存后的 ACK，缓存命中不能生成新同步事实。
    pub sync: Option<CompanionObservation>,
}
impl WatchCompanionReport {
    /// 拒绝无证明在线、超龄、零序号及同步晚于通信的矛盾报告。
    pub fn validate(&self) -> Result<(), LinkError> {
        self.info.validate()?;
        for observation in [self.contact, self.sync].into_iter().flatten() {
            if observation.sequence == 0 || observation.sequence > 9_007_199_254_740_991 || observation.age_ms > MAX_OBSERVATION_AGE_MS { return Err(LinkError::Invalid); }
        }
        if self.reachable && !self.contact.is_some_and(|contact| contact.age_ms < 45_000) { return Err(LinkError::Invalid); }
        if let Some(sync) = self.sync {
            if !self.contact.is_some_and(|contact| sync.sequence <= contact.sequence && sync.age_ms >= contact.age_ms) { return Err(LinkError::Invalid); }
        }
        Ok(())
    }
}

/// 无IP的子设备页面投影；父会话不可达时不能独立显示可通信。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchCompanionView {
    /// Watch自己的名称、ID及版本。
    pub info: WatchInfo,
    /// 父连接资格和Watch实际回执共同决定。
    pub reachable: bool,
    /// 桌面本机单调年龄。
    pub since_last_contact: Option<Duration>,
    /// 完整快照被 Watch 确认接纳的最近年龄。
    pub since_last_sync: Option<Duration>,
}

/// 当前父代次内的Watch记录，不能写入配对账本或继承到新连接。
#[derive(Clone)]
pub(super) struct CompanionRecord {
    info: WatchInfo,
    available: bool,
    contact: Option<Observed>,
    sync: Option<Observed>,
    applied_at: Instant,
}
/// 锚与初始年龄分开保存，桌面刚开机也能表示更久的历史而不减穿单调时钟原点。
#[derive(Clone, Copy)]
struct Observed { sequence: u64, anchor: Instant, initial_age: Duration }
impl Observed {
    /// 任一时间倒退或加法溢出都不能产生新鲜证明。
    fn age(self, now: Instant) -> Option<Duration> { self.initial_age.checked_add(now.checked_duration_since(self.anchor)?) }
}
impl CompanionRecord {
    /// 身份匹配不构造页面快照，父租约在换表时仍保留全局回执高水位。
    pub(super) fn is_for(&self, device_id: &str) -> bool { self.info.device_id == device_id }
    /// 新身份先建立空状态，再按相同严格合同应用首份报告。
    pub(super) fn new(report: &WatchCompanionReport, anchor: Instant, now: Instant) -> Result<Self, LinkError> {
        let mut value = Self { info: report.info.clone(), available: false, contact: None, sync: None, applied_at: now };
        value.apply(report, anchor, now)?; Ok(value)
    }
    /// 同序号不续时效，撤销后必须新回执才可恢复；失败不修改已有记录。
    pub(super) fn apply(&mut self, report: &WatchCompanionReport, anchor: Instant, now: Instant) -> Result<(), LinkError> {
        report.validate()?;
        if report.info.device_id != self.info.device_id || now < self.applied_at { return Err(LinkError::Invalid); }
        let mut next = self.clone();
        let previous = next.contact.map(|value| value.sequence);
        Self::observe(&mut next.contact, report.contact, anchor)?;
        Self::observe(&mut next.sync, report.sync, anchor)?;
        if !report.reachable { next.available = false; }
        else if report.contact.is_some_and(|value| previous.is_none_or(|old| value.sequence > old)) { next.available = true; }
        next.info = report.info.clone(); next.applied_at = now; *self = next; Ok(())
    }
    /// 较旧序号失败，相同序号忽略年龄；以先于手机发送的桌面ping作保守时间锚，包含排队延迟。
    fn observe(saved: &mut Option<Observed>, report: Option<CompanionObservation>, anchor: Instant) -> Result<(), LinkError> {
        let Some(report) = report else { return Ok(()); };
        if let Some(previous) = saved {
            if report.sequence < previous.sequence { return Err(LinkError::Stale); }
            if report.sequence == previous.sequence { return Ok(()); }
        }
        *saved = Some(Observed { sequence: report.sequence, anchor, initial_age: Duration::from_millis(report.age_ms) }); Ok(())
    }
    /// 父心跳不会进入此方法；即使定时器未调度也直接投影Watch独立超时。
    pub(super) fn snapshot(&self, parent_reachable: bool, now: Instant) -> WatchCompanionView {
        let contact = self.contact.and_then(|value| value.age(now));
        WatchCompanionView { info: self.info.clone(), reachable: parent_reachable && now >= self.applied_at && self.available && contact.is_some_and(|age| age < Duration::from_secs(45)),
            since_last_contact: contact, since_last_sync: self.sync.and_then(|value| value.age(now)) }
    }
}
