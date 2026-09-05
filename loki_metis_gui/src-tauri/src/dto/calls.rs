//! 调用页的安全筛选、固定排序与逐条本机调用响应。

use loki_metis_core::{LocalIndexState, MetricFact, TokenUsage};
use serde::{Deserialize, Serialize};

use super::{AgentClientKindDto, DisplayLabelCodeDto};

/// 描述前端请求的安全调用筛选，只接受后端生成的不透明 ID 或技术标签。
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsageCallFiltersDto {
    /// 模型技术标签；空值表示全部。
    pub model: Option<String>,
    /// 推理强度技术标签；空值表示全部。
    pub reasoning_effort: Option<String>,
    /// 匿名项目 ID；不得是路径。
    pub project: Option<String>,
    /// 内容无关的线程 ID。
    pub thread: Option<String>,
    /// 内容无关的数据根 ID；不得是路径。
    pub root: Option<String>,
}

/// 固定调用排序字段；IPC 不接受任意列名或表达式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageCallSortField {
    /// 按调用发生时间排序。
    #[default]
    OccurredAt,
    /// 按模型显示标签排序。
    Model,
    /// 按推理强度显示标签排序。
    ReasoningEffort,
    /// 按全部输入 Token 排序。
    InputTokens,
    /// 按缓存输入 Token 排序。
    CachedInputTokens,
    /// 按未缓存输入 Token 排序。
    UncachedInputTokens,
    /// 按输出 Token 排序。
    OutputTokens,
    /// 按推理输出 Token 排序。
    ReasoningOutputTokens,
    /// 按单次总 Token 排序。
    TotalTokens,
}

/// 固定调用排序方向。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageCallSortDirection {
    /// 升序排列主排序字段。
    Asc,
    /// 降序排列主排序字段。
    #[default]
    Desc,
}

/// 描述调用页的完整类型化请求，页大小只能由 backend 决定。
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsageCallsQueryDto {
    /// 五类固定安全筛选。
    #[serde(default)]
    pub filters: UsageCallFiltersDto,
    /// 九个固定可见列排序字段。
    #[serde(default)]
    pub sort_field: UsageCallSortField,
    /// 只允许升序或降序。
    #[serde(default)]
    pub sort_direction: UsageCallSortDirection,
    /// backend 上一页返回的不透明游标；首屏为空。
    pub cursor: Option<String>,
}

/// 描述单条本机调用，不包含正文、项目原路径或数据根绝对路径。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageCallItemDto {
    /// 内容无关的稳定行 ID。
    pub id: String,
    /// 本条调用所属的具体物理 Agent。
    pub client: AgentClientKindDto,
    /// 调用发生时的 Unix 毫秒时间戳。
    pub occurred_at_epoch_ms: i64,
    /// 受控项目末段或回退短标签。
    pub project_label: String,
    /// 项目标签的稳定展示语义。
    pub project_label_code: DisplayLabelCodeDto,
    /// 线程短标题或回退短标签。
    pub thread_label: String,
    /// 线程标签的稳定展示语义。
    pub thread_label_code: DisplayLabelCodeDto,
    /// 模型显示名。
    pub model_label: String,
    /// 模型标签的稳定展示语义。
    pub model_label_code: DisplayLabelCodeDto,
    /// 推理强度显示名。
    pub reasoning_effort_label: String,
    /// 推理强度标签的稳定展示语义。
    pub reasoning_effort_label_code: DisplayLabelCodeDto,
    /// 与 Token fact 相同口径的未缓存输入，避免前后端算法漂移。
    pub uncached_input_tokens: Option<u64>,
    /// 本机调用的可追溯 Token 事实。
    pub fact: MetricFact<TokenUsage>,
}

/// 描述一个不透明筛选 ID、稳定展示语义与安全回退标签。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageFilterOptionDto {
    /// backend 生成或验证的固定等值筛选 ID。
    pub id: String,
    /// 不含绝对路径的用户可见标签。
    pub label: String,
    /// 标签的稳定展示语义。
    pub label_code: DisplayLabelCodeDto,
    /// 同名安全标签的稳定序号；前端按当前 locale 添加标点。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disambiguation_index: Option<usize>,
}

/// 描述调用页从完整启用根 canonical 集合生成的安全筛选值。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageAvailableFiltersDto {
    /// 当前结果中可用的模型名。
    pub models: Vec<UsageFilterOptionDto>,
    /// 当前结果中可用的推理强度。
    pub reasoning_efforts: Vec<UsageFilterOptionDto>,
    /// 当前结果中可用的项目别名。
    pub projects: Vec<UsageFilterOptionDto>,
    /// 当前结果中可用的线程短标签。
    pub threads: Vec<UsageFilterOptionDto>,
    /// 当前结果中可用的数据根别名。
    pub roots: Vec<UsageFilterOptionDto>,
}

/// 描述调用查询的一页、完整筛选总数与下一稳定游标。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageCallsPageDto {
    /// 与本页调用处于同一 SQLite 快照的索引四态。
    pub index_state: LocalIndexState,
    /// 当前页面事实的统一观测时刻。
    pub observed_at_epoch_ms: i64,
    /// 固定页大小内的调用行。
    pub items: Vec<UsageCallItemDto>,
    /// 从完整启用集合生成且不含绝对路径的筛选值。
    pub available_filters: UsageAvailableFiltersDto,
    /// 应用全部当前筛选后的完整结果数量。
    pub total_count: u64,
    /// 仍有下一页时返回的版本化不透明游标。
    pub next_cursor: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{DisplayLabelCodeDto, UsageFilterOptionDto};

    /// 同名序号只有存在时才进入 DTO，标点由当前前端 locale 决定。
    #[test]
    fn filter_disambiguation_index_is_optional_and_locale_neutral() {
        let base = UsageFilterOptionDto {
            id: "root-a".to_owned(),
            label: "Shared root".to_owned(),
            label_code: DisplayLabelCodeDto::Literal,
            disambiguation_index: None,
        };
        assert_eq!(
            serde_json::to_value(&base).unwrap(),
            serde_json::json!({
                "id": "root-a",
                "label": "Shared root",
                "labelCode": "literal",
            })
        );
        assert_eq!(
            serde_json::to_value(UsageFilterOptionDto {
                disambiguation_index: Some(2),
                ..base
            })
            .unwrap(),
            serde_json::json!({
                "id": "root-a",
                "label": "Shared root",
                "labelCode": "literal",
                "disambiguationIndex": 2,
            })
        );
    }
}
