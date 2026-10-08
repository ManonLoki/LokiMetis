//! 统一的来源根文案：面向 CLI/MCP/GUI 可复用的无副作用文本。

/// 返回本产品可复用数据源索引写入失败时的统一文案。
pub const fn source_root_store_error_message() -> &'static str {
    "无法更新本产品的数据根索引；Agent 客户端原始文件未修改。"
}

/// 返回通过目录选择器选定目录不可读时的统一文案。
pub const fn source_root_selected_directory_unreadable_message() -> &'static str {
    "所选项不是可读的本地目录。"
}

/// 返回源根标识格式校验失败时的统一文案。
pub const fn source_root_id_invalid_message() -> &'static str {
    "数据根标识无效。"
}

/// 返回源根别名格式校验失败时的统一文案。
pub const fn source_root_alias_invalid_message() -> &'static str {
    "数据根别名必须是 1–64 个字符的短文本，不能包含路径分隔符。"
}

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

/// 返回源根登记成功时的统一文案。
pub const fn source_root_registered_message() -> &'static str {
    "数据根已登记；可通过快速扫描建立本产品索引。"
}

/// 返回源根重复登记时的统一文案。
pub const fn source_root_already_registered_message() -> &'static str {
    "该目录已在数据源中；已保留现有别名、启停和主目录设置。"
}

/// 返回源根已启用时的统一文案。
pub const fn source_root_enabled_message() -> &'static str {
    "数据根已启用；下次快速扫描将纳入该范围。"
}

/// 返回源根已停用时的统一文案。
pub fn source_root_disabled_message(client_display_name: &str) -> String {
    format!("数据根已停用；已有索引保留，原始 {client_display_name} 文件未修改。")
}

/// 返回源根别名更新时的统一文案。
pub const fn source_root_alias_updated_message() -> &'static str {
    "数据根别名已更新。"
}

/// 返回源根移除时的统一文案。
pub fn source_root_removed_message(client_display_name: &str) -> String {
    format!("已移除该根的本产品索引；原始 {client_display_name} 文件未修改。")
}

/// 返回非 Codex 客户端设置主数据目录时的统一文案。
pub const fn source_root_primary_only_for_codex_message() -> &'static str {
    "只有 Codex 数据根可设为主数据目录。"
}

/// 返回尝试设置停用源根为主目录时的统一文案。
pub const fn source_root_primary_root_not_enabled_message() -> &'static str {
    "停用的数据根不能设为主数据目录。"
}

/// 返回主目录校验未通过时的统一文案。
pub const fn source_root_primary_validation_failed_message() -> &'static str {
    "该目录当前未通过本地卷、链接与 Codex rollout 签名验证，主目录未改变。"
}

/// 返回成功切换 Codex 主数据目录时的统一文案。
pub const fn source_root_primary_changed_message() -> &'static str {
    "已切换 Codex 主数据目录。"
}

/// 返回主数据目录已经选择后重复请求时的统一文案。
pub const fn source_root_primary_already_selected_message() -> &'static str {
    "该目录已经是 Codex 主数据目录。"
}

/// 返回清除 Codex 主数据目录后的统一文案。
pub const fn source_root_primary_cleared_message() -> &'static str {
    "已清除主数据目录选择。"
}

/// 返回当前未设置 Codex 主数据目录时的统一文案。
pub const fn source_root_primary_not_set_message() -> &'static str {
    "当前未设置 Codex 主数据目录。"
}
