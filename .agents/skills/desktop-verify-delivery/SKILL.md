---
name: desktop-verify-delivery
description: 对 GUI 发布候选、用户明确要求的完整验收或当次启用的 E2E 执行最终验证；性能按 manifest 当次选择复核。
---

# 验证 GUI 交付

## 准入

1. 只接受发布/渠道要求、用户明确完整验收，或当前构建 `e2eSelection: enabled`。读取 Product Spec、构建 manifest、Agent Policy、工程/发布/验证规则及 GUI profile。没有 Work Plan 不阻断验收。
2. 要求候选绑定批准场景、40 位 source commit、版本、平台、架构、校验和与完整 manifest。调用 `$desktop-manage-version check --phase release`；验收不提升版本。
3. 拒绝源码片段、占位、中性脚手架、开发预览、推测路径或旧证据。

## 验证

1. 核对当前构建已经运行项目全部非空单元测试，包括同一提交的 Rust workspace/all-targets/all-features 锁定测试与完整前端测试；源码或锁文件变化时返回构建流程。
2. 核对 `releaseNotesVersion`、`releaseNotesSha256`、`releaseNotesPath: release-notes.json`，对可定位资源运行字节检查；macOS 最终 DMG 重新只读验证布局和包内 release notes。
3. 按 manifest 的 `performanceSelection` 复核：enabled 要求同一 commit/platform/arch 的原生 `passed` 或保留失败证据的明确 `waived`；xwin 为 `Unverified`。disabled 且无硬要求时只接受 `Not run`、非空原因/风险且无探针字段。
4. manifest 的 `e2eSelection` 记录 E2E enabled 或渠道硬要求时调用 `$desktop-test-final-artifact-e2e`；`disabled` 时记录 `Not run`、原因与剩余风险。所有真实候选都从设置页打开本地 release notes，并确认 `about_page`、About 路由/入口/资源缺席。
5. 验证安装包、manifest 和 bundle 没有应用内版本服务、远程 feed 或产品统计传输残留。
6. 记录候选、命令/场景、预期/观测、清理、运行时验证、性能、签名/公证、跳过项和未验证平台。凭据、生产数据、支付或不可逆副作用仍需独立授权。

## 结论

- `Milestone accepted`：全部必需场景与门禁通过，适用人工复核已记录；
- `Awaiting human review`：自动证据通过但必需人工签署未完成；
- `Milestone rejected`：任一必需条件失败并记录修复回流。

验收不授权 tag、push、上传、发布、重新签名或破坏性清理；失败候选不得降级为“部分通过”。只写被验收或重要阻断真实触发的记录，不创建无触发原因的记忆占位；只有用户要求的活动计划存在时才重开或新增 Todo。
