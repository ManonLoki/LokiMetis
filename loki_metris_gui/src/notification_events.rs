//! Hook 与换肤的短时通知邮箱；后台只投递纯值，系统发送仍归应用唯一原生 owner。

use loki_metris_core::{AiTool, HookNotification, SkinHostKind};
use std::{collections::VecDeque, sync::Mutex, time::{Duration, Instant}};

/// 瞬时通知不携带会话内容、用户输入、路径或宿主错误文本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Activity {
    Hook { tool: AiTool, state: HookNotification },
    Skin { host: SkinHostKind, success: bool },
}

/// 一次后台终态对应一个独立原生通知；超时后不补发过时操作。
pub(crate) struct ActivityRequest {
    pub id: String,
    pub activity: Activity,
    pub created: Instant,
}

/// 一个锁同步发送资格和有界队列，关闭与权限暂停立即丢弃未提交瞬时事件。
#[derive(Default)]
struct Mailbox {
    ready: bool,
    requests: VecDeque<ActivityRequest>,
}

/// 应用级 owner 在页面离开后继续接收终态，退出后不保留跨进程旧事件。
#[derive(Default)]
pub(crate) struct ActivityQueue(Mutex<Mailbox>);
impl ActivityQueue {
    /// 只消费已确认开关及系统权限；不在这里请求授权或重放暂停期间事件。
    pub fn configure(&self, ready: bool) {
        let mut mailbox = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        mailbox.ready = ready;
        if !ready { mailbox.requests.clear(); }
    }
    /// 有界内存投递，不做 IO；满载时淘汰最旧事件并记录稳定诊断。
    pub fn push(&self, activity: Activity) {
        let mut mailbox = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if !mailbox.ready { return; }
        if mailbox.requests.len() == 128 {
            mailbox.requests.pop_front();
            tracing::warn!(event = "activity_notification_capacity");
        }
        mailbox.requests.push_back(ActivityRequest {
            id: format!("a1-{}", uuid::Uuid::new_v4().simple()), activity, created: Instant::now(),
        });
    }
    /// 只取当前仍有效的事件；等待过久的终态不会在权限恢复后突然出现。
    pub fn take(&self) -> Option<ActivityRequest> {
        let mut mailbox = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        while let Some(request) = mailbox.requests.pop_front() {
            if mailbox.ready && request.created.elapsed() <= Duration::from_secs(60) {
                return Some(request);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 默认关闭、暂停清队列和重新开启均不能重放旧事件；有效事件保持独立身份。
    #[test]
    fn activity_notifications_obey_permission_and_lifecycle() {
        let queue = ActivityQueue::default();
        let event = Activity::Skin { host: SkinHostKind::Codex, success: false };
        queue.push(event);
        assert!(queue.take().is_none());
        queue.configure(true);
        queue.push(event);
        queue.push(event);
        let first = queue.take().unwrap();
        let second = queue.take().unwrap();
        assert_eq!(first.activity, event);
        assert_ne!(first.id, second.id);
        queue.push(event);
        queue.configure(false);
        queue.configure(true);
        assert!(queue.take().is_none());
    }
    /// 邮箱上限和过期策略防止长期后台事件堆积；不使用真实等待。
    #[test]
    fn activity_notifications_bound_backlog_and_expire() {
        let queue = ActivityQueue::default();
        queue.configure(true);
        for _ in 0..150 { queue.push(Activity::Skin { host: SkinHostKind::Codex, success: true }); }
        assert_eq!(queue.0.lock().unwrap().requests.len(), 128);
        for request in &mut queue.0.lock().unwrap().requests { request.created -= Duration::from_secs(61); }
        assert!(queue.take().is_none());
    }
}
