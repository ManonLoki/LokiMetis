//! 面向 GUI 的可序列化 Hook 错误：`code` 对应前端 i18n 键。

use std::collections::BTreeMap;

use serde::Serialize;

/// Hook 配置生成、合并或解析失败时返回给 adapter 的稳定错误。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HookError {
    /// 前端翻译键。
    pub code: &'static str,
    /// 翻译插值参数。
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
}

impl HookError {
    /// 构造不含插值的错误。
    pub fn new(code: &'static str) -> Self {
        Self {
            code,
            params: BTreeMap::new(),
        }
    }

    /// 追加一个插值参数。
    #[must_use]
    pub fn param(mut self, key: &str, value: impl Into<String>) -> Self {
        self.params.insert(key.to_owned(), value.into());
        self
    }
}

impl std::fmt::Display for HookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code)?;
        if !self.params.is_empty() {
            write!(f, " {:?}", self.params)?;
        }
        Ok(())
    }
}

impl std::error::Error for HookError {}
