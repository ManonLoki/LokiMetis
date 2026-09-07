//! 定义换皮 IPC 使用的稳定、可序列化错误。

use serde::Serialize;
use thiserror::Error;

/// 向前端返回稳定错误码、脱敏消息与可选详情。
#[derive(Debug, Error, Serialize)]
#[error("{message}")]
pub struct AppError {
    pub code: &'static str,
    pub message: String,
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
