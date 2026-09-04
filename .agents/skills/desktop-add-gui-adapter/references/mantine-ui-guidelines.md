# Tauri Mantine UI 实施入口

Tauri + React + Mantine 的用户可见设计标准已经迁移到：

- [`docs/design_standards/README.md`](../../../../docs/design_standards/README.md)：标准匹配、优先级与偏离治理；
- [Tauri GUI 通用设计标准](../../../../docs/design_standards/tauri_gui.md)：Theme、布局、组件语义、交互、i18n、可访问性与测试；
- [Tauri GUI 左侧栏标准](../../../../docs/design_standards/tauri_sidebar.md)：精简/详细模式的唯一尺寸、AppShell 接线和回归。

本文件只保留 `$desktop-add-gui-adapter` 的稳定兼容入口。实现 GUI 时必须先从设计标准索引完成精确匹配，再结合 [React 前端基线](react-frontend-baseline.md)和 [GUI 基线](gui-baseline.md)处理依赖、状态所有权、Tauri 生命周期与 core-first 边界。产品 `docs/GUI_APP_PROFILE.md` 中已批准的专属标准优先于 Harness 通用缺省；没有精确命中或用户要求偏离时，不得自行发明密度。
