# AGENTS.md

## 项目使命

诡秘神谕 / LokiMetris 是独立终端下游项目，负责人 ManonLoki。产品为历史 LokiMetis 的 GPUI 原生体验复刻，当前边界见最新 `docs/product_spec/`；实施进度见当前 `docs/work_plan/`。本仓库不得继续派生项目。

## 启动门禁

1. 先读本文件及 `docs/AGENT_POLICY.md` 的 YAML frontmatter、字段语义；仅 Task、并行、构建或永久策略任务继续读对应章节。
2. 写入前确认 Git 顶层、分支和状态，保护已有修改；冲突无法安全归属时停止。
3. 根 `Cargo.toml` 是版本、接口、目标平台与框架事实源；当前 GUI / GPUI / Windows、macOS、Linux，不加载 Tauri、React、Mantine 专属规则。技术选型与依赖声明见 `docs/TECHNOLOGY.md`，Linux双架构工具链按 `docs/LINUX_GPUI_TOOLCHAIN.md` 渐进读取。
4. 按任务渐进读取，仅完整读取命中 Skill 及其精确引用，不预读全部记忆或同类 Skills。

## 按任务渐进读取

| 任务 | 事实来源 | 入口 |
|---|---|---|
| 产品目标、边界、约束、成功标准 | 存在时的规格索引/最新规格，必要时最新 ADR | `$desktop-define-product` |
| 实现、修复、工程维护 | `docs/ENGINEERING_RULES.md`、接口事实、版本门禁 | `$desktop-implement-change`、`$desktop-manage-version` |
| 行为保持的结构清理 | 工程规则、受影响模块 | `$desktop-refactor-code` |
| 新功能、独立修复、明确推送或发布 | Policy 的 Git 生命周期；发布再读 `docs/RELEASE.md` | `$desktop-manage-git-lifecycle`、发布用 `$desktop-prepare-release` |
| Rust core / GUI | `docs/RUST_CLI_TEMPLATE.md`、`docs/GUI_APP_PROFILE.md`，设计索引后只读 GPUI 命中标准 | `$desktop-add-gpui-adapter`、`$desktop-extract-i18n-strings` |
| GUI 身份或已批准改名 | GUI profile、两份许可证、产品事实 | `$desktop-prepare-gui-app-identity`、`$desktop-rename-project-identity` |
| 用户可见 Task | Policy 的完整 Task 开关、状态机和描述模板 | Codex 项目/Task 工具 |
| 当前 Task 内部并行或实际提交 | Policy 对应章节；提交规范 | `$desktop-run-parallel-worktrees`、`$desktop-configure-git-commits` |
| 真实测试/构建因受管环境失败 | 原失败诊断与精确门禁 | `$desktop-check-development-environment`；不得例行预检或全局安装 |
| 恢复、阻断、交接、显式持久计划 | 存在时最新 Product Status / Work Plan | `$desktop-plan-change`（仅真实触发） |
| GPUI 试包或候选 | 根 Cargo、发布规则、Policy 构建段 | `$desktop-build-gpui-release` |
| 最终候选 E2E、完整验收、取回 | 发布规则、存在时验证规则、当前忽略的 `release/` | `$desktop-test-final-artifact-e2e`、`$desktop-verify-delivery`、`$desktop-collect-release-artifacts` |
| 长期决定、例外、回顾 | 存在时记忆索引/最新文件，只追溯明确引用 | 事件记录流程；回顾用 `$desktop-summarize-development-history` |
| 发布后动作切换、工程升级 | 当前 Policy、工程事实与精确 Skill 引用 | `$desktop-switch-post-release-action`、`$desktop-upgrade-harness` |

## 始终生效的边界

- `superpowers: disabled` 时不得调用或遵循 `superpowers:*`。持久能力只表示允许，不能替代任务触发条件或授权。
- 日常直接实施，只运行本次相关非空单元/回归测试；纯文档/机械变更只做必要解析/完整性检查。不得自动增加计划、全仓检查、构建、冒烟、E2E、Verification 或人工复核。
- Core-first：宿主/接口无关的业务规则、值域、跨字段关系、工作流、权威状态及稳定领域错误属于 core；GUI 只拥有展示、交互、原生机制及映射。
- 优先标准库和已有依赖，不假设全局第三方包；未经用户明确要求不得全局安装。必要新依赖先说明、确认，并在项目内显式声明与隔离。
- `user_owned_tasks: disabled` 默认不自动拆左侧 Task，仍响应显式创建；永久手动切换按 Policy 确认并记 ADR。内部 plan/Subagent/Worktree/brief/report/review/checkpoint 不占序号，也不使用用户可见标题合同。
- 用户可见 Task 固定一个结果、范围、禁止范围、完成条件和独立工作区。结果类型、生命周期、外部副作用、禁止范围或验收责任变化须拆新 Task，同结果测试/必要返工不拆。每项目同时仅一个写入型 active Task；Task0 只协调。
- 标题为 `Task {序号} | {当前进度} | {单一结果}`。同一 `hostId`/精确 `projectId` 先清点 `list_threads(limit=50)` 与同宿主逐页 `list_archived_threads` 当前有效三字段标题，取最大有效序号加一；空历史从 1、缺号不回填，不识别旧格式。进度仅 `已分配`、`运行中`、`检查中`、`已完成`；真实转换至多更新一次，以真实 `threadId` 有界复读，失败报告且不伪写完成。
- 创建前先 `list_projects` 精确绑定路径/`projectId`/Git 状态，使用 Policy 完整描述模板；Git 固定独立 Worktree，非 Git 固定 Local。仅真实 `threadId` 可继续；`clientThreadId` 是 setup。核对标题、项目、cwd、状态、干净工作区和起始提交，否则零实现，不重复创建或代做。
- 新功能/独立修复写入前经生命周期 Skill 建立 `feature-{ascii-kebab摘要}-{YYYYMMDD}`。初始化后首次真实开发才创建 common-dir 生命周期清单；不按前缀扫描或删除未登记资源。
- 版本只经 `$desktop-manage-version`；受保护 `.harness/version-state.json` 不手改。疑似新需求优先 `feature`，本周期首个完成且通过相关测试的功能提升 Minor；确认非新需求的修复/可感知优化按稳定 ID `bug-fix` 提升 Patch。Minor/Patch 为 `0..99` base-100，历史 `100` 失败关闭。事实冲突先查底层漂移，再以稳定 ID/证据走 `record-reconciliation`，不重复消费事件。
- 明确发布时提交上下文，普通合并登记分支到本地默认主分支并复读 `v{版本}-{YYYYMMDD}`。主分支/tag 同一最终 HEAD 即完成 Git 发布；之后执行本次冻结的 `post_release_action`。当前 `local_package` 走 GPUI 打包；未来切换不改旧发布。推送、远端、凭据、签名和渠道分发遵守独立授权/门禁。
- 安全/隐私、不可逆操作、迁移、对外契约、生产/付费/凭据、签名、发布和跨平台最终候选进入专用门禁，偏好不绕过硬要求。普通实现细节不新增范围会议，真正产品边界歧义先确认。
- 完成/可用/已验证必须有真实可观察结果；中性 scaffold、开发预览、Mock/stub 不等于产品候选验收，人工批准不能把失败或未执行改为通过。
- 项目记忆只由独立事件触发，不迁移上游历史、不预建空规格/ADR/计划/变更/验证。候选构建/E2E/验收/就绪证据只写忽略的 `release/`；范围外问题写 `docs/TECH_DEBT.md`。
- 两份许可证完整保留；改名只替换精确双语名称。受管设计标准完整保留，产品像素偏离写 profile/ADR。

## Skills 地图

- GUI：`$desktop-add-gpui-adapter`、`$desktop-prepare-gui-app-identity`、`$desktop-rename-project-identity`、`$desktop-extract-i18n-strings`、`$design-taste-frontend`。
- 开发治理：`$desktop-define-product`、`$desktop-implement-change`、`$desktop-refactor-code`、`$desktop-manage-version`、`$desktop-plan-change`、`$desktop-manage-git-lifecycle`、`$desktop-configure-git-commits`、`$desktop-run-parallel-worktrees`、`$desktop-check-development-environment`、`$desktop-summarize-development-history`、`$desktop-switch-post-release-action`、`$desktop-upgrade-harness`。
- 构建验收：`$desktop-prepare-release`、`$desktop-build-gpui-release`、`$desktop-collect-release-artifacts`、`$desktop-test-final-artifact-e2e`、`$desktop-verify-delivery`。

## 约束地图

| 事实 | 唯一来源 |
|---|---|
| 产品目标/规格与状态 | 自身事件触发后建立的 `docs/product_spec/`、`docs/project_status/` 最新文件 |
| 策略、Task 描述模板、生命周期、发布后动作 | `docs/AGENT_POLICY.md` |
| 文件、中文注释、依赖、测试、文档事件、例外 | `docs/ENGINEERING_RULES.md` |
| 版本/接口/平台/框架与周期 | 根 `Cargo.toml`、受保护 `.harness/version-state.json`、版本 Skill |
| Rust 核心、GUI 运行时、工程命令 | `docs/RUST_CLI_TEMPLATE.md` |
| GUI 身份、九字段、热键合同、设计资源 | `docs/GUI_APP_PROFILE.md`、设计索引及命中标准 |
| 已登记 Git 资源与发布/推送进度 | Git common dir `agent-first-harness/git-lifecycle.json`、生命周期 Skill |
| 未来真实发布上下文/双语日志 | `.harness/release-context.json`、`release-notes.json`，首次发布才创建 |
| 发布/候选及长期审计 | `docs/RELEASE.md`、当前 `release/`；独立记录事件才建立验证文档 |
| 计划、决定与变更 | 事件触发后建立的 `docs/work_plan/`、`docs/adr/`、`docs/changelog/` 最新文件 |
| 技术债、许可证、升级 | `docs/TECH_DEBT.md`、两份根许可证、升级 Skill；缺溯源锁时先初始基线审计 |

## 每次任务的最小闭环

1. 确认唯一根和任务类型，按路由读取最少事实及完整命中 Skill。
2. 在授权范围实施，保护既有修改，完成必要验证。
3. 只更新独立事件触发的权威记录；范围内修复失败，新增副作用先确认。
4. 报告实际变化、验证、未执行项及风险，不把日常实现或中性初始化描述为发布就绪。
