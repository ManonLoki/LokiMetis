# Tauri 工程候选

仅根 Cargo 已选 GUI 且框架为 tauri（旧 GUI 缺省兼容）时完整读取；共同保护、身份渲染和机器匹配仍执行 [所有权策略](ownership-policy.md)。

`$mantine-list-view`、`$desktop-add-gui-system-locale`、`$desktop-add-gui-updater`、`$desktop-add-gui-window-state`、`$desktop-add-gui-dialog`、`$desktop-add-gui-system-tray`、`$desktop-add-gui-single-instance`、`$desktop-add-gui-deep-link`、`$desktop-add-gui-global-shortcut`、`$desktop-add-gui-system-notifications`、`$desktop-add-gui-autostart` 与 `$desktop-prepare-gui-support-surfaces` 只随已选 GUI 且 `gui-framework = "tauri"` 的下游传播；其中 `$mantine-list-view` 的 SKILL、metadata、references 与完整 `assets/` 资产集是一个条件知识资产，传播本身不生成页面或安装 dnd-kit。系统语言、updater、窗口状态与 dialog 属于固定基线，dialog 固定保持主窗口 `dialog:default` 且不授权 fs，托盘、单实例、深链接、全局快捷键、通知和开机自启是否实际接线继续由 profile 中各自的 `enabled|disabled` 决定。支持界面的 Skill、参考、React 模板、品牌 profile/i18n/manifest 和全部原始媒体属于同一完整工程资产，产品实例 `docs/GUI_SUPPORT_SURFACES.md` 不属于。`$desktop-add-gui-adapter` 与 Tauri 打包 Skills 同样仅适用于 Tauri。

Tauri GUI 目标平台包含 Windows 或 Linux 时还同步 `$desktop-build-tauri-local-install`。`$desktop-prepare-gui-app-identity` 同样适用。传播知识不自动启用能力、生成业务页面或安装依赖；实际能力、产品身份及支持实例继续 protected。
