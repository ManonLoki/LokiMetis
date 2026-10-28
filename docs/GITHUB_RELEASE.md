# GitHub Actions 发布

本项目后续正式发布固定使用 GitHub Actions 构建，再上传到 [ManonLoki/LokiMetis Releases](https://github.com/ManonLoki/LokiMetis/releases)。本选择由用户于 2026-10-22 明确确认，change_id：`LMDESK-GITHUB-ACTIONS-RELEASE-001`，required_version：`0.7.5`，工程维护不另升产品版本。流程实现是[release.yml](../.github/workflows/release.yml)，Git 后续动作取 [Agent Policy](AGENT_POLICY.md)。本页记录方法与目标，不作为任何候选已通过的证据。

| 目标 | 原生 runner | 产物 | 签名 |
| --- | --- | --- | --- |
| Windows x64 | windows-2025 | NSIS exe | 当前未要求 Authenticode，明确 unsigned |
| macOS Apple Silicon | macos-15 arm64 | DMG | Developer ID、App/DMG 分别公证及 stapling、Gatekeeper |
| Linux AMD64 | ubuntu-26.04 | deb、tar.gz | unsigned |
| Linux ARM64 | ubuntu-26.04-arm | deb、tar.gz | unsigned |

macOS Intel、Windows ARM、移动端及额外格式不属于此固定桌面矩阵。每个目标实际执行 workspace/all-targets/all-features 的非空测试、日志注入、资源与最终包校验；各平台测试证据分别保存。macOS 的真实 Metal 测试失败时停止，不能跳过后沿用本机历史结果。

## 发布步骤

1. 按[发布规则](RELEASE.md)完成源码审查、双语日志和发布上下文提交，普通合并到本地默认主分支，并创建精确版本 tag。
2. 按本次冻结的 `push_release_branch` 推送默认主分支、小写 `release` 和 tag，逐项复读同一提交；不要移动已有版本 tag。
3. 每次候选重新取得 E2E 选择，再显式启动工作流。以当前版本的精确 tag 替换下列参数：

   ```sh
   gh workflow run release.yml --repo ManonLoki/LokiMetis --ref main -f tag=vMAJOR.MINOR.PATCH-YYYYMMDD -f e2e=disabled
   ```

   `disabled` 仅在用户已关闭本次 E2E 时使用。本次 2026-10-22 已明确关闭；不改变未来候选选择。`enabled` 会形成待验收候选，流水线不会把未通过的 E2E 自动改成通过或公开发布。
4. 四组 job 各自从精确 tag checkout 完整历史，复读远端三引用并恢复到上下文默认分支；candidate 入口重新核对 clean、tag、上下文、版本与源码审查摘要。CI 不伪造本机 common-dir 生命周期记录。
5. Linux 每次格式生成后立即保存完整集合到 `RUNNER_TEMP`，防止下一次 candidate 刷新整个 `release/`。每组上传独立 Actions artifact。
6. 全部 job 成功后，发布 job 要求六份产物、18个精确文件；校验同一版本、源码、上下文、当前 Actions run、非空测试、最终资源、macOS 两次公证及 SHA-256。先上传到草稿，再下载全部附件复算，复读远端身份后才公开为最新 GitHub Release。

候选 manifest 保留 `milestoneAcceptance:pending`、`runtimeVerification:Unverified`；关闭 E2E 的渠道发布不代表安装、交互或产品完整验收已通过。任何失败均保留真实失败结论及可用的 Actions 日志，不发布缺平台集合。

## 凭据和工具

用户已授权将本机既有 Developer ID 及公证材料配置为本仓库 Actions secrets。只允许下列专用秘密字段，不提交秘密文件或值到源码：

- `MACOS_CERTIFICATE_BASE64`：只含已批准 Developer ID 身份、采用 macOS Security 兼容格式且经独立临时钥匙串导入及身份复读校验的 P12；不能仅凭 OpenSSL 解码成功判定可导入。
- `MACOS_CERTIFICATE_PASSWORD`：该导出包的随机密码。
- `APPLE_NOTARIZATION_KEY`：既有 App Store Connect P8。
- `APPLE_NOTARIZATION_KEY_ID`、`APPLE_NOTARIZATION_ISSUER`：对应公证标识。

公开签名身份取 `packaging/gpui.json`。runner 建立短期钥匙串和0600密钥文件，`always()` 步骤清理；签名/公证失败不得回退 unsigned。Windows 从预装全局7-Zip解包，不安装应用来代替校验。Rust 1.98.1、Node 24.21.0、pnpm 12.4.1 和 cargo-packager 0.11.8 在临时 runner 安装并复读实际版本/路径，不改变开发主机工具。

当前 Cargo.lock 为 ignored，不保证不同 runner 解析到完全相同依赖；源码身份、工具版本、各宿主测试和实际产物独立绑定。固定官方 Actions 的提交摘要；runner 镜像和原生库随 GitHub 镜像更新，版本兼容以本次真实输出为准。

## 本次旧 Release 清理

2026-10-22另有一次保留`0.9.0`并替换`v0.9.0-20261022`的明确请求，范围见[ADR-20261022-007](adr/20261028_ADR.md#adr-20261022-007仅本次保留090并归档替换旧发布)。该次替换已完成，对应的一次性脚本已随用随删，不再保留；旧Release在备份校验后曾按精确ID改绑归档tag、转为草稿，新Release继续使用以上原工作流。普通发布规则和发布器拒绝覆盖公开Release的保护保持有效。

用户于 2026-10-22 另行授权一次性删除开始任务时已有的三个 GitHub Release：`v0.7.4-20261021` 草稿、`v0.5.0-20261020`、`v0.3.13-20261019`。清理只删除 Release 对象及附件，保留 Git tag 和提交。先在忽略目录保存对象清单和附件，再精确删除并复读；实际结果另取任务输出或渠道证据。

该清理不是未来发布流程，不加入工作流自动删除步骤；将来再次清理必须有新的明确请求。
