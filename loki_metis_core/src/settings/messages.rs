//! 运行时设置读写与校验路径共用文案。

/// 语言、初始化等持久化失败后的统一提示。
pub const fn privacy_settings_save_failed_message() -> &'static str {
    "无法保存隐私设置；原设置保持不变。"
}

/// 扫描间隔保存失败后的统一提示。
pub const fn scan_interval_save_failed_message() -> &'static str {
    "无法保存扫描间隔；原设置保持不变。"
}

/// 扫描间隔合法范围提示。
pub const fn scan_interval_range_message() -> &'static str {
    "扫描间隔必须是 1 至 1440 分钟。"
}

/// 保留天数保存失败后的统一提示。
pub const fn retention_days_save_failed_message() -> &'static str {
    "无法保存自动清理天数；原设置保持不变。"
}

/// 保留天数合法范围提示。
pub const fn retention_days_range_message() -> &'static str {
    "自动清理天数必须是 1 至 3650 天。"
}

/// 语言设置保存失败后的统一提示。
pub const fn language_setting_save_failed_message() -> &'static str {
    "无法保存语言设置；原设置保持不变。"
}

/// 概览本机窗口偏好保存失败后的统一提示。
pub const fn overview_window_save_failed_message() -> &'static str {
    "无法保存概览窗口偏好；原设置保持不变。"
}

/// 用量页窗口或分组维度保存失败后的统一提示。
pub const fn usage_query_save_failed_message() -> &'static str {
    "无法保存用量查询条件；原设置保持不变。"
}

/// 页头最近一次选中 Agent 保存失败后的统一提示。
pub const fn last_selected_agent_save_failed_message() -> &'static str {
    "无法保存最近选中的 Agent；原设置保持不变。"
}

/// 初始化状态保存失败后的统一提示。
pub const fn initialization_state_save_failed_message() -> &'static str {
    "无法保存初始化状态；原设置保持不变。"
}

/// 首次扫描标记持久化失败后的统一提示。
pub const fn initial_scan_state_save_failed_message() -> &'static str {
    "无法记录首次扫描状态；未启动自动扫描。"
}

/// WorkBuddy 本地统计开关保存失败后的统一提示。
pub const fn workbuddy_stats_enabled_save_failed_message() -> &'static str {
    "无法保存 WorkBuddy 本地统计开关；原设置保持不变。"
}

/// Codex 客户端索引文件逻辑位置标签。
pub const fn index_location_codex_label() -> &'static str {
    "本产品应用数据目录 / usage-index.sqlite3（Codex）"
}

/// Claude Code 客户端索引文件逻辑位置标签。
pub const fn index_location_claude_code_label() -> &'static str {
    "本产品应用数据目录 / clients / claude-code / usage-index.sqlite3"
}

/// Grok Build CLI 客户端索引文件逻辑位置标签。
pub const fn index_location_grok_build_cli_label() -> &'static str {
    "本产品应用数据目录 / clients / grok-build-cli / usage-index.sqlite3"
}

/// 装配完成后业务响应的状态说明。
pub const fn scaffold_status_ready_message() -> &'static str {
    "用量看板业务命令已装配。"
}

/// 装配未完成时业务响应的中性说明。
pub const fn scaffold_status_not_ready_message() -> &'static str {
    "产品能力正在实施中，尚未启用真实数据读取。"
}

/// 返回业务冒烟输出固定描述。
pub const fn usage_smoke_overview_message() -> &'static str {
    "隔离只读冒烟未访问真实账户或用户目录。"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 验证设置校验与持久化错误暴露稳定用户文案。
    fn exposes_settings_messages() {
        assert_eq!(
            privacy_settings_save_failed_message(),
            "无法保存隐私设置；原设置保持不变。"
        );
        assert_eq!(
            scan_interval_save_failed_message(),
            "无法保存扫描间隔；原设置保持不变。"
        );
        assert_eq!(
            language_setting_save_failed_message(),
            "无法保存语言设置；原设置保持不变。"
        );
        assert_eq!(
            overview_window_save_failed_message(),
            "无法保存概览窗口偏好；原设置保持不变。"
        );
        assert_eq!(
            usage_query_save_failed_message(),
            "无法保存用量查询条件；原设置保持不变。"
        );
        assert_eq!(
            last_selected_agent_save_failed_message(),
            "无法保存最近选中的 Agent；原设置保持不变。"
        );
        assert_eq!(
            initialization_state_save_failed_message(),
            "无法保存初始化状态；原设置保持不变。"
        );
        assert_eq!(
            initial_scan_state_save_failed_message(),
            "无法记录首次扫描状态；未启动自动扫描。"
        );
        assert_eq!(
            workbuddy_stats_enabled_save_failed_message(),
            "无法保存 WorkBuddy 本地统计开关；原设置保持不变。"
        );
        assert_eq!(
            scan_interval_range_message(),
            "扫描间隔必须是 1 至 1440 分钟。"
        );
        assert_eq!(
            retention_days_save_failed_message(),
            "无法保存自动清理天数；原设置保持不变。"
        );
        assert_eq!(
            retention_days_range_message(),
            "自动清理天数必须是 1 至 3650 天。"
        );
        assert_eq!(
            index_location_codex_label(),
            "本产品应用数据目录 / usage-index.sqlite3（Codex）"
        );
        assert_eq!(
            index_location_claude_code_label(),
            "本产品应用数据目录 / clients / claude-code / usage-index.sqlite3"
        );
        assert_eq!(
            index_location_grok_build_cli_label(),
            "本产品应用数据目录 / clients / grok-build-cli / usage-index.sqlite3"
        );
        assert_eq!(scaffold_status_ready_message(), "用量看板业务命令已装配。");
        assert_eq!(
            scaffold_status_not_ready_message(),
            "产品能力正在实施中，尚未启用真实数据读取。"
        );
        assert_eq!(
            usage_smoke_overview_message(),
            "隔离只读冒烟未访问真实账户或用户目录。"
        );
    }
}
