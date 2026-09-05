//! 本机扫描与本地索引错误文案的共享常量。
use std::fmt::Write;

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

/// 本地索引读取失败的统一通用文案。
pub const fn local_storage_read_error_message() -> &'static str {
    "无法读取本产品本机索引；Agent 客户端原始记录未被修改。"
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

/// 本机扫描在当前触发源下无法启动时返回的稳定文案。
pub const fn local_scan_in_progress_error_message() -> &'static str {
    "已有扫描正在运行。"
}

/// 将客户端展示名称与扫描失败细节拼接为统一前缀。
pub fn local_scan_client_failure_message(client_display_name: &str, detail: &str) -> String {
    let mut message = String::with_capacity(client_display_name.len() + detail.len() + 8);
    let _ = write!(message, "{} 扫描失败：{}", client_display_name, detail);
    message
}

/// 本机扫描与索引写入冲突时用于扫描失败状态的后缀文案。
pub const fn local_scan_writer_busy_failure_detail() -> &'static str {
    "本机索引已有写入任务；原始客户端文件未被修改。"
}

/// 本机扫描入口已被占用时，直接返回给用户的短报错。
pub const fn local_scan_writer_busy_message() -> &'static str {
    "本机索引已有写入任务。"
}

/// 同步/异步 writer 资源抢占失败时给用户的统一文案。
pub const fn local_scan_not_running_cancellable_message() -> &'static str {
    "当前客户端没有可取消的扫描。"
}

/// 返回扫描运行期间显示的通用完成语。
pub const fn scan_running_status_message() -> &'static str {
    "正在扫描用户授权范围。"
}

/// 返回扫描已请求取消时的通用语。
pub const fn scan_cancelling_status_message() -> &'static str {
    "正在取消扫描；已发现候选将保留。"
}

/// 返回扫描已取消后的通用完成语。
pub const fn scan_cancelled_status_message() -> &'static str {
    "扫描已取消；覆盖标记为不完整。"
}

/// 返回扫描成功完成后的通用语（带句号）。
pub const fn scan_completed_status_message() -> &'static str {
    "扫描完成。"
}

/// 返回“扫描已完成”阶段性文字（不含句号）。
pub const fn scan_progress_finished_message() -> &'static str {
    "扫描完成"
}

/// 返回“已登记数据根”扫描范围标签。
pub const fn scan_scope_registered_roots_label() -> &'static str {
    "已登记数据根"
}

/// 返回“尚未开始扫描”状态说明。
pub const fn scan_idle_status_message() -> &'static str {
    "尚未开始扫描。"
}

/// 返回“本地固定卷”扫描范围标签。
pub const fn scan_scope_local_fixed_volumes_label() -> &'static str {
    "本地固定卷"
}

/// 返回“已清空 index”操作的通用成功文案。
pub fn clear_local_index_success_message(client_name: &str) -> String {
    format!("已清空 {client_name} 的本产品索引；原始客户端文件未受影响。")
}

/// 返回本机索引读取失败时的实现降级说明。
pub const fn local_usage_overview_unavailable_message() -> &'static str {
    "本机索引暂时无法读取，请重试。"
}

/// 清空索引与扫描任务冲突时的提示。
pub const fn clear_local_index_while_scanning_message() -> &'static str {
    "扫描运行期间不能清空索引；请先取消扫描。"
}

/// 根数据登记与扫描写入竞争时禁止修改的统一文案。
pub const fn source_root_operations_blocked_by_scan_message() -> &'static str {
    "扫描运行期间不能修改数据根。"
}

/// 扫描工作线程不可用时用于扫描失败状态的后缀文案。
pub const fn local_scan_worker_unavailable_detail() -> &'static str {
    "本机扫描工作线程不可用；原始客户端文件未被修改。"
}

#[cfg(test)]
mod tests {
    use super::{
        LocalScanErrorCategory, LocalScanErrorKindProvider, local_scan_client_failure_message,
        local_scan_error_message, local_scan_error_message_from, local_scan_task_error_message,
        local_scan_worker_unavailable_detail, local_scan_writer_busy_failure_detail,
        local_storage_read_error_message, local_usage_overview_unavailable_message,
        scan_completed_status_message, scan_idle_status_message, scan_progress_finished_message,
        scan_running_status_message, scan_scope_local_fixed_volumes_label,
        scan_scope_registered_roots_label,
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
    /// 验证公共扫描错误映射为稳定且可展示的文案。
    fn exposes_common_scan_error_messages() {
        assert_eq!(
            local_scan_task_error_message(),
            "本机索引任务失败；Agent 原始记录未被修改。"
        );
        assert_eq!(
            local_storage_read_error_message(),
            "无法读取本产品本机索引；Agent 客户端原始记录未被修改。"
        );
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
    /// 验证扫描错误状态返回对应的用户可见说明。
    fn exposes_scan_error_state_messages() {
        let detail = local_scan_writer_busy_failure_detail();
        let failure = local_scan_client_failure_message("Codex", detail);
        assert_eq!(
            failure,
            "Codex 扫描失败：本机索引已有写入任务；原始客户端文件未被修改。"
        );
        assert_eq!(
            local_scan_worker_unavailable_detail(),
            "本机扫描工作线程不可用；原始客户端文件未被修改。"
        );
    }

    #[test]
    /// 验证扫描生命周期状态返回完整稳定文案。
    fn exposes_scan_status_messages() {
        assert_eq!(scan_running_status_message(), "正在扫描用户授权范围。");
        assert_eq!(scan_completed_status_message(), "扫描完成。");
        assert_eq!(scan_progress_finished_message(), "扫描完成");
        assert_eq!(scan_scope_registered_roots_label(), "已登记数据根");
        assert_eq!(scan_idle_status_message(), "尚未开始扫描。");
        assert_eq!(scan_scope_local_fixed_volumes_label(), "本地固定卷");
        assert_eq!(
            local_usage_overview_unavailable_message(),
            "本机索引暂时无法读取，请重试。"
        );
    }
}
