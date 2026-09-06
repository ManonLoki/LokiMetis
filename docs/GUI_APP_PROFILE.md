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

系统托盘保留 `show_window` 与 `quit` 基线项，并增加 `toggle_pet_overlay` 桌宠显隐动作。桌宠在每次冷启动时默认显示，托盘动作只改变当前会话的真实窗口可见性，不保存显隐偏好。

## 桌宠浮窗基线

- 桌宠浮窗与主窗口并存；显示桌宠不隐藏主窗口，也不中止本机 Hook listener。
- 浮窗固定使用 12 个纯位置编号。位置不预绑定 Codex、Claude Code、Grok 或 WorkBuddy；Agent 只在实时 Hook 状态按其展示草稿中的位置写入后成为该位置的动态内容，冷启动没有初始图片或 Agent 占位名称。
- 同一位置允许由不同 Agent 配置使用，运行时按最近一次有效展示或释放迁移决定该位置内容，不自动改号或重排其它位置。
- 浮窗对齐 AIMonitorDesktop 2.0.5 的交互与视觉：透明无边框画布、图片等比完整显示、悬停信息层、循环分页、左键原生拖动、右键打开独立桌宠设置窗、方向键与滚轮翻页、`Ctrl`/`Cmd` + 滚轮缩放。
- 支持 `1×1`、`1×2`、`2×1`、`1×3`、`3×1`、`2×2` 六种布局；默认 `2×2`，默认单格为 64 逻辑像素。设置窗提供布局、尺寸、始终置顶和位置/大小锁定，并提供返回主界面及隐藏到托盘动作。
- 锁定只禁止拖动和缩放，不禁止翻页、悬停或打开设置。桌宠设置是独立原生窗口，关闭时隐藏并保留桌宠当前状态。

## 应用身份

- 中文名：诡秘神谕
- 英文名与应用显示名：LokiMetis
- 项目标识：`loki_metis`
- 负责人：ManonLoki
- 接口：GUI only
- Rust 结构：`loki_metis_core` + `loki_metis_gui`
- 目标平台：Windows、macOS、Linux
- 初始版本：`0.1.0`；当前版本始终以根 `Cargo.toml` 为准
- 窗口标题：`LokiMetis`
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
