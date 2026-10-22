# GitHub Actions 多平台渠道发布记录

日期：2026-10-22（Asia/Shanghai）；公开时间：2026-10-22 13:53:51。

change_id：`LMDESK-GITHUB-PUBLICATION-20261022-001`；分类：maintenance；required_version：`0.9.0`，不提升产品版本。

## 授权与渠道结果

负责人明确要求发布并推送，通过 GitHub Actions 构建到 GitHub Release；本次明确启用源码审查、关闭最终产物 E2E。

- 发布页：[LokiMetris v0.9.0](https://github.com/ManonLoki/LokiMetis/releases/tag/v0.9.0-20261022)；Release ID：`420353378`。
- 标签：`v0.9.0-20261022`；最终发布提交：`66bbf827b5eb5894d1fa3273977cd35978ed2d98`。
- 审查源码提交：`7874ac3153e54c4a7d156ee997c5dae31c93dd8c`；五项发布源码审查均通过。
- 发布上下文 SHA-256：`c9ce87699909f205550d54bc6a0d53ea1a6e122d315dd767444bae3e420aaa55`。
- [Actions run 39726059745](https://github.com/ManonLoki/LokiMetis/actions/runs/39726059745)，attempt `1`；四个原生构建 job 与发布 job 均为 success。
- 已复读 `draft=false`、`prerelease=false`，并确认 latest Release ID 等于本次 ID。
- 远端 `main`、`release` 和版本 tag 已逐项复读，均指向上述最终发布提交。

## 完整产物集合与摘要

本次公开六份安装包，每份附带一个 `.sha256` 和一个 `.manifest.json`，合计18个附件。CI 发布 job 先校验精确集合及当前 run 身份，再上传到草稿、下载全部附件复算 SHA-256，最后复读远端身份并公开。

| 安装包 | 字节数 | SHA-256 |
| --- | ---: | --- |
| `loki-metris-v0.9.0-linux-aarch64.deb` | 41422044 | `a189d37d925dad04cd74c3b35752bdfc08bfcbc65dcc3831469669c54bf7a920` |
| `loki-metris-v0.9.0-linux-aarch64.tar.gz` | 53053221 | `05ed193eede7facc3137c1e262a8204c02b884f61789dae906c24ce274f20dac` |
| `loki-metris-v0.9.0-linux-x86_64.deb` | 41907588 | `9b376fa4bf91c51a4bd3717dbaf3b0346fb7f427e2fe6d971d0e6d27dc1fa9a4` |
| `loki-metris-v0.9.0-linux-x86_64.tar.gz` | 53789054 | `5d981035c6b3ec5d61dc8263085333639e323f4f98ac3dd9945f34695ef722d4` |
| `loki-metris-v0.9.0-macos-aarch64.dmg` | 40533896 | `7ebd9d6fe5c808a3cca714d931475a4115390f45b28d7423dd8a3160347ccfcb` |
| `loki-metris-v0.9.0-windows-x86_64.exe` | 33419906 | `c212fd2820938b8a9acd6dc5683028719420a3dc0ef53adeca63df3c8d81a294` |

公开后另行取回六份 manifest 和六份校验文件，复算这些小文件并核对全部18项 GitHub 服务端 SHA-256、非空大小与 uploaded 状态；各安装包服务端摘要与 manifest 一致。完整安装包的下载字节复算由上述成功的 CI 发布 job 执行，本地渠道证据目录不作为完整候选集合。

## 测试、资源与签名证据

- 每份候选均保存 workspace/all-targets/all-features 非空测试通过证据；macOS 为1798项、Windows为1750项，Linux两个架构各为1767项。不同宿主的条件测试集合不同，不合并为跨平台运行验收。
- 六份 manifest 的源码、版本、tag、上下文及当前 Actions run 一致；资源及最终包校验均通过。
- macOS Apple Silicon DMG 完成 Developer ID 签名、App/DMG分别公证及票据附加校验。
- App 公证任务 `4479286b-c293-4950-8269-5c4877fdbdd9`、DMG公证任务 `63d8a491-df90-4d4c-bb14-06ef9b5ae13d` 均为 Apple Accepted；Gatekeeper为 accepted，策略为 assessments enabled，distributionPolicy为 passed。
- Windows x64、Linux AMD64/ARM64 本次明确 unsigned。

## 验证边界与证据位置

E2E为 `Not run`，原因是本次用户明确关闭；安装、交互与目标系统实际运行仍为 `Unverified`。六份 manifest 保留 `milestoneAcceptance:pending`，本次渠道公开不改写为完整产品验收 accepted。

渠道回执、取回的 manifest 和校验文件保存在忽略目录 `release/github-publication-v0.9.0-39726059745/`。真实构建、解包、签名公证及完整附件下载复算的原始日志保留在本次 Actions run及其产物中。

本记录与发布后的版本周期复位只保存为后续维护提交；本地和远端 `main`、`release` 及产品 tag 继续绑定上述发布提交。
