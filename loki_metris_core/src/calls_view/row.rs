//! 调用列表内部行与可见筛选装配，专门封装排序/标签前处理。

use std::cmp::Ordering;
use std::collections::BTreeMap;

use super::{
    UsageAvailableFilters, UsageCallFilters, UsageCallSortDirection, UsageCallSortField,
    UsageCallsQuery, UsageFilterOption,
};
use crate::{
    DisplayLabelCode, TotalTokenAccounting, UsageCall, is_safe_usage_filter_id, safe_path_basename,
    safe_root_label, safe_short_value, safe_technical_label, safe_thread_title,
    validate_usage_filter_id,
};

/// 仅表示行内字段，不直接用于持久化；跨 `calls_view`/`cursor` 子模块共享，
/// 因此字段是 `pub(crate)` 而非 crate 外公开。
pub struct UsageCallRow {
    pub(crate) call: UsageCall,
    pub(crate) model_id: String,
    pub(crate) model_label: String,
    pub(crate) model_label_code: DisplayLabelCode,
    pub(crate) reasoning_effort_id: String,
    pub(crate) reasoning_effort_label: String,
    pub(crate) reasoning_effort_label_code: DisplayLabelCode,
    pub(crate) project_id: String,
    pub(crate) project_label: String,
    pub(crate) project_label_code: DisplayLabelCode,
    pub(crate) thread_id: String,
    pub(crate) thread_label: String,
    pub(crate) thread_label_code: DisplayLabelCode,
    pub(crate) root_ids: Vec<String>,
}

/// 当行组装逻辑发生异常时的占位错误类型，当前调用路径不会返回该错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildUsageCallRowsError {
    /// 未预期的输入不变更。
    UnsupportedInput,
}

/// 把一个 canonical 调用转换为只含业务展示字段的内部行。
pub fn build_rows(calls: &[UsageCall]) -> Vec<UsageCallRow> {
    calls.iter().cloned().map(UsageCallRow::from_call).collect()
}

/// 校验筛选 ID，并在非法时返回明确边界错误。
pub fn validate_filters(filters: &UsageCallFilters) -> Result<(), String> {
    for value in [
        filters.model.as_deref(),
        filters.reasoning_effort.as_deref(),
        filters.project.as_deref(),
        filters.thread.as_deref(),
        filters.root.as_deref(),
    ] {
        if let Some(value) = value
            && validate_usage_filter_id(value).is_err()
        {
            return Err("调用筛选包含不允许的值；请从页面固定选项中选择。".to_owned());
        }
    }
    Ok(())
}

/// 从完整集合中抽取安全、可展示的筛选选项。
pub fn collect_available_filters(
    rows: &[UsageCallRow],
    root_aliases: &std::collections::BTreeMap<String, String>,
) -> UsageAvailableFilters {
    let mut models = BTreeMap::new();
    let mut reasoning_efforts = BTreeMap::new();
    let mut projects = BTreeMap::new();
    let mut threads = BTreeMap::new();
    let mut roots = BTreeMap::new();

    for row in rows {
        models
            .entry(row.model_id.clone())
            .or_insert_with(|| (row.model_label.clone(), row.model_label_code));
        reasoning_efforts
            .entry(row.reasoning_effort_id.clone())
            .or_insert_with(|| {
                (
                    row.reasoning_effort_label.clone(),
                    row.reasoning_effort_label_code,
                )
            });
        projects
            .entry(row.project_id.clone())
            .or_insert_with(|| (row.project_label.clone(), row.project_label_code));
        threads
            .entry(row.thread_id.clone())
            .or_insert_with(|| (row.thread_label.clone(), row.thread_label_code));
        for root_id in &row.root_ids {
            roots.entry(root_id.clone()).or_insert_with(|| {
                root_aliases
                    .get(root_id)
                    .and_then(|alias| safe_root_label(alias))
                    .map(|label| (label, DisplayLabelCode::Literal))
                    .unwrap_or_else(|| ("未命名数据根".to_owned(), DisplayLabelCode::UnnamedRoot))
            });
        }
    }

    UsageAvailableFilters {
        models: finalize_filter_options(models),
        reasoning_efforts: finalize_filter_options(reasoning_efforts),
        projects: finalize_filter_options(projects),
        threads: finalize_filter_options(threads),
        roots: finalize_filter_options(roots),
    }
}

/// 按展示标签排序并给同名标签打稳定歧义序号。
fn finalize_filter_options(
    options: BTreeMap<String, (String, DisplayLabelCode)>,
) -> Vec<UsageFilterOption> {
    let mut options = options
        .into_iter()
        .map(|(id, (label, label_code))| UsageFilterOption {
            id,
            label,
            label_code,
            disambiguation_index: None,
        })
        .collect::<Vec<_>>();

    let mut indices_by_label = BTreeMap::<String, Vec<usize>>::new();
    for (index, option) in options.iter().enumerate() {
        indices_by_label
            .entry(option.label.clone())
            .or_default()
            .push(index);
    }
    for indices in indices_by_label
        .values_mut()
        .filter(|indices| indices.len() > 1)
    {
        indices.sort_by(|left, right| options[*left].id.cmp(&options[*right].id));
        for (offset, index) in indices.iter().enumerate() {
            options[*index].disambiguation_index = Some(offset + 1);
        }
    }
    options.sort_by(|left, right| {
        left.label
            .cmp(&right.label)
            .then_with(|| left.id.cmp(&right.id))
    });

    options
}

/// 对固定筛选键执行 AND 等值匹配。
pub fn matches_filters(row: &UsageCallRow, filters: &UsageCallFilters) -> bool {
    filters
        .model
        .as_ref()
        .is_none_or(|value| value == &row.model_id)
        && filters
            .reasoning_effort
            .as_ref()
            .is_none_or(|value| value == &row.reasoning_effort_id)
        && filters
            .project
            .as_ref()
            .is_none_or(|value| value == &row.project_id)
        && filters
            .thread
            .as_ref()
            .is_none_or(|value| value == &row.thread_id)
        && filters
            .root
            .as_ref()
            .is_none_or(|value| row.root_ids.contains(value))
}

/// 使用固定列与方向比较，实现稳定总排序。
pub fn compare_rows(
    left: &UsageCallRow,
    right: &UsageCallRow,
    query: &UsageCallsQuery,
    accounting: TotalTokenAccounting,
) -> Ordering {
    let primary = match query.sort_field {
        UsageCallSortField::OccurredAt => left
            .call
            .occurred_at_epoch_ms
            .cmp(&right.call.occurred_at_epoch_ms),
        UsageCallSortField::Model => left.model_label.cmp(&right.model_label),
        UsageCallSortField::ReasoningEffort => left
            .reasoning_effort_label
            .cmp(&right.reasoning_effort_label),
        UsageCallSortField::InputTokens => left
            .call
            .usage
            .input_tokens
            .cmp(&right.call.usage.input_tokens),
        UsageCallSortField::CachedInputTokens => left
            .call
            .usage
            .cached_input_tokens
            .cmp(&right.call.usage.cached_input_tokens),
        UsageCallSortField::UncachedInputTokens => left
            .call
            .usage
            .uncached_input_tokens()
            .cmp(&right.call.usage.uncached_input_tokens()),
        UsageCallSortField::OutputTokens => left
            .call
            .usage
            .output_tokens
            .cmp(&right.call.usage.output_tokens),
        UsageCallSortField::ReasoningOutputTokens => left
            .call
            .usage
            .reasoning_output_tokens
            .cmp(&right.call.usage.reasoning_output_tokens),
        UsageCallSortField::TotalTokens => left
            .call
            .usage
            .accounted_total_tokens(accounting)
            .cmp(&right.call.usage.accounted_total_tokens(accounting)),
    };

    let directed = match query.sort_direction {
        UsageCallSortDirection::Asc => primary,
        UsageCallSortDirection::Desc => primary.reverse(),
    };
    directed.then_with(|| left.call.logical_call_id.cmp(&right.call.logical_call_id))
}

// 与 statistics_view/detail.rs::group_label 里的同一张表保持逐字一致；
// 新增推理强度取值时两处必须同步修改。
/// 将推理强度规范值映射为安全展示文案与稳定标签代码。
fn reasoning_effort_display_label(value: &str) -> (String, DisplayLabelCode) {
    match value {
        "__unknown_reasoning_effort__" => (
            "未知推理强度".to_owned(),
            DisplayLabelCode::UnknownReasoningEffort,
        ),
        "none" => ("无".to_owned(), DisplayLabelCode::ReasoningNone),
        "minimal" => ("最低".to_owned(), DisplayLabelCode::ReasoningMinimal),
        "low" => ("低".to_owned(), DisplayLabelCode::ReasoningLow),
        "medium" => ("中".to_owned(), DisplayLabelCode::ReasoningMedium),
        "high" => ("高".to_owned(), DisplayLabelCode::ReasoningHigh),
        "xhigh" => ("很高".to_owned(), DisplayLabelCode::ReasoningXHigh),
        other => (other.to_owned(), DisplayLabelCode::Literal),
    }
}

/// 仅接受不含路径信息的稳定数据根标识，防止展示层泄露绝对路径。
fn safe_root_id_from_input(value: &str) -> Option<String> {
    if is_safe_usage_filter_id(value) {
        Some(value.to_owned())
    } else {
        None
    }
}

impl UsageCallRow {
    /// 从 core `UsageCall` 映射为不泄露路径的内部行。
    fn from_call(call: UsageCall) -> Self {
        let model_key = call.model.as_deref().and_then(safe_technical_label);
        let (model_id, model_label, model_label_code) = match model_key {
            Some(model) => (model.clone(), model, DisplayLabelCode::Literal),
            None => (
                "__unknown_model__".to_owned(),
                "未知模型".to_owned(),
                DisplayLabelCode::UnknownModel,
            ),
        };

        let reasoning_effort_id = call
            .reasoning_effort
            .as_deref()
            .and_then(safe_technical_label)
            .unwrap_or_else(|| "__unknown_reasoning_effort__".to_owned());

        let (reasoning_effort_label, reasoning_effort_label_code) =
            reasoning_effort_display_label(&reasoning_effort_id);

        let project_id = call
            .project_key
            .as_deref()
            .filter(|value| is_safe_usage_filter_id(value))
            .unwrap_or("__unknown_project__")
            .to_owned();
        let (project_label, project_label_code) = if project_id == "__unknown_project__" {
            (
                "未归类项目".to_owned(),
                DisplayLabelCode::UncategorizedProject,
            )
        } else if let Some(label) = call.project_label.as_deref().and_then(safe_path_basename) {
            (label, DisplayLabelCode::Literal)
        } else {
            (safe_short_value(&project_id), DisplayLabelCode::Project)
        };

        let thread_id = if is_safe_usage_filter_id(&call.thread_key) {
            call.thread_key.clone()
        } else {
            "__unknown_thread__".to_owned()
        };
        let (thread_label, thread_label_code) = if thread_id == "__unknown_thread__" {
            ("未知线程".to_owned(), DisplayLabelCode::UnknownThread)
        } else if let Some(label) = call.thread_label.as_deref().and_then(safe_thread_title) {
            (label, DisplayLabelCode::Literal)
        } else {
            (safe_short_value(&thread_id), DisplayLabelCode::Thread)
        };

        let root_ids = call
            .provenance
            .iter()
            .map(|source| source.root_id.clone())
            .filter_map(|root_id| safe_root_id_from_input(&root_id))
            .collect::<Vec<_>>();

        Self {
            call,
            model_id,
            model_label,
            model_label_code,
            reasoning_effort_id,
            reasoning_effort_label,
            reasoning_effort_label_code,
            project_id,
            project_label,
            project_label_code,
            thread_id,
            thread_label,
            thread_label_code,
            root_ids,
        }
    }
}
