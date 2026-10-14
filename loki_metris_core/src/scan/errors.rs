//! 本机扫描与本地索引错误文案的共享常量。

/// 代表可对外稳定展示的扫描错误分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalScanErrorCategory {
    /// 扫描到无效路径。
    InvalidPath,
    /// 文件系统权限不足。
    PermissionDenied,
    /// 来源尚不可访问。
    SourceUnavailable,
    /// 不受支持的来源结构。
    UnsupportedSchema,
    /// 本地索引层失败。
    Database,
    /// 发现了无效用量字段。
    InvalidUsage,
    /// 发现任务冲突。
    ScanBusy,
    /// 计数超出可安全范围。
    Overflow,
}

impl LocalScanErrorCategory {
    /// 返回稳定中文标签。
    pub const fn label(self) -> &'static str {
        match self {
            Self::InvalidPath => "来源路径无效",
            Self::PermissionDenied => "权限被拒绝",
            Self::SourceUnavailable => "来源暂不可用",
            Self::UnsupportedSchema => "来源格式不受支持",
            Self::Database => "本产品索引故障",
            Self::InvalidUsage => "用量字段无效",
            Self::ScanBusy => "扫描任务冲突",
            Self::Overflow => "本机计数超出范围",
        }
    }
}

/// 本地扫描执行失败的统一通用文案。
pub const fn local_scan_task_error_message() -> &'static str {
    "本机索引任务失败；Agent 原始记录未被修改。"
}

/// 按分类返回不暴露绝对路径或 SQL 的扫描错误文案。
pub fn local_scan_error_message(category: LocalScanErrorCategory) -> String {
    format!("错误类别：{}；Agent 原始记录未被修改。", category.label())
}

/// 将适配器本机错误稳定分类的统一入口。
pub trait LocalScanErrorKindProvider {
    /// 返回供跨端共享的本机错误分类。
    fn local_scan_error_kind(&self) -> LocalScanErrorCategory;
}

/// 将实现 `LocalScanErrorKindProvider` 的错误转换为统一用户文案。
pub fn local_scan_error_message_from(error: &impl LocalScanErrorKindProvider) -> String {
    local_scan_error_message(error.local_scan_error_kind())
}

/// 返回扫描已取消后的通用完成语。
pub const fn scan_cancelled_status_message() -> &'static str {
    "扫描已取消；覆盖标记为不完整。"
}

#[cfg(test)]
mod tests {
    use super::{
        LocalScanErrorCategory, LocalScanErrorKindProvider, local_scan_error_message,
        local_scan_error_message_from, local_scan_task_error_message,
    };

    /// 提供可控扫描错误类别的测试替身。
    struct FakeLocalScanError(LocalScanErrorCategory);

    impl LocalScanErrorKindProvider for FakeLocalScanError {
        /// 返回测试预设的本机扫描错误类别。
        fn local_scan_error_kind(&self) -> LocalScanErrorCategory {
            self.0
        }
    }

    #[test]
    /// 验证扫描错误类别具有稳定标签。
    fn exposes_scan_error_category_label() {
        let message = local_scan_error_message(super::LocalScanErrorCategory::InvalidPath);
        assert!(message.contains("来源路径无效"));
        assert_eq!(
            local_scan_error_message_from(&FakeLocalScanError(
                super::LocalScanErrorCategory::PermissionDenied,
            )),
            super::local_scan_error_message(super::LocalScanErrorCategory::PermissionDenied)
        );
    }

    #[test]
    /// 验证索引任务失败文案明确原始记录未被修改。
    fn exposes_common_scan_error_messages() {
        assert_eq!(
            local_scan_task_error_message(),
            "本机索引任务失败；Agent 原始记录未被修改。"
        );
    }
}
