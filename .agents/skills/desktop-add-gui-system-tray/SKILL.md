---
name: desktop-add-gui-system-tray
description: 为已选择 GUI 系统托盘能力的下游安装 tray-icon、双语菜单与关闭隐藏生命周期；未选择时不得接入。
---

# 增加 GUI 系统托盘

只在 `docs/GUI_APP_PROFILE.md` 的 `system_tray = enabled`，或初始化后产品范围明确批准系统托盘时使用。

## 固定契约

1. 该能力不是独立插件，而是 Tauri `tray-icon` feature + GUI adapter 生命周期约束；启用时才在 `tauri` 上打开 feature。
2. 托盘安装必须从 Builder `.setup(...)` 可达，并在同一实现里绑定 `show_window`/`quit` 双项菜单、必需应用图标、`.icon(...)` 与 `.build(app)`；关闭隐藏从 `.on_window_event(...)` 可达。
3. 可见标签只通过 `rust-i18n` 解析为中文“显示窗口/退出”与英文 “Show Window/Quit”，语言切换无需重启刷新。
4. 左键与显示项只恢复既有主窗口；`CloseRequested` 只隐藏，`quit` 才结束进程。禁用时不得残留 feature、菜单、locale 资源、`prevent_close()` 或隐藏调用。
5. locale 必须来自 GUI adapter 固定的 system-locale helper，不得自行探测；Builder 插件顺序由 GUI 基线统一拥有，本 Skill 只通过 `.setup(...)` 和 `.on_window_event(...)` 接线，不插入伪 plugin。
6. 必须保留 `tray_show_restores_and_focuses_main_window`、`close_request_hides_without_exit`、`tray_quit_exits_application`、`tray_labels_resolve_for_supported_locales`、`tray_labels_fall_back_to_english`、`language_change_updates_tray_menu_labels` 六个非空回归和真实宿主托盘场景；透明 32px 图标、空白点击区域、常量真断言或未接线死代码都不能通过。

## 完成输出

报告 feature、菜单 ID、本地图标、关闭隐藏/恢复/退出生命周期，以及禁用时关闭最后窗口退出的缺席证据。
