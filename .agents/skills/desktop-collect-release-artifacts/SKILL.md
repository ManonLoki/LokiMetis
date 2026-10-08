---
name: desktop-collect-release-artifacts
description: 在项目根 release 目录中取回、安全合并并验证当前下游项目已完成的本地、交叉编译或原生 CI 构建结果，同时保留每个候选的 pending、rejected 或 accepted 状态。用于本地或 CI 构建完成后、下载工作流产物时，或准备供人工复核的发布候选时。
---

# 收集发布产物

创建一个可复核的当前构建结果目录，不重新构建、签名、执行或发布候选。

GUI 先按根 Cargo metadata 的 `gui-framework` 分派，缺省兼容 Tauri。GPUI 只接收 `$desktop-build-gpui-release` 的 `candidate` 结果；`local` 试包不能进入发布集合。GPUI 保留原生 packager 身份、编译期日志与最终包内资源的字节证据，不加载 Tauri 配置、前端或 updater 专用检查。

GPUI 的 `bundleFormat: app` 对应 `.app.zip` 与 `artifactKind: application-archive`；`dmg|nsis` 对应 `artifactKind: installer`。三者都须核对每次唯一 `buildRun`、`interface: gui` 和 `guiFramework: gpui`，不能把应用归档当作安装行为已验证。

## 工作流程

1. 读取 `docs/RELEASE.md`、活动构建请求、每个已完成构建 manifest 中的本次 `e2eSelection` 和签名选择，以及用户提供的每项结果；只有候选经过独立验收时才读取匹配证据。收集阶段不得补问、推断或改变构建选择，也不得改写发布上下文或更新日志。要求规范化 Git 顶层等于项目根；在接触目标目录前运行发布上下文 `verify`，锁定 clean 当前 HEAD、`releaseContextSha256`、上下文默认主分支、`expectedTag` 与 `releaseReview`。本地主分支/tag 与所有源 manifest 的 `sourceCommit` 必须等于当前 HEAD；各 manifest 的上下文摘要与审查字段逐字段匹配，候选选择则与原始构建 manifest 及声明的构建证据一致。不能用对话或单个 manifest 补齐缺失上下文。随后调用 `$desktop-manage-version check --phase build`；所有候选版本必须等于当前目标和上下文版本，收集不得提升版本。在接触目标前确定项目、版本、源码提交、平台矩阵、产物名称、校验和、构建标识、签名状态和候选状态。该检查只验证本地 Git 发布事实，不限制分支拓扑。
2. 对当前宿主结果接受已完成的本地构建目录。只有本次明确请求三平台原生候选、发布后推送已另行授权并完成，且远端默认主分支/tag 经只读复核均指向当前 `sourceCommit`，才通过已配置 provider 取回矩阵结果；不得在收集时自行推送。对于每个预期平台/架构和产物类别，选择与当前项目、版本、源码提交和明确构建标识匹配的已完成结果。绝不得把含糊的提供方“最新”结果、单独的文件系统修改时间（mtime）、其他项目/提交、不完整运行或未经验证的目录视为已选结果。
3. 复制前构建并验证完整源清单。要求每个源文件都存在、属于普通文件、位于目标目录之外，并匹配项目/版本/提交/构建。为 `$desktop-build-rust-release`、`$desktop-build-tauri-release` 或 `$desktop-build-gpui-release` 收集候选结果时可以接受 `milestoneAcceptance: pending`。必须保留原精确选择与状态，绝不得称其为已验证。发布就绪复核仍要求所有匹配 manifest 组成完整 `Milestone accepted` 原子集合。拒绝缺失、额外、重复、空、过时、跨项目或发生冲突的文件。
4. 将目标目录精确解析为 `<canonical-project-root>/release`，但在全部源结果通过前不得清理、移走或写入它。遇到目标或父目录为符号链接/重解析点、规范化后路径越界、目标非目录或目标等于项目根时必须拒绝。在项目根同级、同一文件系统创建唯一且权限受限的 staging，要求它是目标外普通目录；后续复制、展平和验证都只在 staging 完成，不得依赖 ignore 隐藏工作树内暂存项。
5. 仅把已选择的当前源清单文件复制到 staging。只有完成全局文件名冲突检查后，才可展平提供方包装目录。仅保留声明的最终用户二进制文件或归档、相邻校验和/签名文件、清单和明确要求的验证证据；绝不得复制 Cargo 中间产物，也不得逐个平台直接复制到 `release/`。
6. 在 staging 内重新计算每个最终归档或安装包的 SHA-256，并与相邻校验和及清单比较。验证各清单的 `project`、机器 `version`、`sourceCommit`、`releaseContextSha256`、`buildRun`、`buildMode`、平台/架构/target/host、制品/摘要、测试、E2E、审查、`releaseNotesVersion`、`releaseNotesSha256`、`releaseNotesPath`、签名与 `milestoneAcceptance` 字段一致；再次运行发布上下文 `verify`，要求 HEAD、上下文摘要、`releaseReview` 与第 1 步及全部 manifest 的审查绑定逐字段相等。候选 `e2eSelection` 和 GUI `candidateSelections` 只核对当前构建清单及相邻证据，不要求它们存在于发布上下文。只读校验根更新日志及 staging 归档/应用资源内同一路径内容。要求本次请求预期的平台/架构精确一次；当前宿主构建只接受当前宿主结果，三平台矩阵必须精确覆盖三平台。GUI 清单继续验证 interface、artifactKind、bundleFormat、runtimeVerification 和签名/公证状态。
   - 发布审查 `enabled` 时要求 `reviewedSourceCommit`、`reviewEvidence` 与上下文字段一致；不对它与候选 `sourceCommit` 施加祖先、线性或允许路径门禁。审查 `disabled` 时两字段都必须缺席。
   - `dmg` 已签名时只接受 `notarized-and-stapled`；`nsis` xwin 时只接受 `buildMode: cross-compiled-xwin` 与 `runtimeVerification: Unverified`。不得尝试新签名、公证或 stapling。对于 CLI 就地签名，要求存在已记录的固定钩子验证结果，并要求 `detachedFiles` 列表为空；项目声明独立签名证据时，明确要求每个被引用的普通文件及其校验和存在。
7. 检查 staging 中全部归档内容但不得执行二进制文件；拒绝绝对路径、父目录穿越和非预期载荷。只有本次构建选择或产品/渠道硬要求启用了冒烟/E2E 时，才验证已有对应证据。收集过程绝不得自行启动冒烟/E2E。
8. 在触碰目标目录前第二次运行发布上下文 `verify`，要求 HEAD、上下文摘要与 `releaseReview` 等于第 1/6 步快照，且全部 manifest 的 `sourceCommit` 相同；三平台矩阵另复核远端默认主分支/tag 仍指向同一提交。重新枚举 staging 并按全部清单复算精确集合。只有全部成功，才在不跟随链接的同一提交临界区把完整 staging 目录级原子替换为 `release/`；提交失败必须恢复原目标，任何前置失败都不得部分污染目标。替换后只读重新枚举 `release/` 并复算。只在当前 manifests、其声明的相邻制品证据和最终回复中记录结果；不得创建或更新 Product Spec、ADR、Changelog、Product Status、Work Plan 或 Verification。独立触发的渠道分发或回顾审计记录各自新增事实。

## 输出契约

项目根 `release/` 目录只能包含本次构建调用选中的以下文件：

- 每个成功构建的平台/架构对应一个归档、安装包或声明的二进制文件；
- 每个产物对应一个相邻的 `<artifact>.sha256` 校验和，以及任何声明的独立签名；
- 每个平台对应一个清单，或一个包含等价字段的聚合清单；
- 由清单命名并引用的可选测试或日志证据。

每个清单必须保留 `pending`、`rejected` 或 `accepted` 中的一种状态；目录存在绝不得提升该状态。`failed`、缺失以及被硬要求覆盖的 `Not run`/`Unverified` 都不能随收集升级。绝不得包含凭据、绝对本机路径、缓存或未遮盖的环境转储。

## 边界

- 收集不能证明构建可信，也不能替代 `$desktop-verify-delivery`。
- 当前宿主收集只接受当前宿主结果，不访问 provider/remote；经独立推送且明确请求三平台矩阵时才可取回远程结果。Git 发布本身不要求推送或构建。
- 构建产物收集本身不触发任何项目记忆，既有 `pending`、`rejected` 或 `accepted` 状态只能原样保留在 manifest 及其声明证据中。
- 不得构建、发布、上传到发布渠道、签名、公证、stapling、创建标签或更改版本文件。
- 除第 4–8 步声明的项目根同级、同一文件系统、唯一且权限受限的 sibling staging 外，不得在精确且非符号链接的 `<canonical-project-root>/release` 目录之外清理或写入；该 staging 只用于形成整组原子替换，失败时也只能清理已复核的本次 staging 精确路径。
- 原子提交会替换 `release/` 中的历史构建结果；只有完整 staging 已验证且提交临界区能够恢复原目标时才允许进入，不能逐文件删除或覆盖。
- 即使存在其他文件，缺失或不一致的必需结果、未通过的必需验收门禁仍必须阻断就绪状态。
