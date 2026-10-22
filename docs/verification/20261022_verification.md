# GitHub Actions 多平台渠道发布记录

## 同版本替换（当前公开发布）

日期：2026-10-22（Asia/Shanghai）；公开时间：2026-10-22 19:57:08。

change_id：`LMDESK-GITHUB-REPUBLICATION-20261022-002`；分类：maintenance；required_version：`0.9.0`，产品版本保持不变。用户明确要求发布、推送、通过Actions全量构建并替换之前的0.9.0，本轮关闭E2E；受限替换范围见[ADR-20261022-007](../adr/20261022_ADR.md#adr-20261022-007仅本次保留090并归档替换旧发布)。

### 当前渠道身份

- [LokiMetris v0.9.0](https://github.com/ManonLoki/LokiMetis/releases/tag/v0.9.0-20261022)，新Release ID：`420664215`；已复读`draft=false`、`prerelease=false`及latest精确指向本次ID。
- tag：`v0.9.0-20261022`；发布提交：`0e326ceb745d7fa50119d61ab5a2ee59d868c9f6`；审查源码：`e902a4980c1d232b725ed75b327d59ee0a21b48d`，五项源码审查通过。
- 发布上下文SHA-256：`0185ec6230cf9d663831689d75c2bac42c693be69dc50c8c433f23d90f69b29b`。
- [Actions run 39765773399](https://github.com/ManonLoki/LokiMetis/actions/runs/39765773399)，最终attempt `2`：四个原生目标与发布job均success；Linux/macOS产物来自attempt `1`，Windows来自attempt `2`，各manifest保留实际attempt。
- 渠道公开后复读远端`main`、`release`和版本tag，均为上述发布提交。本记录及版本周期绑定随后保存为维护提交；三引用一致性指渠道公开时快照，产品tag和release分支继续固定于发布提交。

旧Release ID `420353378`已按精确身份改绑到`archive/v0.9.0-20261022/66bbf827b5eb5894d1fa3273977cd35978ed2d98`并转为草稿，18附件保留。旧对象JSON、安装包与全部附件SHA-256已在`target/republication-v0.9.0-66bbf827/`备份复算；旧源码和原生命周期状态分别由归档tag及Git common-dir的替换journal保留。普通发布仍禁止同版移动已发布tag。

### 本次产物与下载复核

六个安装包各附带`.sha256`和`.manifest.json`，合计18附件：

| 安装包 | 字节数 | SHA-256 |
| --- | ---: | --- |
| `loki-metris-v0.9.0-windows-x86_64.exe` | 33376599 | `9efaa9258a40ddd1690855b186153e9b40d36e6fe5bea5796a1118d713244923` |
| `loki-metris-v0.9.0-macos-aarch64.dmg` | 40527042 | `34dfb097bd4f4e6cc3d5b6ebeab9035afefe6e7f3a7a8fadf4deac53ea37784a` |
| `loki-metris-v0.9.0-linux-x86_64.deb` | 41885058 | `45e66a9a2d72c01996c1cccd514c4322c8db8e6ee873a318c5958141b1656c87` |
| `loki-metris-v0.9.0-linux-x86_64.tar.gz` | 53747663 | `4d559e42b80b644141f95f752885fc2c9d591a95278e4b4610a3959848dc4cee` |
| `loki-metris-v0.9.0-linux-aarch64.deb` | 41384344 | `d869d805e1306b0c8c1e9cfd95cad8721041ce69041f47e30bd95c75a965be4d` |
| `loki-metris-v0.9.0-linux-aarch64.tar.gz` | 53044525 | `9feacdfdc5c1b42169fceaa8c3bdd48a4dabeb2b6f38bcb29465d495da0bfc1f` |

CI发布job在公开前下载全部18附件并复算字节摘要。公开后本机另取回6份manifest和6份校验文件，复算这些小文件并核对全部18项服务端摘要、大小及uploaded状态；安装包服务端摘要与manifest一致。本机渠道证据目录仅为回执与小附件，不作为完整候选集合。

### 本轮测试、签名与验收边界

- workspace/all-targets/all-features全量非空测试通过：Windows `1750`项、macOS `1798`项、Linux两架构每个格式各`1767`项；宿主条件集合不同，计数分别保留。
- Windows首次执行在`hook_directory_controls_preserve_tool_drafts_and_disable_unsaved_write`等待Hook界面回执时超时，GUI crate当次为1187通过、1失败。随后对同一源码完整重跑Windows并通过1750项；首次失败和实际等待预算问题保留在[技术债](../TECH_DEBT.md)及原始Actions日志。
- 六份manifest均绑定本次版本、源码、tag、上下文及当前run；资源和最终包校验通过。Windows/Linux为本次明确的unsigned状态。
- macOS完成Developer ID签名、App/DMG双公证及stapling；App任务`47e20e39-adc8-4bdc-8225-636eeb24edbf`、DMG任务`b45d0b07-0745-41a1-a804-4216327e2ce1`均为Accepted，Gatekeeper为accepted，distributionPolicy为passed。
- E2E：`Not run`，本轮用户明确关闭；安装、交互和目标系统实际运行仍为`Unverified`，manifest的`milestoneAcceptance`保持`pending`。渠道公开不等于完整产品验收。

本次回执与小附件位于`release/github-republication-v0.9.0-39765773399/`，首次失败记录位于`release/github-republication-v0.9.0-39765773399-attempt1-failure/`；完整构建、资源、公证和附件复算日志保留在上述Actions run。

## 首次发布历史（已归档）

以下保留首次发布时的记录。其原版本tag和公开链接现由上方同版本替换接管，原三引用核对为当时快照；旧源码、Release及摘要不能用于代表本轮候选。

日期：2026-10-22（Asia/Shanghai）；公开时间：2026-10-22 13:53:51。

change_id：`LMDESK-GITHUB-PUBLICATION-20261022-001`；分类：maintenance；required_version：`0.9.0`，不提升产品版本。

### 授权与渠道结果

负责人明确要求发布并推送，通过 GitHub Actions 构建到 GitHub Release；本次明确启用源码审查、关闭最终产物 E2E。

- 发布页：[LokiMetris v0.9.0](https://github.com/ManonLoki/LokiMetis/releases/tag/v0.9.0-20261022)；Release ID：`420353378`。
- 标签：`v0.9.0-20261022`；最终发布提交：`66bbf827b5eb5894d1fa3273977cd35978ed2d98`。
- 审查源码提交：`7874ac3153e54c4a7d156ee997c5dae31c93dd8c`；五项发布源码审查均通过。
- 发布上下文 SHA-256：`c9ce87699909f205550d54bc6a0d53ea1a6e122d315dd767444bae3e420aaa55`。
- [Actions run 39726059745](https://github.com/ManonLoki/LokiMetis/actions/runs/39726059745)，attempt `1`；四个原生构建 job 与发布 job 均为 success。
- 已复读 `draft=false`、`prerelease=false`，并确认 latest Release ID 等于本次 ID。
- 远端 `main`、`release` 和版本 tag 已逐项复读，均指向上述最终发布提交。

### 完整产物集合与摘要

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

### 测试、资源与签名证据

- 每份候选均保存 workspace/all-targets/all-features 非空测试通过证据；macOS 为1798项、Windows为1750项，Linux两个架构各为1767项。不同宿主的条件测试集合不同，不合并为跨平台运行验收。
- 六份 manifest 的源码、版本、tag、上下文及当前 Actions run 一致；资源及最终包校验均通过。
- macOS Apple Silicon DMG 完成 Developer ID 签名、App/DMG分别公证及票据附加校验。
- App 公证任务 `4479286b-c293-4950-8269-5c4877fdbdd9`、DMG公证任务 `63d8a491-df90-4d4c-bb14-06ef9b5ae13d` 均为 Apple Accepted；Gatekeeper为 accepted，策略为 assessments enabled，distributionPolicy为 passed。
- Windows x64、Linux AMD64/ARM64 本次明确 unsigned。

### 验证边界与证据位置

E2E为 `Not run`，原因是本次用户明确关闭；安装、交互与目标系统实际运行仍为 `Unverified`。六份 manifest 保留 `milestoneAcceptance:pending`，本次渠道公开不改写为完整产品验收 accepted。

渠道回执、取回的 manifest 和校验文件保存在忽略目录 `release/github-publication-v0.9.0-39726059745/`。真实构建、解包、签名公证及完整附件下载复算的原始日志保留在本次 Actions run及其产物中。

本记录与发布后的版本周期复位只保存为后续维护提交；本地和远端 `main`、`release` 及产品 tag 继续绑定上述发布提交。
