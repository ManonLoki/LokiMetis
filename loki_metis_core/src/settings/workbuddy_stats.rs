//! 用户是否显式开放读取 WorkBuddy 本地用量统计；默认关闭，关闭时不得读取或展示。

/// 新安装与旧设置文件缺该字段时的默认值：关闭。
pub const DEFAULT_WORKBUDDY_STATS_ENABLED: bool = false;

/// 缺字段视为关闭，不得把旧文件缺省回填为已开启。
pub const fn resolve_workbuddy_stats_enabled(stored: Option<bool>) -> bool {
    match stored {
        Some(enabled) => enabled,
        None => DEFAULT_WORKBUDDY_STATS_ENABLED,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 缺省与旧文件缺字段都必须是关闭，不能悄悄回填成开启。
    #[test]
    fn defaults_and_missing_storage_are_disabled() {
        const { assert!(!DEFAULT_WORKBUDDY_STATS_ENABLED) }
        assert!(!resolve_workbuddy_stats_enabled(None));
    }

    /// 已保存的显式值必须原样返回。
    #[test]
    fn stored_value_is_returned_verbatim() {
        assert!(resolve_workbuddy_stats_enabled(Some(true)));
        assert!(!resolve_workbuddy_stats_enabled(Some(false)));
    }
}
