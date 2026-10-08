---
name: desktop-switch-post-release-action
description: 为已初始化下游明确选择或切换未来 Git 发布完成后的固定动作，并安全迁移旧 Agent 策略。只接受本地打包或推送 release 分支。
---

# 切换发布后动作

在终端下游项目中，按用户本次明确选择持久化 `docs/AGENT_POLICY.md` 的 `post_release_action`。这个选择只约束未来完成的 Git 发布，不执行当前发布、不构建、不修改远端。

## 固定选择

| 用户选项 | 持久值 | 未来 Git 发布完成后的流程 |
|---|---|---|
| 本地打包（初始化表单默认项） | `local_package` | 进入已有本地候选打包 Skill；其构建、E2E、签名与制品门禁保持原规则。 |
| 提交远程 | `push_release_branch` | 按发布流程把已发布代码提交到 `release` 分支并同步用户已配置远端的默认主分支、`release` 分支和 tag；三者复读为同一已发布 HEAD 后该流程结束。 |

表单默认项只决定表单初始显示；旧项目升级时不得用默认值替用户作选择。用户已选择后可再次调用本 Skill 改变未来流程。选择不回溯改变已完成发布或候选。

GUI 必须先读取 Cargo `gui-framework`，旧 GUI 缺省为 Tauri，非法值拒绝。GUI 目标含 macOS/Windows 时，两种框架均可选择 `local_package`：Tauri 使用 `$desktop-build-tauri-release`，GPUI 使用独立 `$desktop-build-gpui-release` 生成本机 macOS `.app`/DMG 或 Windows NSIS。GPUI 检查实际 `gui-root`、受 Git 跟踪的根/GUI Cargo 与 Rust 测试清单，以及适用 Cargo 锁策略；前端/Tauri清单、项目本地 Tauri CLI 和 pnpm 锁文件检查只适用于 Tauri。不含 CLI 的仅 Linux GUI 必须选择 `push_release_branch`。

## 执行

1. 读取下游 `AGENTS.md`、`docs/AGENT_POLICY.md`、`docs/RELEASE.md`、ADR 索引与当前最新 ADR；确认规范 Git 顶层、当前分支和工作区。`docs/AGENT_POLICY.md` 是 protected 下游事实，升级器的候选树、`apply` 与 `record` 不得写它。不得在带有 Harness `Version.md` 和实例化 Skill 的模板根运行。
2. 只读检查现有状态：`node .agents/skills/desktop-switch-post-release-action/scripts/post_release_action.mjs inspect --project-root "<downstream-root>"`。`schema_version: 3` 只表示旧项目缺少选择，返回 `status: selection_required`；schema 4 必须有合法 `post_release_action`。其他 schema、字段缺失、重复或非法值均失败关闭，不得把损坏状态当成未选择。读取生命周期 `inspect` 的 `lastRelease.postReleaseAction`；它是上次发布冻结的动作，`null` 代表升级前旧发布没有可恢复动作，不得用本次新选择补做该旧发布。旧 schema 4 若正文尚未合并默认主分支、release 和 tag 同步规则，先保留 frontmatter 和自定义规则迁移正文，再 inspect/check，无需重新选择动作。旧 schema 3 的正文常保留“发布后另行请求推送/打包”等旧规则：升级器把该文件视为 protected，不会替项目覆盖它。读取源 Harness 当前 `docs/AGENT_POLICY.md` 的发布后动作、初始化、开发与构建段，逐处合并到下游受保护策略正文，保留项目自己的其他规则，并移除相反的旧句；helper 将在写入前检查合并结果，不允许只改 frontmatter 就宣布迁移成功。
3. 初次选择和每次切换都向用户呈现上表两项，取得本次明确选择。`local_package` 只适用于根 Cargo 接口元数据含 CLI，或含 GUI 且目标平台含 macOS/Windows；纯 TUI/MCP 或仅 Linux GUI 没有现成的本地打包 Skill，必须明确选择 `push_release_branch`。已有 GUI 项目选择本地打包前还须由 helper 核对实际 `gui-root`、`rust-test-manifests`、受 Git 精确跟踪的根 Cargo、所选 GUI Cargo 与非空 Rust 测试清单。Tauri 另须核对受 Git 跟踪的前端 `package.json`、Tauri 配置与项目本地 CLI；GUI 根目录的 `Cargo.toml` 与 `tauri.conf.json`，或其 `src-tauri/` 对应文件，必须成对存在，两处同时有 Tauri 配置时失败关闭。GPUI GUI 根只要求真实原生 Rust package，不要求 Tauri/前端文件。项目锁文件不属于这项默认检查。接口和平台匹配只表示路线种类可选，不证明项目已能打包。旧项目补选不推断；当 `inspect` 已有相同选择且用户没有提出变更时，保持字节不变，直接运行第 5 步。需要写入时，先由 `$desktop-manage-version` 执行 `node .agents/skills/desktop-manage-version/scripts/version_gate.mjs plan --project-root "<downstream-root>" --kind maintenance` 并保存返回的 `required_version`；若旧项目仍缺 `.harness/version-state.json`，先完成升级 Skill 规定的显式版本状态迁移，不能先写策略。随后使用 `inspect` 返回的当前值作为 `--expected-action`；旧 schema 3 使用 `missing`。例如选择本地打包：`node .agents/skills/desktop-switch-post-release-action/scripts/post_release_action.mjs set --project-root "<downstream-root>" --action local_package --expected-action missing --confirmed-user-choice`。切换到远端时把 `--action` 改为 `push_release_branch`，并把 `--expected-action` 改为已读的当前值。`--confirmed-user-choice` 只在实际得到用户选择后传入；它不是替用户确认的默认参数。helper 仅用 Node.js 标准库，原子替换策略 frontmatter 并保留已合并的正文与换行；预期值漂移会阻断覆盖。
   根 Cargo 元数据显式设置 `dependency-lock-policy = "tracked"` 时，本地打包检查还要求实际 Cargo workspace 的 `Cargo.lock` 受 Git 跟踪；Tauri GUI 另要求所选 `gui-root` 的 `pnpm-lock.yaml` 受 Git 跟踪，GPUI 不要求 pnpm 锁文件。缺省策略不要求项目锁文件。
4. `changed: true` 时按 `docs/adr/README.md` 在选择当天的唯一 `YYYYMMDD_ADR.md` 记录新长期决定及 `change_id`、第 3 步已复核的 `required_version`、旧值、新值、适用未来发布、理由、风险和调整标准，并更新 ADR 索引。同值 `changed: false` 是幂等 no-op，不重复写 ADR 或版本状态。修改后的策略与 ADR 属于用户这次批准的下游治理变更；不要把它伪装成升级器自动覆盖 protected 文件。
5. 运行强制流程检查：`node .agents/skills/desktop-switch-post-release-action/scripts/post_release_action.mjs check --project-root "<downstream-root>"`，要求输出 `schema_version: 4`、`status: configured` 与用户选定值，再复读策略文件、实际改动和相应 ADR。`selection_required`、非法 schema/值、未复核的写入或 ADR 缺失都不能报告切换/升级闭环完成。旧项目工程层升级可先记录 `upstream-lock.json`，但本步骤完成前必须分别报告“工程层已升级”和“发布后动作未补选”，不得合并为完整升级。

## 边界

- 只能写入终端下游自己的 `docs/AGENT_POLICY.md` 与真正触发的 ADR；不得修改 `.harness/release-context.json`、Git common-dir 状态、构建证据、远端、分支或 tag。
- `set` 要求完整且已确认的旧 schema 3 或合法 schema 4，绝不自行补填旧策略中缺少的其他字段。旧策略历史事实不明时先修复原状态，再执行选择。
- 本 Skill 不改变本地打包 Skill 的候选要求，也不把“切换为提交远程”解释成对旧发布的远端推送授权。每次新发布开始时才把当前选择冻结到 Git 生命周期记录；旧发布的重试和恢复始终按旧快照，未来推送仍按目标项目的发布后分支门禁和已配置远端执行。

## 输出

报告先前选择、本次选择、是否实际改变、策略 schema 与 `check` 结果、ADR 条目及未执行的构建/远端动作。
