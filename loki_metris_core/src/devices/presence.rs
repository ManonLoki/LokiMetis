//! 在线仅表示认证应用会话当前可通信；不把心跳超时解释为设备物理关机。

use super::{DeviceBook, is_lan_endpoint, MAX_SAVED_DEVICES, link::{AuthenticatedLinkSession, DeviceInfo, LinkError, PresenceTrust}};
use std::{collections::BTreeMap, net::SocketAddr, time::{Duration, Instant}};

/// 应用运行且网络可达时的心跳周期；后台挂起不承诺持续保活。
pub const DEVICE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
/// 连续45秒无认证入站消息则会话失效，之后必须重新握手。
pub const DEVICE_HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(45);

/// 暂不可达原因属于应用通信状态，不推断手机电源或网络硬件状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnavailableReason {
    /// socket关闭、网络切换或传输失败。
    Disconnected,
    /// 对端主动告知暂停同步。
    Background,
    /// 未在期限内收到认证消息。
    HeartbeatTimeout,
    /// 调用方提供的单调时间早于最后通信，不能证明当前仍可达。
    TimeUnknown,
}

/// 页面可观察的可信连接状态，独立于未经认证的发现广告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceConnectionState {
    /// 本进程尚无该设备的认证通信。
    NotConnected,
    /// 当前认证会话仍在心跳期限内。
    Reachable,
    /// 曾连接但当前不可通信，保留缓存与最近通信信息。
    Unavailable(UnavailableReason),
}

/// 不含秘密的设备状态投影；peer仅表示最近认证socket的对端，不作为稳定身份。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevicePresenceView {
    /// 已保存设备身份。
    pub device_id: String,
    /// 只有认证连接能更新的当前名称与平台。
    pub info: Option<DeviceInfo>,
    /// 当前通信状态。
    pub state: DeviceConnectionState,
    /// 最近认证socket的实际地址；不可达时作为历史信息保留。
    pub peer: Option<SocketAddr>,
    /// 距本机收到最后认证消息的时间，不使用远端时钟计算。
    pub since_last_contact: Option<Duration>,
}

/// 私有代次防止旧socket的迟到心跳或关闭事件影响重新连接的同一设备。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresenceLease { device_id: String, generation: u64 }
impl PresenceLease {
    /// 仅供连接owner按稳定身份查找对应的有界任务。
    pub fn device_id(&self) -> &str { &self.device_id }
}

/// 单设备进程内通信记录，不写入配对账本或配置。
struct Entry { info: DeviceInfo, peer: SocketAddr, generation: u64, last_contact: Instant, unavailable: Option<UnavailableReason>, trust: PresenceTrust }
/// 所有已保存终端的进程内权威连接状态；全局代次在忘记设备时也不回退。
#[derive(Default)]
pub struct DevicePresence { entries: BTreeMap<String, Entry>, generation: u64 }

impl DevicePresence {
    /// 认证会话、当前账本、实际局域网socket和有效元信息共同建立一条新租约。
    pub fn connected(&mut self, book: &DeviceBook, session: &mut AuthenticatedLinkSession, info: DeviceInfo, peer: SocketAddr, now: Instant) -> Result<PresenceLease, LinkError> {
        if !session.authorized_by(book) { return Err(LinkError::Untrusted); }
        info.validate_for(session.device_id())?;
        if !is_lan_endpoint(peer) { return Err(LinkError::Invalid); }
        if self.entries.len() >= MAX_SAVED_DEVICES && !self.entries.contains_key(session.device_id()) { return Err(LinkError::Limit); }
        let generation = self.generation.checked_add(1).ok_or(LinkError::Limit)?;
        let trust = session.claim_presence(book)?;
        self.generation = generation;
        self.entries.insert(info.device_id.clone(), Entry { info, peer, generation, last_contact: now, unavailable: None, trust });
        Ok(PresenceLease { device_id: session.device_id().into(), generation })
    }

    /// 只有当前且未过期租约的认证入站消息可以续期；过期后迟到消息不能复活。
    pub fn received(&mut self, book: &DeviceBook, lease: &PresenceLease, now: Instant) -> Result<(), LinkError> {
        let entry = self.current(lease)?;
        if !entry.trust.authorized_by(book) { entry.unavailable = Some(UnavailableReason::Disconnected); return Err(LinkError::Untrusted); }
        let elapsed = now.checked_duration_since(entry.last_contact).ok_or(LinkError::Invalid)?;
        if entry.unavailable.is_some() { return Err(LinkError::Expired); }
        if elapsed >= DEVICE_HEARTBEAT_TIMEOUT { entry.unavailable = Some(UnavailableReason::HeartbeatTimeout); return Err(LinkError::Expired); }
        entry.last_contact = now; Ok(())
    }

    /// 旧连接的关闭不能影响新代次；本地关闭不会伪造一次收到消息的时间。
    pub fn disconnected(&mut self, lease: &PresenceLease, reason: UnavailableReason) -> Result<(), LinkError> {
        let entry = self.current(lease)?;
        if entry.unavailable.is_none() { entry.unavailable = Some(reason); } Ok(())
    }

    /// 返回精确过期租约供网络owner关闭对应socket，不按设备ID误关新的连接。
    pub fn expire(&mut self, book: &DeviceBook, now: Instant) -> Vec<PresenceLease> {
        let mut expired = Vec::new();
        for (device_id, entry) in &mut self.entries {
            let revoked = !entry.trust.authorized_by(book);
            if entry.unavailable.is_none() && (revoked || now.checked_duration_since(entry.last_contact).is_some_and(|age| age >= DEVICE_HEARTBEAT_TIMEOUT)) {
                entry.unavailable = Some(if revoked { UnavailableReason::Disconnected } else { UnavailableReason::HeartbeatTimeout });
                expired.push(PresenceLease { device_id: device_id.clone(), generation: entry.generation });
            }
        } expired
    }

    /// 只有本机移除信任真实落盘成功后调用；全局代次继续递增。
    pub fn forget(&mut self, device_id: &str) { self.entries.remove(device_id); }

    /// 仅投影当前已保存设备；即使定时过期尚未调度也不显示超时连接可达。
    pub fn snapshot(&self, book: &DeviceBook, now: Instant) -> Vec<DevicePresenceView> {
        book.summaries().into_iter().map(|saved| {
            let Some(entry) = self.entries.get(&saved.id).filter(|entry| entry.trust.authorized_by(book)) else {
                return DevicePresenceView { device_id: saved.id, info: None, state: DeviceConnectionState::NotConnected, peer: None, since_last_contact: None };
            };
            let age = now.checked_duration_since(entry.last_contact);
            let reason = entry.unavailable.or_else(|| match age {
                None => Some(UnavailableReason::TimeUnknown),
                Some(age) if age >= DEVICE_HEARTBEAT_TIMEOUT => Some(UnavailableReason::HeartbeatTimeout),
                _ => None,
            });
            DevicePresenceView { device_id: saved.id, info: Some(entry.info.clone()), peer: Some(entry.peer), since_last_contact: age,
                state: reason.map_or(DeviceConnectionState::Reachable, DeviceConnectionState::Unavailable) }
        }).collect()
    }

    /// 精确匹配代次；未知或已忘记的租约不泄露其它设备的状态。
    fn current(&mut self, lease: &PresenceLease) -> Result<&mut Entry, LinkError> {
        let entry = self.entries.get_mut(&lease.device_id).ok_or(LinkError::Stale)?;
        if entry.generation != lease.generation { return Err(LinkError::Stale); } Ok(entry)
    }
}

#[cfg(test)]
#[path = "presence_tests.rs"]
mod tests;
