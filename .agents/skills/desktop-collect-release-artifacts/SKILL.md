---
name: desktop-collect-release-artifacts
description: 在项目根 release 目录安全合并并验证已完成的 GUI 本地、交叉或原生 CI 候选，同时保留 pending、rejected 或 accepted 状态。
---

# 收集 GUI 发布制品

1. 读取 `docs/RELEASE.md`、候选 manifest、根版本状态与 GUI profile。只接受同一项目、版本、40 位 source commit 和构建身份的 Tauri DMG/NSIS 候选。
2. 构建完整源清单：每个文件必须非空、普通、非符号链接，位于目标目录之外，摘要与 manifest 匹配。拒绝缺失、额外、重复、旧版本、跨项目或冲突文件。
3. 目标固定为规范化 `<project-root>/release`，不得是项目根、符号链接或越界路径。独立调用时只使用 Tauri release helper 安全刷新目标。
4. 原样保留 `milestoneAcceptance`、`e2eSelection`、`performanceSelection` 与 `releaseNotesVersion`。pending 构建可以收集，但发布就绪要求匹配的 accepted 证据；不得把未验证状态改写为通过。
5. `performanceSelection: enabled` 时只接受 `passed|waived` 的原生证据，或 xwin 的准确 `Unverified`；`disabled` 且无硬要求时只接受 `Not run`、原因/剩余风险和全部探针字段缺席。`e2eSelection` 同样只能保留构建产生的真实选择与证据。
6. DMG 已签名时要求 `notarized-and-stapled`；xwin NSIS 要求 `buildMode: cross-compiled-xwin` 与 `runtimeVerification: Unverified`。只验证既有签名/公证，不尝试新签名。
7. 候选只含安装包、release notes、manifest、相邻摘要和适用性能/验收证据；拒绝应用内版本更新 archive、feed 或相关签名字段。
8. 在同根暂存目录完成逐文件复制、摘要复核、manifest 集合复核后原子替换 `release/`。收集阶段不重新构建、签名或改变候选字节；失败时不破坏已有有效目录。

报告收集来源、精确文件集、摘要、平台、构建模式、性能/E2E/验收状态、原子替换结果与未验证边界；不上传、不发布、不重写项目记忆。
