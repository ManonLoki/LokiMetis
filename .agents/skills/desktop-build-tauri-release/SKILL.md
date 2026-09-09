---
name: desktop-build-tauri-release
description: 从 clean HEAD 构建 GUI-only Tauri 2 发布候选，支持 macOS 原生 DMG、Windows 原生 x64 NSIS 与 macOS xwin NSIS，并按当次选择执行性能和 E2E 门禁。
---

# 构建 Tauri 发布候选

## 准入

1. 只接受用户明确提出的发布候选请求。普通本机安装检查交给 `$desktop-build-tauri-local-install`。
2. 读取产品事实、GUI profile、Agent Policy、`docs/RELEASE.md`、根 Cargo/前端锁文件、Tauri 配置与 [平台路线](references/tauri-macos-windows.md)。根 metadata 必须精确为 `interfaces = ["gui"]`，目标平台包含本次候选平台。
3. 项目必须是独立 Git 根，有 40 位 `HEAD` 且 `git status --porcelain=v1 --untracked-files=all` 为空。锁定 `buildSourceCommit`；构建期间任何 HEAD 或工作树漂移都停止。
4. 在测试或编译前只解析本次 `e2eSelection`；产品/渠道硬要求优先，否则用户未明确时询问一次。`reviewSelection`、`performanceSelection` 和 macOS `macosSigningSelection`/来源必须来自 `$desktop-prepare-release` 已锁定的同一发布信封，构建只读消费，不得重复询问、翻转或根据本机条件推断。硬要求与封存选择冲突时停止并返回发布准备。
5. 调用 `$desktop-manage-version check --phase build`，要求 Cargo 版本与受保护状态一致，且不得计算、提升版本或重置正式发布周期。只读校验 `release-notes.json` 的当前版本，并校验 `src-tauri/tauri.release.conf.json` 只把它映射到候选资源根。

## 构建

1. 不先逐文件写入或清理项目根 `release/`。在项目根同级、同一文件系统创建权限受限且目标外的唯一 staging；拒绝项目根、越界、符号链接/重解析点和非普通文件。所有构建、证据、manifest 与复核都先在 staging 完成，失败不得部分污染既有候选。
2. 不例行预检环境。先运行真实命令；只有失败诊断明确指向受管工具缺失/不兼容时，才调用 `$desktop-check-development-environment` 并重试原命令一次。
3. 先确认并运行非空 `cargo test --workspace --all-targets --all-features --locked`，再运行前端 manifest/lock 声明的完整非空单元测试套件。失败、零测试或测试产生工作树变化都停止。
4. 固定 GUI 基线要求 system-locale、window-state、dialog、设置页应用/版本与本地 release-notes Rust/React 链路，以及 profile 已选能力结构完整；固定支持页面、导航和专属资源必须精确匹配仅含设置页的允许集合。依赖、配置、命令、前端、资源与 manifest 不得包含应用内版本服务或产品统计网络管线。
5. `performanceSelection = enabled` 或硬要求时，先用相同 release 配置运行项目本地 release-profile Tauri `--no-bundle`，精确定位 runtime，计算 `performanceProbeSha256` 并调用 `$desktop-test-gui-release-performance`。按 `gui-release-v2` 验证恰好 5 次冷启动、至少 20 次交互、Long Task、整棵进程树 CPU/RSS 和回收；只有 `passed` 或用户看过保留的 `failed` 指标后明确 `waived` 才继续。原生候选还必须形成 `performanceRuntimeBinding`，证明探针、打包前 runtime 与最终包内 runtime 的签名前/签名后关系；DMG/NSIS 容器摘要不能冒充探针摘要。xwin 精确标记 `Unverified`。关闭且无硬要求时记录 `performanceStatus: Not run`、非空 `performanceReason`/`performanceRemainingRisk`，且 `performanceEvidence`、`performanceProbe`、`performanceProbeSha256`、`performanceThresholdProfile`、`performanceWaiver` 与 `performanceRuntimeBinding` 全部缺席。
6. 原生 macOS 构建 DMG；原生 Windows x64 构建 NSIS；macOS 可按批准路线使用 xwin 构建 Windows x64 NSIS。每条正式命令都显式传 `--config src-tauri/tauri.release.conf.json`，不得生成其他 bundle 格式。
7. macOS 只消费封存的签名意图：`disabled/not-requested` 使用 `--no-sign`，不探测身份、证书、公证凭据或 profile，并记录 unsigned 原因/风险；`enabled` 只允许来源 `configured|requested|channel-required`，必须完成签名、公证、stapling 与 Gatekeeper 验证，任一步缺失或失败都阻断，绝不回退 unsigned。`system_notification = enabled` 的 macOS 候选必须选择并完成签名。Windows 签名仅按批准渠道执行。不得索取、显示、写入或上传密钥实值。
8. macOS 最终 DMG 在本次选择形成的所有最终字节变化后运行 `scripts/verify-dmg-layout.sh <final-dmg> <project-root>/release-notes.json`，只读验证唯一 app、Applications 链接、背景、布局与包内 release notes；启用签名时针对已签名、公证并 stapled 的最终字节，禁用时针对明确的 unsigned 最终字节。xwin runtime 保持 `Unverified`。
9. 所有布局、签名、公证、stapling 与包内资源变化完成后，才对最终安装包、release notes 和适用性能/审查证据计算摘要。随后在 staging 写 manifest，再按每份 manifest 重新枚举并复算精确文件集、大小、摘要、选择和证据；全部成功后才用不跟随链接的目录级原子替换提交到根 `release/`，提交失败恢复旧目录。替换后只读重新枚举和复算，不再写入。不得有历史、额外、空或未声明文件。

## Manifest 与后续

Manifest 至少记录项目、版本、source commit/clean 状态、平台、架构、bundle format、native/xwin 模式、安装包路径/大小/SHA-256、release-notes 版本/路径/摘要、Rust/前端测试数、签名/公证、`runtimeVerification`、E2E 选择和 `milestoneAcceptance: pending`。同时保留 `reviewSelection`/`reviewStatus` 及启用时的结构化 `reviewEvidence`/`reviewedSourceCommit`，或关闭时非空原因/风险且证据字段缺席；保留性能选择/状态及 `gui-release-v2` 适用证据或完整 `Not run` 缺席契约；macOS 记录 `macosSigningSelection`/`macosSigningSource`、原因、结构化 `signingEvidence` 与适用 `notarizationEvidence`，来源按 `channel-required > requested > configured > not-requested` 唯一化。不得声明或生成应用内版本更新 archive、签名或 feed 字段。

最终字节形成后，E2E 启用或渠道硬要求时调用 `$desktop-verify-delivery`；否则记录 `E2E: Not run` 与剩余风险。构建不创建 Product Spec、ADR、Changelog、Product Status、Work Plan 或 Verification；候选事实只进入忽略的 `release/` 原子集合及最终回复。构建不授权 tag、push、上传或渠道发布。
