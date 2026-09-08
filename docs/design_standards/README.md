# LokiMetis UI 设计标准目录

本目录是 GUI 展示、布局、组件语义和交互所有权的规范入口。实现前先读取 `docs/GUI_APP_PROFILE.md`，再只加载当前任务命中的标准。

## 路由顺序

1. 读取 GUI profile、当前请求和已批准 ADR（若存在），确认身份、compact 侧栏和产品专属约束。
2. 所有 Tauri/React 页面读取 [`tauri_gui.md`](tauri_gui.md)。
3. 涉及 AppShell、导航、Logo、图标、密度或内容偏移时，再读取 [`tauri_sidebar.md`](tauri_sidebar.md)。
4. 当前标准没有覆盖或用户要求偏离时，先取得明确批准，再把稳定标准 ID、精确差异和关联 ADR 写回 GUI profile。

## 标准索引

| 条件 | 稳定标识 | 文档 |
|---|---|---|
| Tauri 2 + React + Mantine GUI | `tauri-gui-common-v1` | [`tauri_gui.md`](tauri_gui.md) |
| 当前 compact 左侧栏 | `tauri-gui-sidebar-compact-80-v1` | [`tauri_sidebar.md`](tauri_sidebar.md) |

## 变更治理

- 像素、密度、组件层级、交互所有权、焦点和可访问名称是硬规则。
- 页面不得在局部复制主题 token、侧栏尺寸或品牌 hex；这些事实分别由主题、AppShell 与 GUI profile 拥有。
- 当前赞助支持按已批准差异嵌入设置页底部，不建立独立路由或侧栏入口；全局快捷键禁用，相关状态与交互不得进入设计或运行时。
- 产品专属视觉或信息架构变化必须先定义产品，并由负责人批准；长期差异或硬规则例外同时记录 ADR。
- 验证真实窗口、窄宽度、键盘、明暗主题和中英文文案；开发预览或组件快照不能替代最终安装产物 E2E。
