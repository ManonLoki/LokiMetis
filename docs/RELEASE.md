# LokiMetis 版本、构建与发布

本文件是 LokiMetis 的语义化版本、本地发布说明、GUI 候选、manifest 和发布顺序的唯一事实来源。构建或测试成功不自动授权 tag、push、上传、签名、公证、安装、商店提交或渠道发布。

## 事实来源

- 当前版本：根 `Cargo.toml` 的 `[workspace.package].version`。
- 发布周期与缺陷去重：`.harness/version-state.json`；它不是第二份当前版本来源。
- 本地双语发布说明：根 `release-notes.json`。
- 目标平台与接口：根 `Cargo.toml` 的 `[workspace.metadata.agent-first-harness]`。
- Agent 冒烟/E2E 建议默认值：`docs/AGENT_POLICY.md`。
- 当前 GUI 身份、能力和 macOS 通知签名要求：`docs/GUI_APP_PROFILE.md`。

## SemVer

LokiMetis 使用无预发布/构建元数据的三段语义化版本，由 `$desktop-manage-version` 确定性管理：

- 新生成的 Minor 与 Patch 使用 `0..99` 的 base-100 数位：Patch 99 后自动进位 Minor 并归零，Minor/Patch 同为 99 时自动进位 Major；例如 `0.0.99 → 0.1.0`、`0.99.99 → 1.0.0`。自动进位不另算一次显式 Major 决策。
- Major 没有 99/100 的业务上限，但必须位于 Cargo/Rust `u64` 范围。显式 Major 只由用户批准精确目标 `N.0.0`，目标必须高于当前 Major，并视为包含本发布周期的首个功能提升。
- 每个正式发布周期的第一个已完成新功能提升一次 Minor；同周期后续功能只记录所需版本，不重复提升。Minor 提升将 Patch 归零。
- 每个具有新稳定 ID 的问题修复或用户可感知优化（机器分类 `bug-fix`）将 Patch 提升一次，且不受当前周期的功能提升锁影响；相同 ID 的补充、重试或跨周期再次处理不重复提升。历史 `bug-fix` ID 不得改作其他提升类别；正式发布后确认的回归使用新回归 ID。
- 查询、诊断、复现、未完成尝试、行为保持重构、内部优化、测试补强、文档、格式和清理使用 `maintenance`，不提升版本。`check`、`plan` 和 `maintenance` 始终零写入。
- 历史版本与受保护状态中的 Minor/Patch `100` 继续可读；只在实际 `apply` 提升时先规范化并按 base-100 进位，不能因只读检查或发布准备改写历史。
- 版本只在变化完成且本次相关测试通过后写入；构建、`pending` 候选、验收和失败发布只读核对。
- 只有正式渠道发布真实成功后才清空待发布变化并开启下一功能周期；历史缺陷 ID 永久保留用于去重。

版本变化与 Changelog 写入是独立门禁；Product Spec、ADR、Work Plan 也分别按自身事件触发。仅含普通缺陷修复或纯重构时不创建、不补写也不汇总按日 Changelog；这些变化仍进入本地发布说明。没有合格触发事件时，缺少 Changelog 不削弱发布证据。记录被自身事件触发时写稳定 `change_id` 与 `required_version`；版本提升本身不制造项目记忆，也不授权标签或发布。

## 用户可见版本与本地发布说明

- 面向用户显示的版本统一且只使用一个小写 `v` 前缀，包括侧栏、设置页、发布记录、产物名和本地发布说明。LokiMetis 的窗口标题是已批准例外，固定为 `LokiMetis` 且不带版本号。Cargo、版本状态和 manifest 的机器 `version` 字段不带前缀；展示边界先移除已有 `v`/`V` 再规范化。
- 根 `release-notes.json` 已存在。它使用 `schemaVersion: 2`，`releases` 按最新在前；每项字段为 `releaseDate`、带单个 `v` 的 `version`、`featureOptimizations` 与 `bugFixes`。
- 每个逻辑条目是键恰好为 `zh-CN` 与 `en-US` 的翻译对，两者都必须非空且无首尾空白。每版两类各最多 10 条、合计至少一条，只保留最近 5 个正式版本。
- 发布准备从上次真实发布的 40 位提交之后到当前源码的差异中语义整理重要内容；首个发布以仓库起点为边界。不得直接倾倒提交标题。普通缺陷修复即使不进入按日 Changelog，也进入本版 `bugFixes`。
- 同版本条目以当前真实内容替换后置顶。使用 `$desktop-prepare-release` 随附的标准库 helper 原子写入、`check --expected-version` 校验，并分别以 `zh-CN` 与 `en-US` render 复核。Harness 升级把该文件视为 `protected`。
- GUI 固定从打包本地资源读取发布说明并在设置页展示；没有联网检查、下载或安装行为。LokiMetis 的设置页赞助区、业务路由及其产品资产遵守已批准 Product Spec、GUI profile 与 ADR；不得据本文件新增未批准页面或导航。该边界不影响候选持续携带发布说明的制品契约。
- `release-notes.json` 与 `docs/changelog/` 职责独立：前者是正式发布制品的近五版用户摘要，后者是按日、按触发规则记录的项目变化。发布说明字节变化会产生新源码提交和新候选，必须重新构建与验收。

## 不支持的应用能力与制品

- 下游不安装或配置应用 updater，不实现联网检查、强制更新、自动下载或安装。
- 不生成应用更新归档、更新签名、channel/target 元数据、公钥验签证据或对应 manifest 字段。
- 不收集或发送产品统计、行为分析、崩溃报告、远程观测数据或其他远程遥测。
- 本地 `release-notes.json`、Harness 工程升级、SemVer、本地 tracing 和 GUI 性能采样均不构成上述应用能力。

## 本地开发试包

Windows 原生本地安装试包不是发布候选。普通“构建/打包/首次安装试一下”由 `$desktop-build-tauri-local-install` 处理：

- 在原生 Windows x64 生成未签名 NSIS 开发制品。
- 可以基于当前工作树，但必须报告 dirty 风险。
- 不读取或生成 `release-notes.json`，不传发布专用 Tauri 配置，不写 `release/`，不提交、不安装、不运行。
- 不解析候选 E2E 或性能选择，不声称可分发、已验收或发布就绪。

只有用户明确要求“发布候选”或“准备并构建发布”才进入后续流程。

## GUI 候选平台

- macOS 原生：Tauri DMG。
- Windows 原生 x64：Tauri NSIS。
- macOS 到 Windows x64：使用 cargo-xwin 构建 NSIS；不得生成或声称生成 MSI，也不得把交叉构建成功当作 Windows 原生安装、运行或性能证据。
- Linux：当前没有已批准的统一正式渠道合同；源码目标有效，正式产物保持 `Unverified`。

macOS 签名采用逐次意图与全有或全无门禁。`$desktop-prepare-release` 在任何发布提交前锁定 `macosSigningSelection`/`macosSigningSource`，来源按 `channel-required > requested > configured > not-requested` 唯一化。没有已批准配置、当次主动要求或渠道硬要求时默认 `disabled/not-requested`，使用 `--no-sign`，记录原因/风险且不得探测本机身份、证书、公证凭据或 profile。选择启用后，Developer ID Application、`notarytool`、`stapler`、完整凭据、签名、公证、stapling、Gatekeeper 和最终验证缺一不可；任一步失败都阻断，不能回退 unsigned。`system_notification = enabled` 的 macOS 候选必须启用并完成签名。任何后处理改变字节后重新计算摘要并重新验收。

最终 DMG 必须从当前最终字节只读挂载验证 Finder `.DS_Store`、本地背景、唯一顶层 `.app` 与 `/Applications` 链接。headless 环境不得无界等待 Finder AppleScript，也不得自动接受软件许可。

## 发布物命名与 manifest

LokiMetis GUI 产物使用：

`LokiMetis-v<MAJOR.MINOR.PATCH>-<platform>-<arch>.<ext>`

每个安装包或归档必须有相邻 `<artifact>.sha256` 和 manifest。`pending` manifest 至少包含：

- 项目、无前缀机器版本、批准的 40 位源码提交、构建模式、平台、架构、target、host 和产物名。
- 最终 SHA-256、全量 Rust/前端单元测试结果、签名/公证状态与 `milestoneAcceptance: pending`。
- 当前 `e2eSelection`、`releaseNotesVersion`、`releaseNotesSha256`、`releaseNotesPath: release-notes.json`。
- `reviewSelection: enabled | disabled` 与 `reviewStatus: passed | Not run`；启用时含结构化 `reviewEvidence`/`reviewedSourceCommit`，关闭且无硬要求时含非空原因/风险且证据字段缺席。
- `performanceSelection: enabled | disabled` 与 `performanceStatus: passed | waived | Not run | Unverified`。
- macOS 的 `macosSigningSelection`/`macosSigningSource`、`signingStatus`、`signingReason`、结构化 `signingEvidence` 与适用 `notarizationEvidence`。

性能选择启用或渠道要求且已原生测量时，manifest 使用 `performanceThresholdProfile: gui-release-v2`，保存与同一提交、平台、架构、native release-profile no-bundle 探针字节绑定的 `performanceEvidence`、`performanceProbeSha256` 和 `performanceRuntimeBinding`。v2 要求一次预热后恰好 5 次冷启动中位数 ≤2400ms、最大 ≤3600ms；至少 20 次代表性交互 nearest-rank p95 ≤120ms 且每次 <240ms；全部 ≥50ms Long Task 各 <240ms；整棵进程树 idle CPU p95 ≤6%（隐藏托盘 ≤2.4%）、稳态 RSS ≤360 MiB、峰值 ≤600 MiB、增长 ≤`max(18%, 38.4 MiB)`，且全部进程回收。`waived` 保留原始失败指标、修复尝试、风险、理由和用户确认，并要求原始证据 `waiverAllowed: true`；不能改判为 `passed`。选择关闭且无硬要求时状态固定 `Not run`，保存原因和剩余风险，且 `performanceEvidence`、`performanceProbe`、`performanceProbeSha256`、`performanceThresholdProfile`、`performanceWaiver` 与 `performanceRuntimeBinding` 全部缺席。xwin 在已启用性能但没有原生 Windows 测量时为 `Unverified`；渠道硬要求下不能进入 `accepted`。

manifest 不得包含 updater、更新签名/endpoint、产品统计或远程遥测字段。

## `release/` 原子刷新

所有下游候选写入项目根 `release/`，该路径由精确 `/release/` 规则忽略。目录存在不表示 `ready`。

每次候选构建或收集在接触目标目录前：

1. 验证独立 Git 根、具名 clean 分支与冻结 HEAD，拒绝 `release` 符号链接、重解析点和路径越界。
2. 在项目根同级、同一文件系统创建唯一、权限受限且目标外的 staging；构建、复制、展平和验证都只在 staging 完成，不得先清空或逐文件写入目标。
3. 所有布局、签名、公证、stapling 与包内资源变化完成后才计算最终摘要并写 manifest；随后按每份 manifest 重新枚举并复算大小、SHA-256、选择、证据和精确文件集。
4. 只有整个 staging 原子集合全部通过，才在不跟随链接的提交临界区隔离旧目录并以目录级原子替换提交新集合；提交失败恢复旧目标，任何前置失败不污染它。
5. 替换后只读重新枚举、复算摘要和精确集合，不再写入。候选状态更新同样复制整组到 sibling staging 后一次提交，绝不得逐 manifest 更新或形成 mixed 状态。

远端工作流必须检出并复核显式批准的 40 位提交，只上传 manifest 声明的精确文件。结果取回不得使用“最新”或修改时间猜测，也不得混入其他项目、版本、运行或额外文件。

## 构建、验收与发布顺序

发布语义审查、GUI 性能和 macOS 签名选择都只对当前发布有效，并在 `$desktop-prepare-release` 写元数据或提交前锁定；同一发布重跑复用，新发布重新解析。E2E 由构建单独逐次解析，`milestone_e2e` 只提供建议默认值。明确“发布/准备并构建发布”授权范围内本地提交与构建，不自动授权 tag、push、上传、商店提交或正式发布；这些外部动作仍需用户明确要求。

1. `$desktop-prepare-release` 先锁定 `reviewSelection`、`performanceSelection` 和 macOS 签名意图/来源，再用同时绑定具名分支与 HEAD 的检查快照复核工作树、范围、缓存和疑似秘密。归属明确的已完成源码按逻辑提交；无关/歧义改动、秘密、hook/签名交互或提交失败立即停止。helper 提交后要求同分支直接非 merge 子提交且 committed diff 与快照逐字节一致，拒绝 hook 夹带。clean 且无源码变化时不制造空提交。
2. 发布审查 `enabled` 时集中复核行为正确性、core/adapter 边界、对外契约、职责/规模候选和临时标记；只有结构化证据终点等于 reviewed source commit 才能 `passed`。`disabled` 且无硬要求时记录 `Not run`、原因/风险并让证据字段缺席。
3. 从新源码 HEAD 定位上次真实发布边界，原子生成/整理双语 `release-notes.json`；变化时形成独立发布元数据提交。最终工作树必须干净，`sourceCommit` 精确等于 HEAD，并把三项选择信封与它绑定。
4. `$desktop-build-tauri-release` 只读消费选择，校验版本、发布说明、GUI 固定结构和零 updater/统计/远程遥测，再以 `--list` 或等价方法证明 Rust 与前端测试非空，并运行全部单元测试。
5. 性能为 `enabled` 或渠道强制时，从同一 clean HEAD 生成 `gui-release-v2` release-profile no-bundle 探针。每次预热/冷启动前恢复同一隔离 window-state 基线，所有退出路径恢复用户原状态；测量冷启动、交互/Long Task、整进程树 CPU/RSS、内存增长和退出回收。窗口状态、整进程树、探针字节或进程回收失败均不可豁免；纯指标失败才可在 `waiverAllowed: true` 后由用户显式 waiver。
6. 性能为 `passed`、合法 `waived`，或明确关闭时的完整 `Not run` 契约，才进入适用 DMG/NSIS 打包。macOS 签名 disabled 使用 `--no-sign` 且零探测，enabled 则签名/公证/stapling/Gatekeeper 全有或全无；通知启用要求签名。最终字节形成后计算摘要，在 staging 写并复算 `pending` manifest，再原子替换 `release/` 并只读重验。
7. `$desktop-verify-delivery` 对最终候选运行持久策略要求的冒烟、当前 E2E 选择和渠道硬要求，并复核同一候选的发布审查、性能、发布说明、DMG 布局、签名与 manifest。失败、超时、取消或已选未执行都返回开发循环。
8. 所有 required/enabled 检查和人工复核通过后，才在 sibling staging 对整组 manifest 原子写入一致的 `accepted`；拒绝为整组 `rejected`，等待人工为整组 `pending`，禁止 mixed 状态。替换前后都复算最终字节与精确集合。渠道发布真实成功后才调用 `$desktop-manage-version finalize-release`。
9. 发布说明、签名、公证、stapling、重打包或渠道处理只要改变运行字节、启动器、依赖或行为，就形成新候选并回到步骤 4；旧审查、性能或验收证据不得复用。

构建、收集、E2E、完整验收和就绪复核不创建或更新 Product Spec、ADR、Changelog、Product Status、Work Plan 或 Verification；候选事实只写忽略的 `release/` manifest、相邻证据和最终回复。只有真实渠道发布成功后的受管记录阶段、人工复核或长期审计被独立触发时，才按对应事实源留证。

## 许可

- 源码归档、安装包和其他分发物必须以用户可访问方式携带根目录两份企业专有商业许可证，且项目名与发布产品名一致。
- 分发前生成并核对该版本的第三方依赖许可证与 NOTICE。专有许可不替代或限制第三方许可证依法授予的权利。
- 商业合同或订单决定客户、费用、期限、授权数量和特殊授权；仓库许可证不得误报为双方签署的合同。
