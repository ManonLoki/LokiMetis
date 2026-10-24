//! 面向界面的只读视图装配：本机窗口汇总、统计、图表、调用列表与多 Agent 联合视图。
//!
//! 视图只消费 canonical 调用与聚合结果，不写入索引；依赖方向为
//! `combined → calls/chart`、`chart → statistics`，`local` 独立。

pub(crate) mod calls;
pub(crate) mod chart;
pub(crate) mod combined;
pub(crate) mod local;
pub(crate) mod statistics;
