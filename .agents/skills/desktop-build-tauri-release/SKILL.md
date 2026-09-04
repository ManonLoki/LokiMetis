---
name: desktop-build-tauri-release
description: 从 clean HEAD 构建 GUI-only Tauri 2 发布候选，支持 macOS 原生 DMG、Windows 原生 x64 NSIS 与 macOS xwin NSIS，并按当次选择执行性能和 E2E 门禁。
---

# 构建 Tauri 发布候选

## 准入

1. 只接受用户明确提出的发布候选请求。普通本机安装检查交给 `$desktop-build-tauri-local-install`。
2. 读取产品事实、GUI profile、Agent Policy、`docs/RELEASE.md`、根 Cargo/前端锁文件、Tauri 配置与 [平台路线](references/tauri-macos-windows.md)。根 metadata 必须精确为 `interfaces = ["gui"]`，目标平台包含本次候选平台。
3. 项目必须是独立 Git 根，有 40 位 `HEAD` 且 `git status --porcelain=v1 --untracked-files=all` 为空。锁定 `buildSourceCommit`；构建期间任何 HEAD 或工作树漂移都停止。
4. 在测试或编译前解析本次 `e2eSelection` 与 `performanceSelection`。产品/渠道硬要求优先；否则用户未明确时各询问一次。选择仅属于本次构建，不写回持久策略。
5. 调用 `$desktop-manage-version check --phase build`，要求 Cargo 版本与受保护状态一致。只读校验 `release-notes.json` 的当前版本，并校验 `src-tauri/tauri.release.conf.json` 只把它映射到候选资源根。

## 构建

1. macOS/Linux 使用 `scripts/prepare-release-directory.sh <project-root>`，Windows 使用 `scripts/prepare-release-directory.ps1 -ProjectRoot <project-root>` 安全刷新根 `release/`；helper 返回的 source commit 必须匹配锁定提交。
2. 不例行预检环境。先运行真实命令；只有失败诊断明确指向受管工具缺失/不兼容时，才调用 `$desktop-check-development-environment` 并重试原命令一次。
3. 先确认并运行非空 `cargo test --workspace --all-targets --all-features --locked`，再运行前端 manifest/lock 声明的完整非空单元测试套件。失败、零测试或测试产生工作树变化都停止。
4. 固定 GUI 基线要求 system-locale、window-state、dialog、设置页应用/版本与本地 release-notes Rust/React 链路，以及 profile 已选能力结构完整；`about_page`、About 路由和关于页资源必须缺席。依赖、配置、命令、前端、资源与 manifest 不得包含应用内版本服务或产品统计网络管线。
5. `performanceSelection = enabled` 或硬要求时，先用相同 release 配置运行项目本地 `pnpm tauri build --no-bundle`，精确定位 Release Tauri runtime，计算 SHA-256 并调用 `$desktop-test-gui-release-performance`。只有 `passed` 或用户基于保留失败证据明确 `waived` 才继续；xwin 标记 `Unverified`。关闭且无硬要求时记录 `Not run`、原因与剩余风险，且无探针字段。
6. 原生 macOS 构建 DMG；原生 Windows x64 构建 NSIS；macOS 可按批准路线使用 xwin 构建 Windows x64 NSIS。每条正式命令都显式传 `--config src-tauri/tauri.release.conf.json`，不得生成其他 bundle 格式。
7. 渠道允许时可生成明确的 unsigned 安装候选。macOS Developer ID 直接分发若启用签名，必须完成签名、公证、stapling 与验证；Windows 签名按批准渠道执行。不得索取、显示、写入或上传密钥实值。
8. macOS 最终 DMG 在所有字节变化后运行 `scripts/verify-dmg-layout.sh <final-dmg> <project-root>/release-notes.json`，只读验证唯一 app、Applications 链接、背景、布局与包内 release notes。xwin runtime 保持 `Unverified`。
9. 对最终安装包、release notes 与适用性能证据计算摘要，在同根暂存目录生成 manifest 后原子替换 `release/`。文件集必须与 manifest 精确一致，不得有历史或额外文件。

## Manifest 与后续

Manifest 至少记录项目、版本、source commit/clean 状态、平台、架构、bundle format、native/xwin 模式、安装包路径/大小/SHA-256、release-notes 版本/路径/摘要、Rust/前端测试数、签名/公证、`runtimeVerification`、性能选择/状态及适用证据、E2E 选择和 `milestoneAcceptance: pending`。不得声明或生成应用内版本更新 archive、签名或 feed 字段。

最终字节形成后，E2E 启用或渠道硬要求时调用 `$desktop-verify-delivery`；否则记录 `E2E: Not run` 与剩余风险。构建不创建 Product Spec、ADR、Changelog、Product Status、Work Plan 或 Verification，不授权 tag、push、上传或渠道发布。
