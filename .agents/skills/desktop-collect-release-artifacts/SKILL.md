---
name: desktop-collect-release-artifacts
description: 在项目根 release 目录安全合并并验证已完成的 GUI 本地、交叉或原生 CI 候选，同时保留 pending、rejected 或 accepted 状态。
---

# 收集 GUI 发布制品

1. 读取 `docs/RELEASE.md`、候选 manifest、根版本状态与 GUI profile。只接受同一项目、版本、具名 clean 分支 40 位 source commit 和构建身份的 Tauri DMG/NSIS 候选；锁定当前 HEAD、分支和本次发布审查/性能/macOS 签名选择快照。
2. 构建完整源清单：每个文件必须非空、普通、非符号链接，位于目标目录之外，摘要与 manifest 匹配。拒绝缺失、额外、重复、旧版本、跨项目或冲突文件。
3. 目标固定为规范化 `<project-root>/release`，不得是项目根、符号链接/重解析点或越界路径。在项目根同级、同一文件系统创建唯一、权限受限且目标外的 staging；复制、展平、摘要与精确集合复核都只在 staging 完成，不依赖 ignore 隐藏工作树内暂存项。
4. 原样保留 `milestoneAcceptance`、`e2eSelection`、`reviewSelection`/`reviewStatus`、`performanceSelection`、macOS 签名选择与 `releaseNotesVersion`。`pending` 构建可以收集，但发布就绪复核仍要求所有匹配 manifest 组成完整 `Milestone accepted` 原子集合；不得把未验证状态改写为通过。
5. `reviewSelection: enabled` 时要求结构化 `reviewEvidence`、`reviewedSourceCommit` 和 `passed`，并验证证据终点/祖先绑定；`disabled` 且无硬要求时只接受 `Not run`、非空原因/风险，且证据字段缺席。机械文件/摘要检查不能冒充语义审查。
6. `performanceSelection: enabled` 时只接受 `performanceThresholdProfile: gui-release-v2` 的原生 `passed|waived` 证据，或 xwin 的准确 `Unverified`。原生证据必须复核 `performanceProbeSha256`、release-profile/native/source/platform/arch 绑定和 `performanceRuntimeBinding`；安装包摘要不能冒充探针摘要，`waived` 必须继续引用原始 `failed` 证据。`disabled` 且无硬要求时只接受 `performanceStatus: Not run`、非空 `performanceReason`/`performanceRemainingRisk`，且 `performanceEvidence`、`performanceProbe`、`performanceProbeSha256`、`performanceThresholdProfile`、`performanceWaiver` 和 `performanceRuntimeBinding` 全部缺席。`e2eSelection` 同样只能保留构建产生的真实选择与证据。
7. macOS 要求 `macosSigningSelection`/`macosSigningSource` 与证据一致，来源按 `channel-required > requested > configured > not-requested` 唯一化。disabled 时保持 unsigned 且不得有探测派生签名/公证证据；enabled 时只接受结构化 `signingEvidence` 与 `notarized-and-stapled`。`system_notification = enabled` 时 disabled/unsigned 必须拒绝。xwin NSIS 要求 `buildMode: cross-compiled-xwin` 与 `runtimeVerification: Unverified`。只验证既有签名/公证，不尝试新签名。
8. 候选只含安装包、release notes、manifest、相邻摘要和适用审查/性能/验收证据；拒绝应用内版本更新 archive、feed 或相关签名字段。
9. 在 staging 内重新计算每个最终文件的大小与 SHA-256、复核 manifest 精确集合，并在触碰目标前重验当前分支/HEAD/选择快照。只有全部来源一起通过，才以不跟随链接的目录级原子替换提交到 `release/`；提交失败恢复原目标。替换后只读重新枚举、复算摘要与精确集合，不再写入。绝不得产生 accepted/pending、accepted/rejected 或其他 mixed 状态。收集阶段不重新构建、签名或改变候选字节；前置失败不得部分污染目标。

报告收集来源、精确文件集、摘要、平台、构建模式、发布审查/性能/签名/E2E/验收状态、原子替换与替换后复核结果及未验证边界；候选事实只留在忽略的 `release/` 和最终回复，不上传、不发布、不重写项目记忆。
