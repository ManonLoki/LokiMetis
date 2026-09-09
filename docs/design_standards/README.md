# UI 设计标准目录

本目录是 Harness 及其 GUI 下游选择、实现和复核 UI 设计标准的唯一索引。技术栈、运行时与 core-first 边界分别由 `docs/RUST_GUI_TEMPLATE.md` 和 `docs/ENGINEERING_RULES.md` 管理；本目录只管理用户可见的布局、组件语义、交互、可访问性和视觉密度。

## 路由顺序

初始化或修改 UI 时按以下顺序解析，不得先套用旧模板再局部修补：

1. 读取 `docs/GUI_APP_PROFILE.md`、当前用户请求和已批准 ADR，确定壳层、`sidebar_mode`、已选支持页和产品专属约束；接口与框架固定为 Tauri 2 GUI，无需选择。
2. 从本索引选择所有前置条件都精确命中的标准；只满足相似条件不算命中。
3. 当前请求或产品 profile 中已批准的产品专属标准优先于 Harness 通用缺省；不得用旧模板值覆盖它。
4. 若没有精确标准，或用户明确要求偏离，先完成额外设计并取得明确批准，再实现。像素、密度或信息架构偏离必须同步更新下游 `docs/GUI_APP_PROFILE.md` 与当日 ADR；初始化仍处于中性阶段且没有可记录的批准产品事实时应停止偏离，不得自行发明数值。
5. 只把最终命中的规则下发到对应 adapter 展示层。UI 布局、主题和纯交互状态不得下沉到 shared core。

## 标准索引

| 匹配条件 | 标准 | 当前标识 |
|---|---|---|
| Tauri 2 + React + Mantine GUI | [Tauri GUI 通用设计标准](tauri_gui.md) | `tauri-gui-common-v1` |
| 上述 GUI + 固定左侧栏 + `sidebar_mode = compact` | [Tauri GUI 左侧栏标准](tauri_sidebar.md#精简模式) | `tauri-gui-sidebar-compact-80-v1` |
| 上述 GUI + 固定左侧栏 + `sidebar_mode = detailed` | [Tauri GUI 左侧栏标准](tauri_sidebar.md#详细模式) | `tauri-gui-sidebar-detailed-v1` |

一个界面可以同时命中通用标准和组件标准；更具体的规则只覆盖它明确声明的范围。未列出的产品页面继续遵守通用标准，并按真实任务补充设计。

## 变更治理

- 标准中的像素、组件层级、交互所有权和可访问名称是硬约束。改动必须先有用户明确批准，并以新的 ADR 说明理由、影响、风险和恢复标准。
- 下游像素偏离必须在 `docs/GUI_APP_PROFILE.md` 记录标准标识、精确差异和关联 ADR。Harness 源不创建虚假的产品 profile；它只维护目录、模板和初始化路由。
- 新标准应使用稳定标识，写清精确匹配条件、与其他标准的组合关系、实现边界和最小回归。不要用“更紧凑”“类似桌面应用”等无法机械复核的描述代替数值或语义。
- 其他规范、Skills 和模板只保留路由、技术补充或摘要，并链接回本目录；不要再复制一套可能漂移的 UI 像素事实。
