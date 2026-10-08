//! 仅向操作系统提交已批准的通知；Kit 的无返回值 API 不能证明授权或送达。

use gpui_kit::{App, SystemNotification};

/// 应用偏好、输入拒绝和平台提交是不同结果，不提供虚假的送达状态。
#[derive(Debug, PartialEq, Eq)]
pub enum Submission {
    Disabled,
    InvalidPayload,
    Submitted,
}

/// 真实产品触发器由下游确认；中性启动与设置切换从不调用此入口。
pub fn submit(cx: &App, enabled: bool, tag: &str, title: &str, body: &str) -> Submission {
    if !enabled { return Submission::Disabled }
    if tag.is_empty() || tag.len() > 128 || !tag.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_-/.:".contains(&byte))
        || title.trim().is_empty() || title.len() > 256 || body.len() > 4096
        || title.chars().chain(body.chars()).any(|ch| ch == '\0') {
        return Submission::InvalidPayload;
    }
    cx.show_system_notification(SystemNotification {
        tag: tag.to_owned().into(), title: title.to_owned().into(), body: body.to_owned().into(), actions: Vec::new(),
    });
    Submission::Submitted
}

#[cfg(test)]
#[path = "notifications_test.rs"]
mod tests;
