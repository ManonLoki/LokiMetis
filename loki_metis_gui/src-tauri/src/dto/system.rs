//! 全局首次初始化门禁，不携带任一客户端、路径或索引内容。

use serde::Serialize;

use super::LanguagePreferenceDto;

/// 描述全局首次初始化门禁；该状态不携带客户端、路径或索引内容。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializationStatusDto {
    /// 为 true 时才允许进入业务页面并安排自动本机扫描。
    pub initialization_completed: bool,
    /// 当前持久语言偏好，供初始化门禁显示正确语言。
    pub language_preference: LanguagePreferenceDto,
}
