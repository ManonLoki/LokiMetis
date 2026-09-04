# LokiMetis 版本、构建与发布

本文件是 LokiMetis 的语义化版本、本地发布说明、GUI 候选、manifest 和发布顺序的唯一事实来源。构建或测试成功不自动授权提交、tag、push、上传、签名、公证、安装或渠道发布。

## 事实来源

- 当前版本：根 `Cargo.toml` 的 `[workspace.package].version`。
- 发布周期与缺陷去重：`.harness/version-state.json`；它不是第二份当前版本来源。
- 本地双语发布说明：根 `release-notes.json`，只在首次正式发布准备时创建。
- 目标平台与接口：根 `Cargo.toml` 的 `[workspace.metadata.agent-first-harness]`。
- Agent 冒烟/E2E 建议默认值：`docs/AGENT_POLICY.md`。
- 当前 GUI 身份和能力：`docs/GUI_APP_PROFILE.md`。

## SemVer

版本只由 `$desktop-manage-version` 管理，使用无预发布/构建元数据的 `MAJOR.MINOR.PATCH`，三段均在 `0..100`：

- Major 只在用户批准精确目标 `N.0.0` 后提升，目标必须高于当前 Major。
- 每个正式发布周期的第一个完成的新功能将 Minor 提升 1 并把 Patch 归零；同周期后续功能只记录所需版本。
- 每个新的稳定缺陷 ID 的完成修复将 Patch 提升 1；相同 ID 的补充、重试或跨周期再次处理不重复提升。
- 查询、诊断、复现、未完成尝试、行为保持重构、测试、文档、格式和内部清理不提升版本。
- 版本只在变化完成且本次相关测试通过后写入。构建、pending 候选、验收和失败发布只读核对。
- 只有正式渠道发布真实成功后才重置待发布变化并开启下一周期；历史缺陷 ID 永久保存用于去重。

任一段下一值超过 100 时停止并请项目负责人决定，不自动进位。版本变化与 Product Spec、ADR、Changelog、Status 或 Plan 的写入门禁彼此独立。

## 用户可见版本与发布说明

- 面向用户的版本统一为一个小写 `v` 前缀；窗口标题、侧栏、设置页、发布说明和产物名都先移除已有 `v`/`V` 再格式化。Cargo、状态文件和 manifest 的机器 `version` 不带前缀。
- `release-notes.json` 使用 `schemaVersion: 2`；`releases` 最新在前，每项包含 `releaseDate`、带单个 `v` 的 `version`、`featureOptimizations` 与 `bugFixes`。
- 每个条目必须恰有非空 `zh-CN` 与 `en-US` 翻译且无首尾空白；每版两类各最多 10 条、合计至少一条，只保留最近 5 个正式版本。
- 发布准备从上次真实发布的 40 位提交到当前 HEAD 语义整理重要变化；首个发布以仓库起点为边界，不直接倾倒提交标题。
- 同版本条目以当前真实内容替换后置顶。发布准备必须原子写入、校验预期版本，并分别渲染中英文复核。
- 发布说明是候选内本地资源，不建立远程版本服务。任何发布说明字节变化都会形成新源码提交和新候选。

## 明确不支持的能力与制品

- 不安装或配置应用 updater，不实现联网检查、强制更新、自动下载或安装。
- 不生成更新归档、更新签名、channel/target 元数据、公钥验签证据或对应 manifest 字段。
- 不收集或发送产品统计、行为分析、崩溃报告、远程观测数据或其他远程遥测。
- 本地发布说明、工程层升级、SemVer、本地 tracing 与 GUI 性能采样不构成上述应用能力。

## Windows 本地开发试包

普通“构建、打包、首次安装试一下”由 `$desktop-build-tauri-local-install` 生成原生 Windows x64 NSIS 开发制品：

- 可基于当前工作树，但必须报告 dirty 风险。
- 不读取或生成 `release-notes.json`，不传发布专用 Tauri 配置，不写 `release/`。
- 不提交、不安装、不运行，不解析候选 E2E 或性能选择。
- 不声称可分发、已验收或发布就绪。

只有用户明确要求“发布候选”或“准备并构建发布”才进入正式候选流程。

## 候选平台

- macOS 原生：Tauri DMG。
- Windows 原生 x64：Tauri NSIS。
- macOS 到 Windows x64：cargo-xwin 构建 NSIS；不得生成或声称 MSI，也不得把交叉构建成功当作 Windows 原生安装、运行或性能证据。
- Linux：当前没有已批准的统一正式渠道合同；源码目标有效，正式产物保持 `Unverified`。

macOS 直接分发采用全有或全无门禁：Developer ID Application、`notarytool`、`stapler` 与完整公证凭据齐全时，必须完成签名、公证、stapling 和最终验证；条件缺失且渠道允许时才可明确为 unsigned。不得把只签名未公证的中间状态表述为可分发候选。

最终 DMG 必须从最终字节只读挂载，验证 Finder 布局、本地 660×400 背景、唯一顶层 `.app` 与 `/Applications` 链接；不得以源码图片检查替代最终卷检查。

## 命名与 manifest

GUI 候选命名为：

```text
LokiMetis-v<MAJOR.MINOR.PATCH>-<platform>-<arch>.<ext>
```

每个安装包必须有相邻 `<artifact>.sha256` 与 manifest。`pending` manifest 至少包含：

- 项目标识、无前缀机器版本、批准的 40 位 `sourceCommit`、构建模式、平台、架构、target、host 与精确产物名；
- 最终 SHA-256、全量 Rust/前端单元测试结果、签名/公证状态与 `milestoneAcceptance: pending`；
- 当次 `e2eSelection`、`releaseNotesVersion`、`releaseNotesSha256`、`releaseNotesPath: release-notes.json`；
- `performanceSelection: enabled | disabled` 与 `performanceStatus: passed | waived | Not run | Unverified`。

启用性能或渠道强制且原生测量时，manifest 保存与同一提交、平台和探针字节绑定的证据及 runtime binding。合法 waiver 保留原失败指标、修复尝试、风险、理由与用户确认。性能关闭且无硬要求时固定 `Not run`，记录原因与风险，不生成性能证据。xwin 缺少原生 Windows 测量时为 `Unverified`，渠道要求性能时不能 accepted。

manifest 不得包含 updater、更新签名/endpoint、产品统计或远程遥测字段。

## `release/` 原子刷新

候选只写入根 `release/`；该目录由精确 `/release/` 规则忽略，存在不表示 ready。

1. 验证当前路径是独立 Git 根，拒绝 `release` 符号链接、重解析点和越界。
2. 将旧 `release/` 原子移到同文件系统隔离位置，建立并复核新的空目录，再安全清理隔离旧树；不得在活动目标内原地递归删除。
3. 安装包、摘要和 manifest 先在同根唯一暂存目录形成精确普通文件集。
4. 通过目录级原子重命名提交完整暂存目录，再复核最终文件集。

远端工作流必须检出并复核明确的 40 位提交，只上传 manifest 声明的文件。结果收集不得依赖“最新”或修改时间，也不得混入其他项目、版本或运行的产物。

## 正式候选顺序

每次候选的 E2E 与性能选择只对该次有效。当前请求未明确时，在任何发布提交、测试或编译前分别询问一次；`milestone_e2e` 只提供建议默认，性能没有持久默认。

1. `$desktop-prepare-release` 复核工作树、归属、缓存和疑似秘密，提交已完成且归属明确的变化；无关/歧义变化、秘密、hook/签名交互或提交失败立即停止。
2. 从新 HEAD 原子生成或整理 `release-notes.json`；变化时形成独立发布元数据提交。最终工作树必须干净且 `sourceCommit` 等于 HEAD。
3. `$desktop-build-tauri-release` 校验版本、发布说明、GUI 结构和禁用能力零残留，证明 Rust 与前端测试非空并运行全部单元测试。
4. 性能启用或渠道强制时，`$desktop-test-gui-release-performance` 从同一 clean HEAD 建立 release-profile no-bundle 探针，隔离并恢复 window-state，测量启动、交互、整进程树 CPU/RSS、内存增长和退出回收。
5. 性能 passed、合法 waived，或关闭时的 `Not run` 才进入适用 DMG/NSIS 打包；最终字节形成后计算摘要并提交 pending manifest。
6. `$desktop-verify-delivery` 根据持久冒烟偏好、当次 E2E 选择与渠道要求验收最终候选，并复核性能、发布说明、DMG 布局、签名和 manifest；适用时由 `$desktop-test-final-artifact-e2e` 操作真实安装产物。
7. 所有 required/enabled 检查和人工复核通过后才能转为 `accepted`。渠道发布真实成功后才由 `$desktop-manage-version` 完成发布周期。
8. 发布说明、签名、公证、stapling、重打包或渠道处理只要改变运行字节、启动器、依赖或行为，即形成新候选并回到步骤 3。

候选失败、超时、取消、已选未执行或无法观察都返回开发循环，不能改判为通过。发布流程不自动创建 Product Spec、ADR、Changelog、Status、Plan 或 Verification；只有各自事件另行触发时才留证。

## 许可

- 源码归档、安装包与其他分发物必须以用户可访问方式携带根目录两份企业专有商业许可证，且项目名与发布产品名一致。
- 分发前生成并核对当前版本第三方依赖许可证与 NOTICE。专有许可不替代第三方许可证依法授予的权利。
- 商业合同决定客户、费用、期限、授权数量和特殊授权；仓库许可证不得误报为双方签署的合同。
