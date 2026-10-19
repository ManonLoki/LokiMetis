# GitHub 渠道发布与旧版清理记录

日期：2026-10-19（Asia/Shanghai）。

change_id：`LMDESK-GITHUB-PUBLICATION-20261019-001`；分类：maintenance；required_version：`0.3.13`，不提升产品版本。

## 本次授权与发布事实

用户明确要求上传本次已签名公证的 macOS 本地包，并删除仓库中的全部旧版发布。操作目标为当前 `origin` 对应的公开仓库 `ManonLoki/LokiMetis`。

- 发布页：[LokiMetris v0.3.13 · macOS Apple Silicon](https://github.com/ManonLoki/LokiMetis/releases/tag/v0.3.13-20261019)。
- GitHub Release ID：`416985990`；标签：`v0.3.13-20261019`。
- 发布源码：`1518ca4c81aae9dcef294cb382a2020827a6dfcb`；远端 `main` 与上述标签仍指向此提交。
- 先创建草稿并上传全部附件，核对 GitHub 返回的大小与 SHA-256 后才公开并设为 Latest；随后执行旧版清理。
- 发布后复读确认：`draft=false`、`prerelease=false`，Latest ID 与上述 Release ID 一致。
- 公开的验证摘要使用字段白名单，未上传原始本地 manifest 中的绝对路径；未上传私钥、认证环境或公证凭据。

## 发布附件与真实验证

仅提供 macOS Apple Silicon（`aarch64-apple-darwin`）附件，最低 macOS 12。

| 附件 | 字节数 | SHA-256 |
| --- | ---: | --- |
| `loki-metris-v0.3.13-macos-aarch64.dmg` | 38293650 | `3f44971c4fdba5a52edcbc929399ed20a74a9abc4456b200887f4dd937124386` |
| `loki-metris-v0.3.13-macos-aarch64.app.zip` | 36741807 | `b5d2c3c4d817411c9a39bda07ffcd7402b0fda828b85f4aef5b757691a445d07` |

另有两个对应 `.sha256` 文件，以及[公开构建与公证摘要](https://github.com/ManonLoki/LokiMetis/releases/download/v0.3.13-20261019/loki-metris-v0.3.13-macos-aarch64.manifest.json)，合计五个附件。五项 GitHub 资产均为 `uploaded`，大小和服务器 SHA-256 均与实际上传文件逐项一致。

- 沿用本次本地全量构建的真实结果：workspace/all-targets/all-features 测试记录为 1610 项通过；未为渠道上传重复构建。
- App 与 DMG 的 Developer ID 身份为 `Developer ID Application: Manon Loki (GKVE4TEFQ6)`；应用 hardened runtime、安全时间戳、严格签名验证通过。
- App 公证任务 `6493c6ac-cbfb-4892-84f5-f5ebcbf01ade` 与 DMG 公证任务 `2246f09a-6684-4acd-b398-f2b827d952d6` 均由 Apple 返回 `Accepted`；App 任务日志的 CDHash 与本次可执行文件一致。
- App、最终 DMG、DMG 内 App 及 ZIP 解包后的 App 均完成票据附加或有效性校验；最终挂载卷的身份、Finder 布局、资源及签名后二进制摘要已核对。
- `syspolicy_check distribution` 返回成功且无 findings。

本次附件仍是独立生成的本地签名公证派生包，未转换为受管 candidate，也未标记 milestone accepted。安装、启动、完整 E2E、通知授权和横幅行为为 `Unverified`。本机构建时 Gatekeeper 全局评估关闭，`spctl` 的 accepted 回执包含 `security disabled`；开启强制评估时的启动行为未验证。发布说明和公开摘要已保留这些边界；本次未改系统评估策略。

## 旧版发布清理

新版公开且全部附件验证成功后，按用户明确要求逐项删除下列八个 Release。删除前保存完整远端元数据快照，操作按精确 Release ID 执行，每项成功后保存结果；没有按名称前缀批量删除 Git 资源。

| 旧版标签 | 已删除的 Release ID |
| --- | --- |
| `v0.3.12` | `400527843` |
| `v0.3.8` | `392687622` |
| `v0.3.7` | `387774545` |
| `v0.3.4` | `387542378` |
| `v0.3.2` | `387506716` |
| `v0.3.0` | `387427471` |
| `v0.2.46` | `386354340` |
| `v0.2.43` | `385972768` |

清理后分页读取全部 Releases，结果仅剩本次 Release ID `416985990`，含五个附件。旧发布的 Windows 等附件随所属 Release 一并删除，本版没有对应替代包。上述八个旧 Git 标签均已从远端复读确认仍存在；未删除 Git 标签、分支或源码历史，未移动本次发布标签。

原始远端快照、逐项清理回执和公开元数据暂存于本次忽略目录 `target/gpui-packaging/local-notarized-BV1oT6/github-publication-v5G8TR/`；本记录只追加真实渠道分发事实，不修改既往 Git 发布或候选验收结论。
