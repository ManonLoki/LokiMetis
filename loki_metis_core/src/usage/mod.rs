//! 定义 Token 字段校验、缓存读取口径和单次事实选择规则。

// Serialize/Deserialize 来自 serde，是 Rust 生态最常用的序列化框架；
// derive 后结构体可以和 JSON（前端 DTO）、SQLite（本机索引）等格式互转。
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::Confidence;

/// 定义用户可见与上报 `total_tokens` 的 provider 级聚合口径。
///
/// ADR-121 后三个本机 provider 都必须保留上游单次总量；
/// 保留该类型是为了让调用页游标与聚合依然显式绑定口径。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TotalTokenAccounting {
    /// 直接累计上游已校验的单次总量，缓存输入仍属于实际处理量。
    #[default]
    Observed,
}

/// 表示一次模型调用的规范化 Token 用量。
// #[serde(rename_all = "camelCase")]：Rust 字段名是 snake_case（如 input_tokens），
// 但序列化成 JSON 传给前端 TypeScript 时会自动转成 camelCase（inputTokens），
// 这样前后端各自符合自己语言的命名习惯，不需要手工映射字段名。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    /// 本次调用的全部输入 Token，包含缓存输入。
    pub input_tokens: u64,
    /// 输入 Token 中由缓存读取提供的子集。
    pub cached_input_tokens: Option<u64>,
    /// 上游提供时记录缓存写入 Token。
    pub cache_write_input_tokens: Option<u64>,
    /// 本次调用的输出 Token；推理输出是其分析维度，不重复相加。
    pub output_tokens: u64,
    /// 上游提供的推理输出 Token。
    pub reasoning_output_tokens: Option<u64>,
    /// 上游单次总量，缺失时才按输入加输出推算。
    pub total_tokens: u64,
    /// 标识总量是否由 core 推算，GUI 必须展示相应置信度。
    pub total_is_derived: bool,
}

/// 表示 Token 字段矛盾或聚合溢出的稳定业务错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TokenUsageError {
    /// 缓存输入大于全部输入，违反子集不变量。
    #[error("cached input tokens exceed input tokens")]
    CachedInputExceedsInput,
    /// 推理输出大于全部输出，违反分析子集不变量。
    #[error("reasoning output tokens exceed output tokens")]
    ReasoningOutputExceedsOutput,
    /// 上游总量小于输入加输出，无法作为可信单次总量。
    #[error("total tokens are below input plus output tokens")]
    TotalBelowInputAndOutput,
    /// 计算推算总量或聚合总量时发生整数溢出。
    #[error("token aggregation overflowed")]
    Overflow,
    /// 单次事实和累计回退都缺失，不能生成调用用量。
    #[error("single-call token usage is missing")]
    MissingSingleCallUsage,
}

impl TokenUsage {
    /// 校验单次 Token 字段，并仅在总量缺失时按输入加输出推算。
    pub fn new(
        input_tokens: u64,
        cached_input_tokens: u64,
        cache_write_input_tokens: Option<u64>,
        output_tokens: u64,
        reasoning_output_tokens: u64,
        total_tokens: Option<u64>,
    ) -> Result<Self, TokenUsageError> {
        Self::new_with_availability(
            input_tokens,
            Some(cached_input_tokens),
            cache_write_input_tokens,
            output_tokens,
            Some(reasoning_output_tokens),
            total_tokens,
        )
    }

    /// 校验允许上游缺失缓存或推理分项的单次 Token 字段。
    pub fn new_with_availability(
        input_tokens: u64,
        cached_input_tokens: Option<u64>,
        cache_write_input_tokens: Option<u64>,
        output_tokens: u64,
        reasoning_output_tokens: Option<u64>,
        total_tokens: Option<u64>,
    ) -> Result<Self, TokenUsageError> {
        // is_some_and：Option 上的组合子，等价于「有值且该值满足条件」；
        // 没有值（上游未提供该分项）时直接短路为 false，不会误判为异常。
        if cached_input_tokens.is_some_and(|cached| cached > input_tokens) {
            return Err(TokenUsageError::CachedInputExceedsInput);
        }
        if reasoning_output_tokens.is_some_and(|reasoning| reasoning > output_tokens) {
            return Err(TokenUsageError::ReasoningOutputExceedsOutput);
        }

        // checked_add：整数加法的“防溢出”版本，溢出时返回 None 而不是像
        // 普通 `+` 那样在 release 模式下静默环绕（wrap）成错误数值；
        // `?` 会在 None 时提前用 Overflow 错误从函数返回。
        let minimum_total = input_tokens
            .checked_add(output_tokens)
            .ok_or(TokenUsageError::Overflow)?;
        // match 三路分支：
        //   1) 上游给了总量但比 输入+输出 还小 -> 数据不自洽，报错；
        //   2) 上游给了总量且合法 -> 直接采用，标记“非推算”；
        //   3) 上游没给总量 -> 用 输入+输出 推算，标记“推算得出”。
        let (total_tokens, total_is_derived) = match total_tokens {
            Some(total_tokens) if total_tokens < minimum_total => {
                return Err(TokenUsageError::TotalBelowInputAndOutput);
            }
            Some(total_tokens) => (total_tokens, false),
            None => (minimum_total, true),
        };

        Ok(Self {
            input_tokens,
            cached_input_tokens,
            cache_write_input_tokens,
            output_tokens,
            reasoning_output_tokens,
            total_tokens,
            total_is_derived,
        })
    }

    /// 返回全部分项都可由 provider 证明为零的空集合可加单位元。
    pub const fn zero() -> Self {
        Self::zero_with_component_availability(true, true, true)
    }

    /// 按本值实际观测到的分项可用性构造空集合单位元，用于让空桶/空组
    /// 沿用调用方已聚合总量的分项形状，而不是重新假设某个静态来源。
    pub const fn zero_matching_availability(&self) -> Self {
        Self::zero_with_component_availability(
            self.cached_input_tokens.is_some(),
            self.cache_write_input_tokens.is_some(),
            self.reasoning_output_tokens.is_some(),
        )
    }

    /// 按 provider 能力构造空集合单位元，未提供的分项必须继续保持未知。
    pub const fn zero_with_component_availability(
        cached_input_available: bool,
        cache_write_available: bool,
        reasoning_output_available: bool,
    ) -> Self {
        Self {
            input_tokens: 0,
            cached_input_tokens: if cached_input_available {
                Some(0)
            } else {
                None
            },
            cache_write_input_tokens: if cache_write_available { Some(0) } else { None },
            output_tokens: 0,
            reasoning_output_tokens: if reasoning_output_available {
                Some(0)
            } else {
                None
            },
            total_tokens: 0,
            total_is_derived: false,
        }
    }

    /// 返回未由缓存读取覆盖的输入 Token；上游未提供缓存分项时保持未知。
    // Option::map：有值时对内部值做变换并保持 Some 包装，None 则原样传递；
    // 这里因为 new() 已校验过 cached <= input，减法不会下溢。
    pub fn uncached_input_tokens(&self) -> Option<u64> {
        self.cached_input_tokens
            .map(|cached| self.input_tokens - cached)
    }

    /// 按 provider 已批准口径返回本条调用对总 Token 的贡献。
    pub fn accounted_total_tokens(&self, accounting: TotalTokenAccounting) -> u64 {
        match accounting {
            TotalTokenAccounting::Observed => self.total_tokens,
        }
    }

    /// 返回缓存读取占比的基点数；输入为零时返回“不适用”。
    // “基点”(basis point) = 万分之一，10_000 基点等于 100%；
    // 用整数而不是浮点数表示百分比，避免浮点精度问题，前端展示时再除以 100 转成百分数。
    pub fn cache_read_basis_points(&self) -> Option<u16> {
        if self.input_tokens == 0 {
            return None;
        }

        // `?` 用在 Option 上：cached_input_tokens 为 None 时提前返回 None。
        let cached_input_tokens = self.cached_input_tokens?;
        // 先转成 u128 做乘法，避免 u64 * 10_000 在极端大数值下溢出，
        // 算完比例后再收窄回 u16（比例上限就是 10_000，用 expect 断言不可能超出）。
        let basis_points =
            (u128::from(cached_input_tokens) * 10_000) / u128::from(self.input_tokens);
        Some(u16::try_from(basis_points).expect("validated ratio cannot exceed 10000"))
    }

    /// 表示该调用是否实际包含缓存读取 Token。
    pub fn had_cache_read(&self) -> Option<bool> {
        self.cached_input_tokens.map(|cached| cached > 0)
    }

    /// 对两个已校验用量执行逐字段安全求和，供 canonical 聚合使用。
    // 逐字段调用下面的 checked_sum / optional_checked_sum 辅助函数，
    // 任意一个字段溢出都会通过 `?` 让整个方法提前返回 Overflow 错误。
    pub fn checked_add(&self, other: &Self) -> Result<Self, TokenUsageError> {
        Ok(Self {
            input_tokens: checked_sum(self.input_tokens, other.input_tokens)?,
            cached_input_tokens: optional_checked_sum(
                self.cached_input_tokens,
                other.cached_input_tokens,
            )?,
            cache_write_input_tokens: optional_checked_sum(
                self.cache_write_input_tokens,
                other.cache_write_input_tokens,
            )?,
            output_tokens: checked_sum(self.output_tokens, other.output_tokens)?,
            reasoning_output_tokens: optional_checked_sum(
                self.reasoning_output_tokens,
                other.reasoning_output_tokens,
            )?,
            total_tokens: checked_sum(self.total_tokens, other.total_tokens)?,
            total_is_derived: self.total_is_derived || other.total_is_derived,
        })
    }
}

/// 表示解析器为单次调用选中的用量及其回退置信度。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedTokenUsage {
    /// 被选中的规范化用量。
    pub usage: TokenUsage,
    /// 直接使用 `last` 时为精确，回退累计值时为推算。
    pub confidence: Confidence,
    /// 标识是否因缺失 `last` 而使用累计值，聚合层必须向用户提示。
    pub used_cumulative_fallback: bool,
}

/// 表示一个 Token 快照是否包含可聚合的新调用事实。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncrementalTokenUsageDecision {
    /// 当前事件带来一次新的调用用量。
    Emit(SelectedTokenUsage),
    /// 当前事件只是累计快照重放或上下文估算，不得进入调用聚合。
    IgnoreNonIncremental,
}

/// 根据前后累计快照识别真实新增调用，并过滤 Codex 的重放与上下文估算。
// Codex 的持久 `token_count` 不是逐次调用专用事件：额度更新会原样重发
// 当前累计快照，compact 后也会用“只有 total、没有输入输出”的快照表达
// 上下文占用估算。ADR-085 已忽略完全相同的累计重放和 total-only 估算，
// 但仍会把两种非增量快照写成新调用，导致本机合计相对会话最新
// `total_token_usage` 虚高约 2–10 倍：
//   1. `last_token_usage` 只是当前 running `total` 的拷贝，每回合发出
//      整个会话累计而不是本回合增量；多回合求和是三角数；
//   2. `last` 未变而 `total` 因额度/compact 快照变化，相同 `last` 被再计一次。
// `last` 等于当前累计且存在上一已计费累计时，只在必填字段未下降时采用累计正增量；
// `last` 与上一已计费 `last` 相同则忽略。调用方不得把被忽略的估算或 same-last
// 快照写进 previous_cumulative / previous_last，否则增量会减到非账单基线。
// 缺少 `last` 的旧记录仍沿用显式累计回退。
pub fn select_incremental_call_usage(
    last: Option<TokenUsage>,
    cumulative: Option<TokenUsage>,
    previous_cumulative: Option<&TokenUsage>,
    previous_last: Option<&TokenUsage>,
) -> Result<IncrementalTokenUsageDecision, TokenUsageError> {
    if cumulative
        .as_ref()
        .zip(previous_cumulative)
        .is_some_and(|(current, previous)| current == previous)
    {
        return Ok(IncrementalTokenUsageDecision::IgnoreNonIncremental);
    }

    if last
        .as_ref()
        .zip(cumulative.as_ref())
        .is_some_and(|(last, cumulative)| {
            is_total_only_context_estimate(last) && is_total_only_context_estimate(cumulative)
        })
    {
        return Ok(IncrementalTokenUsageDecision::IgnoreNonIncremental);
    }

    if last
        .as_ref()
        .zip(previous_last)
        .is_some_and(|(current_last, previous_last)| current_last == previous_last)
    {
        return Ok(IncrementalTokenUsageDecision::IgnoreNonIncremental);
    }

    if let (Some(last), Some(cumulative), Some(previous)) =
        (last.as_ref(), cumulative.as_ref(), previous_cumulative)
        && last == cumulative
    {
        return Ok(
            match billed_increment_when_last_copies_session_total(cumulative, previous) {
                Some(usage) => IncrementalTokenUsageDecision::Emit(SelectedTokenUsage {
                    usage,
                    confidence: Confidence::Exact,
                    used_cumulative_fallback: false,
                }),
                None => IncrementalTokenUsageDecision::IgnoreNonIncremental,
            },
        );
    }

    select_single_call_usage(last, cumulative).map(IncrementalTokenUsageDecision::Emit)
}

/// 判断快照是否只填了上下文总量、输入输出都为零。
fn is_total_only_context_estimate(usage: &TokenUsage) -> bool {
    usage.input_tokens == 0 && usage.output_tokens == 0 && usage.total_tokens > 0
}

/// 当 `last` 只是当前会话累计的拷贝时，用前后累计正差值作为本次账单增量。
// 仅在 `last == current total` 这一已证明的误标模式下做减法：必填字段下降
// 视为 compact/回滚，差值为零则不是新账单。可选分项无法构成合法子集时保持未知，
// 不得为了凑字段去夹紧或伪造。这是对 ADR-085「不得用下降累计伪造调用」的窄例外。
fn billed_increment_when_last_copies_session_total(
    current: &TokenUsage,
    previous: &TokenUsage,
) -> Option<TokenUsage> {
    if current.input_tokens < previous.input_tokens
        || current.output_tokens < previous.output_tokens
        || current.total_tokens < previous.total_tokens
    {
        return None;
    }
    let input_tokens = current.input_tokens - previous.input_tokens;
    let output_tokens = current.output_tokens - previous.output_tokens;
    let total_tokens = current.total_tokens - previous.total_tokens;
    if input_tokens == 0 && output_tokens == 0 && total_tokens == 0 {
        return None;
    }
    let cached_input_tokens = optional_token_increment(
        current.cached_input_tokens,
        previous.cached_input_tokens,
        input_tokens,
    );
    let cache_write_input_tokens = optional_token_increment(
        current.cache_write_input_tokens,
        previous.cache_write_input_tokens,
        u64::MAX,
    );
    let reasoning_output_tokens = optional_token_increment(
        current.reasoning_output_tokens,
        previous.reasoning_output_tokens,
        output_tokens,
    );
    TokenUsage::new_with_availability(
        input_tokens,
        cached_input_tokens,
        cache_write_input_tokens,
        output_tokens,
        reasoning_output_tokens,
        Some(total_tokens),
    )
    .or_else(|_| {
        TokenUsage::new_with_availability(
            input_tokens,
            cached_input_tokens,
            cache_write_input_tokens,
            output_tokens,
            reasoning_output_tokens,
            None,
        )
    })
    .ok()
}

/// 计算可选 Token 分项的非负增量；无法保持子集不变量时记为未知。
fn optional_token_increment(
    current: Option<u64>,
    previous: Option<u64>,
    maximum: u64,
) -> Option<u64> {
    match (current, previous) {
        (Some(current), Some(previous)) if current >= previous => {
            let increment = current - previous;
            (increment <= maximum).then_some(increment)
        }
        _ => None,
    }
}

/// 优先选择单次 `last` 事实，仅在缺失时显式回退累计值。
// 这是本文件的核心业务规则：上游日志有时只提供“累计用量”而没有单次用量，
// 此函数保证调用方总是优先拿到精确的单次数据，只有在缺失时才降级使用
// 推算值，并且必须显式标记 confidence/used_cumulative_fallback 供 UI 提示用户。
pub fn select_single_call_usage(
    last: Option<TokenUsage>,
    cumulative: Option<TokenUsage>,
) -> Result<SelectedTokenUsage, TokenUsageError> {
    if let Some(usage) = last {
        return Ok(SelectedTokenUsage {
            usage,
            confidence: Confidence::Exact,
            used_cumulative_fallback: false,
        });
    }

    // 走到这里说明 last 是 None：尝试用累计值兜底；
    // cumulative 也是 None 的话，ok_or 把 Option 转成 Result 的 Err 分支。
    cumulative
        .map(|usage| SelectedTokenUsage {
            usage,
            confidence: Confidence::Derived,
            used_cumulative_fallback: true,
        })
        .ok_or(TokenUsageError::MissingSingleCallUsage)
}

/// 对单个 Token 字段执行不会静默环绕的加法。
fn checked_sum(left: u64, right: u64) -> Result<u64, TokenUsageError> {
    left.checked_add(right).ok_or(TokenUsageError::Overflow)
}

/// 只有两侧都提供字段时才求和，避免把部分可见误报为完整总量。
// zip：两个 Option 都是 Some 时才组合成 Some((left, right))，任一为 None 则整体 None；
// 这里的设计意图是——如果只有一侧知道某个可选分项，直接相加会给出一个
// “看似精确实则遗漏了另一侧”的假总量，所以宁可保持 None（未知）也不瞎加。
fn optional_checked_sum(
    left: Option<u64>,
    right: Option<u64>,
) -> Result<Option<u64>, TokenUsageError> {
    left.zip(right)
        .map(|(left, right)| checked_sum(left, right))
        .transpose()
}

#[cfg(test)]
mod tests;
