# 2026-10-09 开发变更

## GPUI-CORE-001 · 历史领域核心迁入

分类：feature。required_version：`0.2.0`。关联目标：`GPUI-PARITY-001`。

从同一负责人的历史 LokiMetis 迁入纯 Rust 领域实现与原有回归：用量/时间聚合、来源身份与 SQLite schema/迁移、调用筛选和游标、联合视图、Hooks/监控/桌宠及皮肤校验。crate 使用当前 `loki_metris_core` 名称；来源目录仅用于开发取证，不成为运行时依赖。业务核心不依赖 Tauri、GPUI 或 WebView，未复制用户会话、数据库或凭据。

当前 core 383 项回归通过。版本经受管门禁从 `0.1.0` 提升为 `0.2.0`，占用本发布周期首个功能提升；整体 GPUI 体验复刻仍在进行，不能据此视为产品或发布候选验收。
