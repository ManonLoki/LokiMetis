# AGENTS.md

## 项目使命

本仓库是诡秘神谕（LokiMetis）的唯一终端项目根目录，维护 `loki_metis_core + loki_metis_gui` 的 GUI-only 本机 AI 工作台，不再用于创建其他项目。当前产品目的、MVP、业务范围和成功标准以最新 Approved Product Spec 为准。

## 启动门禁

1. 先读取本文件，再读取 `docs/AGENT_POLICY.md` 的 YAML frontmatter；只有改变持久策略时才继续读取其正文。
2. 判断任务类型，只读取下表命中的事实源与 Skill。产品业务请求必须先通过 `$desktop-define-product` 建立批准规格；不得把中性脚手架自行解释为产品需求。
3. 写入前确认当前目录是独立 Git 根、当前分支与工作区状态，保护已有修改。重叠改动无法安全处理时停止；不得虚构命令、能力、产物或验证结果。
4. 安全、隐私、迁移、破坏性操作、生产/付费/凭据副作用、对外兼容、签名与发布必须进入对应门禁。普通实现细节在既定范围内直接处理。

## 按任务渐进读取

| 当前任务 | 读取内容 | 执行入口 |
|---|---|---|
| 定义或改变产品目标、范围、约束、成功标准 | 已有最新 Product Spec（若存在），必要时再读相关 ADR | `$desktop-define-product` |
| 范围清楚的实现或缺陷修复 | `docs/ENGINEERING_RULES.md`、`docs/RUST_GUI_TEMPLATE.md`，再读版本分类 | `$desktop-implement-change`、`$desktop-manage-version` |
| 行为保持的结构清理 | 上述工程事实与本次命中的源码 | `$desktop-refactor-code` |
| GUI 展示、交互、身份或桌面能力 | `docs/GUI_APP_PROFILE.md`、`docs/design_standards/README.md` 和精确命中标准 | 对应 GUI Skill |
| 国际化字符串抽取 | GUI profile、真实前端与 Rust 原生文案 | `$desktop-extract-i18n-strings` |
| 环境故障恢复 | 真实失败和所需工具链事实 | `$desktop-check-development-environment` |
| Git 配置或即将创建提交 | 独立仓库状态与提交要求 | `$desktop-configure-git-commits` |
| 用户要求持久计划、交接或高风险协调 | 当前范围、依赖和已有最新计划（若存在） | `$desktop-plan-change` |
| Windows 本地安装试包 | 根目标平台事实与本地试包规则 | `$desktop-build-tauri-local-install` |
| 准备、构建或收集发布候选 | `docs/RELEASE.md` 与当次 E2E/性能选择 | `$desktop-prepare-release`、`$desktop-build-tauri-release`、`$desktop-collect-release-artifacts` |
| 最终产物 E2E、性能或完整验收 | `docs/RELEASE.md`、候选 manifest 与精确证据 | `$desktop-test-final-artifact-e2e`、`$desktop-test-gui-release-performance`、`$desktop-verify-delivery` |
| 更新工程规则或维护工具 | 当前受保护事实与明确提供的新工程源 | `$desktop-upgrade-harness`，默认先预览 |

## 始终生效的边界

- `superpowers: disabled` 时不得调用或遵循任何 `superpowers:*` Skill。
- Core-first 是硬规则：平台无关业务类型、规则、值域、用例、状态转换、持久化策略与稳定错误属于 `loki_metis_core`；Tauri/React 只承担展示、IPC 映射和宿主机制。
- 接口固定为 GUI。不得新增 CLI、TUI、MCP adapter，也不得接入应用 updater、联网检查、强制更新、更新制品、产品统计或远程遥测。
- GUI 必须与 `docs/GUI_APP_PROFILE.md` 一致：托盘、通知、自启、单实例和受限深链接启用；赞助页与全局快捷键禁用；侧栏固定为 compact。禁用能力必须零依赖、零配置、零命令、零路由、零状态和零运行时资源。
- 日常开发直接实施，只运行本次需要的相关非空单元/回归测试。不得因任务复杂、多模块或 Agent 偏好自动增加持久计划、全仓检查、构建、E2E、Verification 或人工复核。
- 已初始化项目的版本只由 `$desktop-manage-version` 管理；根 `Cargo.toml` 是当前版本事实源，`.harness/version-state.json` 是受保护的周期与缺陷去重状态。
- 对产物声称“完成”“可用”或“已验证”必须有真实可观察结果；mock、stub、源码片段、中性页面或开发预览不能冒充候选验收。
- Product Spec、ADR、Changelog、Product Status、Work Plan、Verification 和技术债只由各自独立事件触发；普通开发不创建占位记录。
- 不覆盖或撤销用户已有修改，不为假想未来增加抽象、能力或依赖；跨平台实现不得默认单一 Shell、路径、权限模型或宿主能力。

## Skills 地图

- GUI 基线与能力：`$desktop-add-gui-adapter`、`$desktop-add-gui-system-tray`、`$desktop-add-gui-system-notifications`、`$desktop-add-gui-autostart`、`$desktop-add-gui-single-instance`、`$desktop-add-gui-deep-link`、`$desktop-prepare-gui-app-identity`、`$desktop-prepare-gui-support-surfaces`、`$desktop-extract-i18n-strings`、`$desktop-rename-project-identity`。
- 开发与治理：`$desktop-define-product`、`$desktop-implement-change`、`$desktop-refactor-code`、`$desktop-manage-version`、`$desktop-plan-change`、`$desktop-check-development-environment`、`$desktop-configure-git-commits`、`$desktop-upgrade-harness`。
- 构建与验收：`$desktop-build-tauri-local-install`、`$desktop-prepare-release`、`$desktop-build-tauri-release`、`$desktop-collect-release-artifacts`、`$desktop-test-gui-release-performance`、`$desktop-test-final-artifact-e2e`、`$desktop-verify-delivery`。

## 约束地图

| 事实 | 唯一来源 |
|---|---|
| 产品目标、范围和成功标准 | `$desktop-define-product` 创建的最新 Approved Product Spec |
| Agent 能力与候选建议默认值 | `docs/AGENT_POLICY.md` |
| 文件、注释、文档、测试与例外 | `docs/ENGINEERING_RULES.md` |
| Rust core、Tauri GUI 与运行时 | `docs/RUST_GUI_TEMPLATE.md` |
| GUI 身份、八项选择与深链接 | `docs/GUI_APP_PROFILE.md` |
| UI 布局、交互与 compact 侧栏 | `docs/design_standards/README.md` 及其索引标准 |
| 当前版本与目标平台 | 根 `Cargo.toml` |
| 发布周期与缺陷去重 | `.harness/version-state.json` |
| 构建、候选和发布 | `docs/RELEASE.md` |
| 商业许可 | `LICENSE.zh-CN.md`、`LICENSE.en.md` |

事实冲突时先调查并修正，不选择更方便的说法。不存在的记录表示相应事件尚未触发，不能以空白文档替代。

## 每次任务的最小闭环

1. 判定任务与事实源，只读取精确命中的最少资料和完整 Skill。
2. 在授权范围内实施，保护现有修改，只运行本次变化需要的测试或最小替代检查。
3. 只更新被真实事件触发的权威记录；失败在当前范围内修复并重跑，新增副作用或范围变化先取得授权。
4. 完成时报告实际变化、实际验证、未执行项和剩余风险，不把日常实现描述为发布就绪。
