//! 换肤适配层通过服务回执传递的稳定、可序列化错误。

use serde::Serialize;
use thiserror::Error;

/// 向界面返回稳定错误码、脱敏消息与可选详情。
#[derive(Clone, Debug, Error, Serialize)]
#[error("{message}")]
pub struct AppError {
    /// 稳定错误码。
    pub code: &'static str,
    /// 脱敏说明。
    pub message: String,
    /// 可选详情。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<String>,
}

impl AppError {
    /// 创建不带详情的稳定换皮错误。
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: Vec::new(),
        }
    }

    /// 创建带有多项脱敏详情的稳定换皮错误。
    pub fn with_details(
        code: &'static str,
        message: impl Into<String>,
        details: Vec<String>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            details,
        }
    }
}
