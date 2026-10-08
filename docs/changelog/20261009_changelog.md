# 2026-10-09 开发变更

## GPUI-CORE-001 · 历史领域核心迁入

分类：feature。required_version：`0.2.0`。关联目标：`GPUI-PARITY-001`。

从同一负责人的历史 LokiMetis 迁入纯 Rust 领域实现与原有回归：用量/时间聚合、来源身份与 SQLite schema/迁移、调用筛选和游标、联合视图、Hooks/监控/桌宠及皮肤校验。crate 使用当前 `loki_metris_core` 名称；来源目录仅用于开发取证，不成为运行时依赖。业务核心不依赖 Tauri、GPUI 或 WebView，未复制用户会话、数据库或凭据。

当前 core 383 项回归通过。版本经受管门禁从 `0.1.0` 提升为 `0.2.0`，占用本发布周期首个功能提升；整体 GPUI 体验复刻仍在进行，不能据此视为产品或发布候选验收。

## GPUI-USAGE-001 · 真实本机概览接入

分类：feature。required_version：`0.2.0`。关联目标：`GPUI-PARITY-001`。

原生 GPUI 接入独立 SQLite 索引和历史本机扫描宿主，支持 Codex、Claude Code、Grok、WorkBuddy 与联合概览、六个日历窗口、缺失指标和覆盖质量、精确值复制、进程内筛选保持及合作取消/退出收尾。WorkBuddy 单独显示会话、请求、积分与 Trace；官方 Geist 静态字重仅内嵌注册，双语/主题即时切换。原始 Agent 文件只读，参考 App 和其派生数据保持独立。

core 383 项、GUI/扫描宿主 240 项相关回归通过。Computer Use 已执行两端 Codex 昨日逐字段对照、WorkBuddy 概览、复制反馈、主题语言、窗口状态与正常退出检查；开发预览证据只保存在忽略的 release/gpui-parity/。后续统计/图表/调用、完整来源管理、监控、换肤和原生体验仍在实施，未形成最终候选。此功能沿用周期内 `0.2.0`，未再次提升 Minor。

## GPUI-DETAILS-001 · 统计与图表实施检查点

分类：feature，实施中，计划 required_version：`0.2.0`，尚未申请完成事件。原生统计和 Canvas 图表消费 core 页面模型，支持按 Agent 保留窗口/分组/曲线选择、日桶和前十分组、WorkBuddy 独立趋势、精确复制及悬浮提示。只读查询不等待或取消 writer，周期请求原子跳过 busy，单次业务 panic 不永久关闭队列。

GUI 250 项相关回归通过；Computer Use 的 Codex 昨日统计与 11:00 悬浮读数对齐，WorkBuddy 八项摘要和小时曲线、双语侧栏可实际操作。像素、动画和部分可达导航仍需收敛，完整来源/调用/设置未完成；未形成发布候选。
