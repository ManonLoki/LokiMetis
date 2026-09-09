---
name: desktop-verify-delivery
description: 对 GUI 发布候选、用户明确要求的完整验收或当次启用的 E2E 执行最终验证；性能按 manifest 当次选择复核。
---

# 验证 GUI 交付

## 准入

1. 只接受发布/渠道要求、用户明确完整验收，或当前构建 `e2eSelection: enabled`。读取 Product Spec、构建 manifest、Agent Policy、工程/发布/验证规则及 GUI profile。没有 Work Plan 不阻断验收。
2. 要求候选绑定批准场景、具名 clean 分支的 40 位 source commit、版本、平台、架构、校验和与完整 manifest；锁定当前 HEAD/分支及本次 `reviewSelection`、`performanceSelection`、macOS 签名选择快照。调用 `$desktop-manage-version check --phase release`；验收不提升版本或重置周期。
3. 拒绝源码片段、占位、中性脚手架、开发预览、推测路径或旧证据。

## 验证

1. 核对当前构建已经运行项目全部非空单元测试，包括同一提交的 Rust workspace/all-targets/all-features 锁定测试与完整前端测试；源码或锁文件变化时返回构建流程。
2. 核对 `releaseNotesVersion`、`releaseNotesSha256`、`releaseNotesPath: release-notes.json`，对可定位资源运行字节检查；macOS 最终 DMG 重新只读验证布局和包内 release notes。
3. 按每个 manifest 的 `reviewSelection` 复核发布语义审查。`enabled` 只接受 `reviewStatus: passed`、结构化 `reviewEvidence` 与 `reviewedSourceCommit`，且证据终点/累计范围与候选 source commit 的祖先关系一致；审查后源码变化使证据失效。`disabled` 只有无安全、隐私、不可逆操作、对外兼容契约或产品/渠道硬要求时才接受 `reviewStatus: Not run`、非空 `reviewReason`/`reviewRemainingRisk`，并要求证据字段缺席。
4. 按每个 GUI manifest 的 `performanceSelection` 条件复核性能，E2E 不能替代该结论。`enabled` 复核打包前 `$desktop-test-gui-release-performance` 的 release-profile no-bundle 探针，要求 `performanceThresholdProfile: gui-release-v2` 及同名证据阈值、同一 commit/platform/arch/native 绑定，状态为 `passed` 或用户看过保留的 `failed` 指标后明确 `waived`；xwin 为准确 `Unverified`。复核 `performanceProbeSha256` 与 `performanceRuntimeBinding`，证明探针、打包前 runtime 和包内签名前/签名后字节关系；DMG/NSIS 容器摘要不能冒充探针摘要。`disabled` 且无硬要求时只接受 `performanceStatus: Not run`、非空 `performanceReason`/`performanceRemainingRisk`，且 `performanceEvidence`、`performanceProbe`、`performanceProbeSha256`、`performanceThresholdProfile`、`performanceWaiver` 和 `performanceRuntimeBinding` 全部缺席。
5. macOS 核对 `macosSigningSelection`/`macosSigningSource`，来源按 `channel-required > requested > configured > not-requested` 唯一化。disabled 时必须 unsigned、保留原因/风险且没有探测派生签名/公证证据；enabled 时只接受完整结构化 `signingEvidence`、签名、公证、stapling 与 Gatekeeper 证据，不能回退 unsigned。`system_notification = enabled` 的 macOS 候选若 disabled/unsigned 必须拒绝，不能被 E2E 关闭、waiver 或 `Unverified` 绕过。
6. manifest 的 `e2eSelection` 记录 E2E enabled 或渠道硬要求时调用 `$desktop-test-final-artifact-e2e`；`disabled` 时记录 `Not run`、原因与剩余风险。所有真实候选都从设置页打开本地 release notes，并确认固定支持页面、导航和专属资源精确匹配仅含设置页的允许集合。
7. 验证安装包、manifest 和 bundle 没有应用内版本服务、远程 feed 或产品统计传输残留。全部真实检查、E2E、清理和人工结论完成后，再次核对当前分支/HEAD/选择快照，重新计算全部最终制品、相邻摘要、manifest 声明、包内关键资源和 `release/` 精确集合；不能沿用 E2E 前摘要。
8. 只在尾门禁通过后，于项目根同级、同一文件系统的唯一受限 staging 复制当前 `release/` 精确集合，原子写入所有 manifest 的同一整组 `milestoneAcceptance` 及结构化验收证据，并重算被修改 manifest、相邻摘要、包内资源绑定和精确文件集。`Milestone accepted` 整组全为 `accepted`，拒绝整组全为 `rejected`，等待人工签署整组保持 `pending`；绝不得产生 mixed 状态。全部复验后才目录级原子替换 `release/`，失败保留旧集合。替换后只读重新枚举并复算，不再写入。
9. 候选验收证据只写入忽略的 `release/` manifest 及其声明证据和最终回复，不得更新 tracked Product Spec、Changelog、Product Status、Work Plan 或 Verification，也不得创建占位记录。记录精确候选、命令/场景、预期/观测、清理、运行时验证、审查、性能、签名/公证、跳过项和未验证平台。凭据、生产数据、支付或不可逆副作用仍需独立授权。

## 结论

- `Milestone accepted`：全部必需场景与门禁通过，适用人工复核已记录；
- `Awaiting human review`：自动证据通过但必需人工签署未完成；
- `Milestone rejected`：任一必需条件失败并记录修复回流。

三种结论都必须走最终字节复算与 staging 原子替换；`Awaiting human review` 对应整组 `pending`，拒绝对应整组 `rejected`，只有接受对应整组 `accepted`。性能豁免不等于性能通过，主动关闭也不等于性能已验证。验收不授权 tag、push、上传、发布、重新签名或破坏性清理；失败候选不得降级为“部分通过”。只有用户要求的活动计划存在时才重开或新增 Todo，但候选状态和证据不得回写 Work Plan。
