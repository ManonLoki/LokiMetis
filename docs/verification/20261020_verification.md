# GitHub macOS 签名公证包发布记录

日期：2026-10-20（Asia/Shanghai）。

change_id：`LMDESK-GITHUB-PUBLICATION-20261020-001`；分类：maintenance；required_version：`0.5.0`，不提升产品版本。

## 授权与渠道事实

负责人明确要求发布并推送所有变更，构建 macOS 签名公证包并发布到 GitHub；本次明确关闭 E2E，另明确批准恢复本机发布登记及全局安装 `cargo-packager 0.11.8`。全局工具实际路径与版本均已复读核验。

- 发布页：[LokiMetris v0.5.0 · macOS Apple Silicon](https://github.com/ManonLoki/LokiMetis/releases/tag/v0.5.0-20261020)。
- GitHub Release ID：`417491919`；标签：`v0.5.0-20261020`。
- 发布源码：`058c5433451f246213cc6387511500604660f87d`；发布时远端 `main`、`release` 与标签逐项复读一致。
- 发布上下文 SHA-256：`38d829d165e690c590c56093fa9652edbd9d8b1c1070949b3f1281728109fbcf`。
- 先创建草稿并上传全部附件，核对服务器返回的大小、`uploaded` 状态与 SHA-256 后公开；已复读 `draft=false`、`prerelease=false`，并确认 Latest ID 与本次 Release ID 一致。
- 公开构建摘要采用字段白名单，已检查不含本机绝对路径、密钥路径或认证环境。

## 产物与可观察验证

本次提供 macOS Apple Silicon（`aarch64-apple-darwin`）DMG 和 App ZIP，最低 macOS 12。

| 附件 | 字节数 | SHA-256 |
| --- | ---: | --- |
| `loki-metris-v0.5.0-macos-aarch64.dmg` | 38061106 | `c7639a97e044a78382ae0eff3b4ee040f0c09a123de2b3922f77d6c36851ff9c` |
| `loki-metris-v0.5.0-macos-aarch64.app.zip` | 36514543 | `fbf0a29cfa72801cb9120edd28c59251e258c5cee07ac1eff9bd58ad0192dc68` |

另有两个对应 `.sha256` 文件及[公开构建与公证摘要](https://github.com/ManonLoki/LokiMetis/releases/download/v0.5.0-20261020/loki-metris-v0.5.0-macos-aarch64.manifest.json)，合计五个附件。五项服务器摘要均与实际上传文件逐项一致。

- 本地全量 workspace/all-targets/all-features 测试执行记录为 1609 项通过，构建源码为 clean 发布提交。
- App 与 DMG 的签名身份均为 `Developer ID Application: Manon Loki (GKVE4TEFQ6)`；安全时间戳、严格签名验证通过，App 启用 hardened runtime。
- App 公证任务 `e8fbf77f-3c64-4464-ba8b-bb8cdeb20d88` 与 DMG 公证任务 `840d6d9d-dda8-4b80-b6ea-656efb4434e6` 均由 Apple 返回 `Accepted`。
- App 公证日志中的 arm64 CDHash 与本次可执行文件一致；DMG 公证日志的上传 SHA-256 与附票前本次 DMG 的真实摘要一致。
- App、最终 DMG、DMG 内 App 及 ZIP 解包后的 App 均完成票据附加或有效性校验。最终 DMG 只读挂载核对 Finder 布局、应用身份、资源及签名后二进制摘要；归档解包再次核对资源与可执行文件。
- `syspolicy_check distribution` 返回成功，所有分发检查通过。

## 验证边界与证据位置

附件为本次本地构建的签名公证派生包，`managedReleaseCandidate=false`，未标记 milestone accepted。受管 candidate 的 macOS Developer ID 路径仍明确阻断；本次沿用独立本地派生流程完成请求。

E2E 按本次明确选择关闭；安装、启动、系统通知授权及横幅行为保持 `Unverified`。本机 Gatekeeper 全局评估关闭，`spctl` 的 accepted 回执含 `security disabled`；开启强制评估时的启动行为未验证。公开发布说明与摘要均保留这些限制。本次未执行其他平台构建或运行验收。

本次本地构建、签名、公证、最终校验及渠道上传的原始证据保留在忽略目录 `target/gpui-packaging/local-notarized-ixKQ8I/`，渠道回执位于其 `github-publication/` 子目录。本文只追加真实渠道分发事实；发布后的版本周期复位与本记录在后续维护提交中保存，产品标签仍绑定上述源码提交。
