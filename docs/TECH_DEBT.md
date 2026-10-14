# 已知边界与技术债

- 产品边界以最新 `docs/product_spec/` 为准；完整候选仍需按当次构建与渠道要求独立验收。
- Windows 剩余宿主验收：托盘指针交互、通知横幅、自启登录、真实 Agent／WSL Hooks、Codex／WorkBuddy 换肤、混合 DPI 热插拔及 NSIS 安装仍需对应真实场景，单元测试和开发预览不能替代。
- GPUI 固定基线当前不提供 updater 服务、通用原生 dialog 与深链接；未来确有需求时独立批准并接入真实实现。当前更新检查零出站。
- 上游传递依赖 block 0.1.6 被 Cargo 报告未来 Rust 不兼容；当前 Rust 1.99.0 构建/测试通过，未来升级工具链时核对上游修复。空热键接口产生未使用项编译警告，初始化无动作/注册是已确认行为。
- 尚无 upstream-lock；首次工程层升级先初始基线审计。版本门禁与完整升级工具已保留，不伪造来源锁。
- 本次 Computer Use 无法读取系统菜单栏；托盘图标、恢复、退出由用户真实本机检查确认，其余原生窗口和系统状态由工具观察。未来发布候选仍按当次 E2E/渠道要求独立验收。
- Core-first 待提升：自动/周期扫描的准入规则（自动扫描只覆盖已开放 Agent、周期触发遇 writer 忙即跳过、显式触发排队等待）仍写在 GUI `usage_service::scan` 与 `UsageService::request_scan`。core 中早期为 Tauri 节拍设计的同类策略从未接线，已随死代码清理删除；下次改动扫描调度时应把这组规则提升为 core 用例并由 GUI 调用。
- core 的 `LocalIndex` 仍公开若干只被测试使用的观测接口（如 `usage_snapshot`、`usage_snapshot_since`、`aggregate_for_provider`、`save_normalized_snapshot`、`clear_index`、`list_sources`、`primary_root`、`provider_snapshot_count`）以及里程碑状态的 `pending_count`、`source_reset` 等，供 GUI 跨 crate 回归断言。若引入测试专用 feature，可把它们移出生产 API；在此之前保留，删除会削弱现有回归。
- Windows 测试构建用测试宿主替换原生桌宠/窗口宿主，`cargo check --tests --target x86_64-pc-windows-gnu` 因此对原生 Windows 宿主模块报告约 40 条 dead_code 警告（生产 Windows 构建已无警告）。若要消除，需要让原生宿主模块在测试配置下不参与编译，并保持真实 HWND 回归可用。
