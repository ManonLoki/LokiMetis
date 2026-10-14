//! 统一的来源根文案：面向 CLI/MCP/GUI 可复用的无副作用文本。

/// 返回当前客户端源根未发现时的统一文案。
pub const fn source_root_not_found_message() -> &'static str {
    "未找到指定的数据根。"
}

/// 返回尝试重新索引停用数据根时的统一文案。
pub const fn source_root_reindex_requires_enabled_message() -> &'static str {
    "停用的数据根不能重新索引；请先启用该数据根。"
}

/// 返回单根重新索引前的本地卷、链接或结构签名校验失败文案。
pub const fn source_root_reindex_validation_failed_message() -> &'static str {
    "该数据根当前未通过本地卷、链接与来源结构签名验证；旧索引已保留。"
}
