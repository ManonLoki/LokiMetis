# LokiMetis GUI 应用资料

本文件是 GUI 初始化选择、应用身份和适用 UI 标准的唯一持久事实来源。当前项目是中性 GUI 基线，产品目标、业务范围和成功标准尚未定义。

## 初始化配置

```gui-initialization-config
system_tray = enabled
system_notification = enabled
autostart = enabled
sponsor_page = disabled
single_instance = enabled
deep_link = enabled
global_shortcut = disabled
sidebar_mode = compact
```

七项条件能力与侧栏模式由 ManonLoki 于 2026-09-05 确认。`deep_link = enabled` 与 `single_instance = enabled` 的组合有效；深链接只接受身份派生的精确恢复地址 `app-loki-metis://restore`。系统语言、窗口状态、dialog、设置页中的应用/版本与本地更新日志属于固定基线，不是配置字段。

## 应用身份

- 中文名：诡秘神谕
- 英文名与应用显示名：LokiMetis
- 项目标识：`loki_metis`
- 负责人：ManonLoki
- 接口：GUI only
- Rust 结构：`loki_metis_core` + `loki_metis_gui`
- 目标平台：Windows、macOS、Linux
- 初始版本：`0.1.0`；当前版本始终以根 `Cargo.toml` 为准
- 窗口标题：`LokiMetis v{version}`
- 产品定义状态：`productDefinitionRequired = true`

## Logo 选择证据

初始化时按稳定顺序展示并保留了三个原始候选：`candidate-1`、`candidate-2`、`candidate-3`。用户选择 `candidate-1`；未选候选不记录格式、摘要或标准化结论。

所选项经过后置验证和必要标准化，结果为 1024×1024、8-bit RGBA、非交错、sRGB 无损 PNG，透明边角无亮边，主体保留安全边距，并在 32×32 的明暗背景上可辨认。

- 最终母版：`loki_metis_gui/src-tauri/icons/app-icon-master.png`
- 前端运行时副本：`loki_metis_gui/public/app-identity/logo.png`
- 平台图标目录：`loki_metis_gui/src-tauri/icons/`
- 最终母版 SHA-256：`82108611d590c195bb633261d1454f2d81461e656f81c88231b47685ec427d62`

母版与前端副本必须逐字节一致；平台图标只由项目本地 Tauri `icon` 命令从该母版生成。托盘图标固定使用并由 bundle 引用 `loki_metis_gui/src-tauri/icons/32x32.png`。

## UI 标准

- 通用标准：`tauri-gui-common-v1`
- 侧栏标准：`tauri-gui-sidebar-compact-80-v1`
- 当前无经批准的像素或信息架构偏离。
- 赞助页、赞助媒体、全局快捷键依赖、配置、命令、状态、文案和运行时接线必须缺席。
- `/about` 与 `/test` 路由必须缺席；固定 `/settings` 提供应用、版本、本地更新日志、语言、主题，以及已启用的通知与自启开关。

首次产品开发必须先使用 `$desktop-define-product` 明确产品意图、MVP 边界、约束和成功标准。身份或 UI 标准变化必须经项目负责人确认后更新本文件；形成长期取舍或硬规则例外时同时记录 ADR。
