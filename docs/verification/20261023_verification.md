# GitHub Actions v0.9.1 渠道发布记录

日期：2026-10-23（Asia/Shanghai）；公开时间：2026-10-23 17:47:28。

change_id：`LMDESK-GITHUB-PUBLICATION-20261023-001`；分类：maintenance；required_version：`0.9.1`。用户明确要求重新发布、推送并通过GitHub Actions构建到GitHub Release，随后明确选择0.9.1及关闭本次E2E；一次性版本分配范围见[ADR-20261023-001](../adr/20261023_ADR.md#adr-20261023-001本次明确升级091并重新构建发布)。

## 渠道身份

- [LokiMetris v0.9.1](https://github.com/ManonLoki/LokiMetis/releases/tag/v0.9.1-20261023)，Release ID：`421675848`；已复读公开、非预发布且latest精确指向该ID。
- tag：`v0.9.1-20261023`；发布提交：`f8bfb3068b838d515ebe5c49654b82a31b77a6fc`；审查源码：`340a7881de418fdb9d05ea30b83caa389a840234`，五项发布审查通过。
- 发布上下文SHA-256：`2507ca2e96d2ef76f14d87d7999b87ea99e8c951ad5a734675dd23a16175851c`。
- [Actions run 39911519002](https://github.com/ManonLoki/LokiMetis/actions/runs/39911519002)最终attempt `2`成功；各平台实际attempt见下表。
- 渠道公开时远端main、release及版本tag均精确指向上述发布提交；后续渠道记录维护提交可以推进main，release和tag继续固定发布身份。
- 保留原0.9.0、0.8.0以及既有归档；本轮不移动旧tag、不替换旧Release或附件。

## 安装包与摘要

六个安装包各附带.sha256和.manifest.json，合计18个附件。CI发布job在公开前下载全部18附件并复算字节摘要；公开后本机独立下载12份小附件，核对本地SHA-256、全部18项服务端摘要、大小和uploaded状态。此本地回执目录不作为完整候选集合。

| 安装包 | 字节数 | SHA-256 | 测试通过数 | attempt |
| --- | ---: | --- | ---: | ---: |
| `loki-metris-v0.9.1-linux-aarch64.deb` | 41387676 | `29f73ecc673c6a082801574eedfdf2ee5fe0bed868afdf5b0a937a430c1f3982` | 1767 | 1 |
| `loki-metris-v0.9.1-linux-aarch64.tar.gz` | 53043274 | `574813e15cc30924b528d800a57748b4dde292ddcf9a02b834405b6eaec0a9f9` | 1767 | 1 |
| `loki-metris-v0.9.1-linux-x86_64.deb` | 41883516 | `486a0bfecc3a44f50d547cb81bdac571492c6d385dec94d7da07a40659c1021b` | 1767 | 2 |
| `loki-metris-v0.9.1-linux-x86_64.tar.gz` | 53744446 | `272e83ac11a2a03fd16aa8e61d8d37269533b0f3a6f987c11fd38adbb1039000` | 1767 | 2 |
| `loki-metris-v0.9.1-macos-aarch64.dmg` | 40539403 | `be83b35e1682dc81ae45f85363f6d7b2804a8fc750e75452e2b49ec0e5754c4c` | 1798 | 1 |
| `loki-metris-v0.9.1-windows-x86_64.exe` | 33373343 | `6f33a8a4e414655a73a292949234d254c44d50c22ef5d45267a4ff534f3f5bad` | 1750 | 1 |

## 测试、签名与验收边界

- 各平台workspace/all-targets/all-features全量非空测试通过，资源、包内双语发布日志及最终包校验均通过；计数分别保留，不将不同宿主测试当作跨平台实际运行。
- Linux AMD64首次执行在`pet::first_populated_tests::first_image_retry_waits_for_changed_service_catalog`的pet_settings_revision断言观察到2，预期1；GUI当次1206通过、1失败、6忽略。相同冻结源码完整重跑通过，首次失败保留。源码显示首次几何位置写与首图选择共用revision，相邻测试已单独排空位置回执；该用例的时序脆弱性记录于[技术债](../TECH_DEBT.md)，不把重试通过当作已修复。
- macOS完成Developer ID签名、App/DMG双公证及stapling。App公证任务`57d31042-db34-4a6e-bb62-8610a295d533`、DMG任务`f60d41a0-a363-4311-911a-cdea4ef04565`均Accepted；Gatekeeper accepted，distributionPolicy passed。Windows/Linux保持既定unsigned状态。
- E2E：`Not run`，本轮用户明确关闭。安装、交互和目标系统实际运行仍为`Unverified`，manifest的milestoneAcceptance保持pending；渠道公开不等于完整产品验收。
- 本地发布/版本脚本90项回归、项目validate三项门禁及版本一致性检查通过；产品业务源码和依赖没有变化。

证据：忽略目录`release/github-publication-v0.9.1-39911519002/`中的receipt、当前Release对象、workflow-run、六份manifest及六份校验文件；首次失败和attempt1/2事实分别保留。完整包内容、公证与全附件字节复算日志由上述Actions run保存。
