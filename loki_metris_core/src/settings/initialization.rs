//! 初始化向导完成门禁：完成位必须带上当前合同 Key 才算完成。

/// 当前新手引导合同的稳定 Key。步数、完成条件或入索引语义变化时必须换新值。
pub const CURRENT_INITIALIZATION_WIZARD_KEY: &str = "20260819-username-agents-autoscan";

/// 根据磁盘完成位与所存 Key 判定是否已完成当前向导。
///
/// 旧文件只有 `initializationCompleted: true`、Key 缺失或不匹配时视为未完成。
/// 未完成不得清掉其它已校验设置。
pub fn initialization_completed_from_stored(
    stored_completed: bool,
    stored_key: Option<&str>,
) -> bool {
    stored_completed && stored_key == Some(CURRENT_INITIALIZATION_WIZARD_KEY)
}

/// 完成时写入本轮 Key；未完成不落 Key。
pub fn initialization_wizard_key_for_store(completed: bool) -> Option<&'static str> {
    completed.then_some(CURRENT_INITIALIZATION_WIZARD_KEY)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 旧完成位或缺 Key、过期 Key 都必须拉回向导；只有本轮 Key 才保持完成。
    #[test]
    fn only_current_wizard_key_counts_as_completed() {
        assert!(!initialization_completed_from_stored(true, None));
        assert!(!initialization_completed_from_stored(
            true,
            Some("expired-wizard-key")
        ));
        assert!(!initialization_completed_from_stored(
            true,
            Some("20260818-three-step")
        ));
        assert!(!initialization_completed_from_stored(
            false,
            Some(CURRENT_INITIALIZATION_WIZARD_KEY)
        ));
        assert!(initialization_completed_from_stored(
            true,
            Some(CURRENT_INITIALIZATION_WIZARD_KEY)
        ));
    }

    /// 保存完成时必须带本轮 Key，清除完成位时不得再写 Key。
    #[test]
    fn store_writes_current_key_only_when_completed() {
        assert_eq!(
            initialization_wizard_key_for_store(true),
            Some(CURRENT_INITIALIZATION_WIZARD_KEY)
        );
        assert_eq!(initialization_wizard_key_for_store(false), None);
    }
}
