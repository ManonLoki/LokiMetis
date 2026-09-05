//! 唯一扫描任务的固定范围、生命周期与可轮询进度；以及清空本产品索引的结果。

use loki_metis_core::{scan_idle_status_message, scan_scope_registered_roots_label};
use serde::{Deserialize, Serialize};

use super::UiMessageCodeDto;

/// 标识扫描进度当前所处的固定范围阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanScopeCodeDto {
    /// 已登记和默认数据根。
    RegisteredRoots,
    /// 用户主动授权发现的本地固定卷。
    LocalFixedVolumes,
    /// 正在发现本地固定卷。
    DiscoveringVolumes,
    /// 固定卷发现已经完成。
    DiscoveryFinished,
    /// 正在索引已确认数据根。
    IndexingRoots,
}

/// 标识扫描范围和是否会主动遍历设备。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanKindDto {
    /// 只检查默认、环境与已登记数据根。
    Quick,
    /// 用户主动发起的本地固定卷发现。
    FullDevice,
}

/// 标识一次近 30 日统一索引的受限触发来源；不接受任意扫描参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LocalIndexRefreshTriggerDto {
    /// 首次向导发现与候选登记已完成。
    Initialization,
    /// 已初始化数据源页的一轮发现已达到允许索引的终态。
    DiscoveryBatch,
    /// 原生目录选择器直接验证并登记了单个数据根。
    DirectManual,
}

/// 标识扫描的稳定生命周期。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanStateDto {
    /// 当前没有扫描任务。
    Idle,
    /// 唯一扫描任务正在运行。
    Running,
    /// 扫描正常完成。
    Completed,
    /// 用户取消扫描并保留部分覆盖。
    Cancelled,
    /// 扫描失败但原始客户端文件未被修改。
    Failed,
}

/// 描述扫描阶段的结构化计数，避免前端解析任一语言的进度句子。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanScopeProgressDto {
    /// 当前正在索引的数据根稳定 ID；发现阶段为空。
    pub current_root_id: Option<String>,
    /// 全设备发现已检查的目录数。
    pub directories_scanned: u64,
    /// 全设备发现已确认的数据根数。
    pub roots_discovered: u64,
    /// 索引阶段已经完成的数据根数。
    pub roots_completed: u64,
    /// 索引阶段需要处理的数据根总数。
    pub roots_total: u64,
}

/// 描述可轮询、可取消且不暴露绝对路径的扫描进度。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStatusDto {
    /// 当前扫描 ID；空闲时为空。
    pub scan_id: Option<String>,
    /// 当前扫描范围。
    pub kind: ScanKindDto,
    /// 当前扫描生命周期。
    pub state: ScanStateDto,
    /// 进度基点数，10000 表示 100%。
    pub progress_basis_points: u16,
    /// 不含绝对路径的当前范围标签。
    pub current_scope_label: String,
    /// 当前扫描阶段的稳定本地化代码。
    pub current_scope_code: ScanScopeCodeDto,
    /// 当前阶段的结构化进度参数。
    pub scope_progress: ScanScopeProgressDto,
    /// 当前状态是否允许取消。
    pub can_cancel: bool,
    /// 已访问的候选文件数量。
    pub files_visited: u64,
    /// 已写入索引的 canonical 调用数量。
    pub calls_indexed: u64,
    /// 扫描开始时的 Unix 毫秒时间戳。
    pub started_at_epoch_ms: Option<i64>,
    /// 扫描结束时的 Unix 毫秒时间戳。
    pub finished_at_epoch_ms: Option<i64>,
    /// 脱敏的中文状态说明。
    pub message: String,
    /// 当前扫描状态的稳定本地化代码。
    pub message_code: UiMessageCodeDto,
}

impl Default for ScanStatusDto {
    /// 创建不会暗示扫描已经发生的初始空闲状态。
    fn default() -> Self {
        Self {
            scan_id: None,
            kind: ScanKindDto::Quick,
            state: ScanStateDto::Idle,
            progress_basis_points: 0,
            current_scope_label: scan_scope_registered_roots_label().to_owned(),
            current_scope_code: ScanScopeCodeDto::RegisteredRoots,
            scope_progress: ScanScopeProgressDto::default(),
            can_cancel: false,
            files_visited: 0,
            calls_indexed: 0,
            started_at_epoch_ms: None,
            finished_at_epoch_ms: None,
            message: scan_idle_status_message().to_owned(),
            message_code: UiMessageCodeDto::ScanIdle,
        }
    }
}

/// 描述清空本产品索引后的可见结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearIndexResultDto {
    /// 是否成功清空本产品索引。
    pub cleared: bool,
    /// 明确不会删除当前客户端原始记录的中文说明。
    pub message: String,
    /// 清空结果的稳定本地化代码。
    pub message_code: UiMessageCodeDto,
}

#[cfg(test)]
mod tests {
    use super::{LocalIndexRefreshTriggerDto, ScanScopeCodeDto, ScanScopeProgressDto};

    /// 验证扫描阶段代码与结构化进度参数使用稳定 camelCase wire 契约。
    #[test]
    fn scan_scope_code_and_progress_use_stable_wire_shapes() {
        assert_eq!(
            serde_json::to_value(ScanScopeCodeDto::DiscoveringVolumes).unwrap(),
            serde_json::json!("discoveringVolumes")
        );
        assert_eq!(
            serde_json::to_value(ScanScopeProgressDto {
                current_root_id: Some("root-safe".to_owned()),
                directories_scanned: 12,
                roots_discovered: 3,
                roots_completed: 2,
                roots_total: 4,
            })
            .unwrap(),
            serde_json::json!({
                "currentRootId": "root-safe",
                "directoriesScanned": 12,
                "rootsDiscovered": 3,
                "rootsCompleted": 2,
                "rootsTotal": 4,
            })
        );
        let status = super::ScanStatusDto {
            progress_basis_points: 6_250,
            ..Default::default()
        };
        let status_json = serde_json::to_value(status).unwrap();
        assert_eq!(status_json["progressBasisPoints"], 6_250);
        assert_eq!(
            serde_json::from_str::<LocalIndexRefreshTriggerDto>("\"discoveryBatch\"").unwrap(),
            LocalIndexRefreshTriggerDto::DiscoveryBatch
        );
        assert!(
            serde_json::from_str::<LocalIndexRefreshTriggerDto>("\"periodicAutomatic\"").is_err()
        );
    }
}
