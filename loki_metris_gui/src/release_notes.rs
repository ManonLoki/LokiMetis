//! 经过候选工具验证的最近十版双语条目；日期、版本、功能优化与修复保持同一编译快照。

#![allow(clippy::type_complexity, reason = "条目元组结构由候选打包工具生成并只读校验，源码不手工改写")]

include!(concat!(env!("OUT_DIR"), "/release_notes.rs"));
