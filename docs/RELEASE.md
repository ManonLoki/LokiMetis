# 版本、Git 发布与 GPUI 候选

当前版本唯一取根 Cargo.toml；每次受管 Git 发布由 schema v3 上下文、生命周期清单及对应默认主分支/tag 共同绑定，实际发布锚点从这些事实复读，不在本规则文档固定旧版本。当前产品目标见批准规格及持续计划，开发预览不能作为最终产品交付。目标平台取根Cargo metadata，Linux双架构工程合同见[Linux GPUI工具链](LINUX_GPUI_TOOLCHAIN.md)；各平台候选构建及真实运行验收单独取证。当前依赖和打包工具版本索引见[技术选型清单](TECHNOLOGY.md)，具体参数取根清单和打包Skill。

## 版本与开发周期

版本只由 $desktop-manage-version 管理，.harness/version-state.json 受保护。上一 Git 发布主分支/tag 复核后，在下一开发分支 finalize-release 新周期。疑似新需求优先 feature，本周期首个完成且相关测试通过的功能提升 Minor；确认非新需求的修复/可感知优化按稳定 ID bug-fix 独立提升 Patch。维护/check/plan 不写版本。

Minor/Patch 为 0..99 base-100，自动进位不等同显式 Major 授权，Major 不超过 Cargo u64::MAX。任何历史 Minor/Patch=100 失败关闭，由负责人先修正。事实冲突先调查和修复底层漂移，再稳定 ID/证据走 record-reconciliation 强制新的 Minor、适用完整验证和 Git 发布；相同证据重试幂等，历史最低 required_version 低于最终版不算冲突。

## Git 发布

1. 新功能/独立修复写入前由生命周期 Skill 创建登记 feature 分支。明确发布才整理本次范围、秘密、提交完整性与必要测试，冻结 sourceHead；本次 reviewSelection 单独解析，安全/隐私/不可逆/对外契约/渠道硬要求强制启用，其余询问一次，同次返工复用。
2. 根 release-notes.json 首次正式发布准备才创建，schemaVersion 2，最新在前，含当前版最近十个实际版本，每版功能优化/问题修复各最多十个中英翻译对；缺语言/空白/重复失败关闭。普通修复即使不触发 Changelog 仍进发布日志，不复制历史。
3. sourceHead 仍精确等于 HEAD 时生成 .harness/release-context.json，与且仅与发布日志同次提交；上下文保存版本/日期/默认主分支/预期 tag/源码身份/审查，不保存候选签名或推送位置。启用审查绑定 sourceHead 的范围证据；关闭保留 Not run 原因和风险。
4. 生命周期 release 收到精确 --release-context-sha256，普通合并已登记分支至本地默认主分支，创建并复读 v{版本}-{YYYYMMDD} 指向最终 HEAD。主分支/tag/上下文一致即 Git 发布完成，不 fetch/push/打包/清理资源。sourceHead 是审查输入，sourceCommit 是最终发布提交，二者不要求相等。
5. 发布开始冻结 Policy 的 post_release_action；当前 local_package 随后进入 GPUI candidate 模式并检测真实结果，失败保留 Git 发布事实。未来切换不改旧发布。若切换 push_release_branch，另行安全推送默认主分支、小写 release 和 tag 并逐项复读，不创建远端/凭据。

## 本地试包与候选

普通构建/试包使用 $desktop-build-gpui-release 的 local 模式，不要求 release-notes、不生成 release/ 或发布元数据、不提交、不询问候选 E2E，不升级解释为发布请求。

macOS local 试包对完整 App 进行无证书 ad-hoc 签名并验证稳定身份、Info.plist 与资源封印；DMG 在封盘前完成签名，最终归档/挂载卷再次核验。该本地身份不代表 Developer ID、公证、通知授权或实际横幅通过，candidate 的签名门禁仍独立执行。

candidate 模式只接受 clean 已发布默认主分支/tag，构建前及写 manifest 前复核上下文摘要/提交/tag/审查选择一致。每次先单独解析当前 E2E；产品/渠道硬要求优先，否则复用请求或以 e2e_hint 建议询问。全 workspace/all-targets/all-features 非空测试失败即阻断。依赖锁 ignored 不冻结，tracked 先核对跟踪与清单一致后冻结。

macOS 在原生宿主形成 app/DMG，Windows x64 NSIS 在真实 Windows 宿主形成；Linux GNU x64/ARM64形成deb/tar.gz，允许同系统跨架构构建。GPUI不复用Tauri/xwin管线。签名缺省disabled/not-requested，不主动探测本机身份；来源优先级channel-required > requested > configured。当前GPUI工具未实现macOS Developer ID签名/公证/stapling及Linux签名，启用请求或渠道硬要求必须在测试前阻断，不能声称这些路径已完成。Windows仅在明确启用且已有批准证书指纹/时间戳配置时执行signtool签名与验证。渠道资格、商店、签名与第三方NOTICE依据当次明确要求核对，不虚构凭据或渠道授权。

当前项目的macOS正式候选还受系统通知门禁阻断：profile中`system_notification: enabled`要求candidate显式启用签名，而工具尚未实现该签名路径，因此disabled与enabled都不能生成该候选；local ad-hoc试包仍按上节执行。Windows的`windows.extractor`当前为null，NSIS正式候选同样须先有已批准的项目内解包器；普通local试包不因此升级为已验收候选。不能通过文档同步、关闭硬门禁或沿用旧产物声称这些候选已可用。

候选包含用户可访问的两份完整商业许可证，名称与当前身份一致；第三方许可/NOTICE 单独核对，不被专有许可覆盖。固定母版衍生 ICNS/ICO、660×400 中性 DMG 背景按 GPUI packaging 配置读取；macOS 最终 DMG 只读挂载核对唯一 app、背景、Applications 目标、DS_Store 以及发布日志资源字节。任何后补布局/签名/重包都重新算摘要和验收。

## 候选证据与验收

所有候选写忽略的根 release/，安全拒绝越界/链接，旧目录原子隔离，新集合暂存验证后原子提交。产物命名 产品名-vMAJOR.MINOR.PATCH-平台-架构.扩展名，相邻 .sha256 与 manifest 精确对应。取回不混旧版本/项目/运行、额外、重复或来源不明文件。

manifest 至少绑定项目、原始版本、40 位 sourceCommit、expectedTag、releaseContextSha256、构建/运行身份、模式/平台/架构/目标/宿主、产物名/摘要、全量测试、当次 E2E/审查/签名选择与证据、更新日志版本/摘要/包内路径和 milestoneAcceptance=pending。审查启用时 reviewedSourceCommit 等于上下文 sourceHead，且为最终提交祖先，最终差异只含冻结发布元数据；关闭时不附审查证据字段。远程 provider 另外复核已明确推送的远端 release/tag。

候选状态仅 pending/rejected/accepted；ready 为只读就绪结论。E2E 只对最终完整真实候选按选择执行，禁用记录 Not run 和风险。完整验收前后两次复核 clean 默认主分支/tag/上下文/全部 manifest，复算实际字节；所有启用/必需项通过才原子 accepted。人工批准不能把失败/未执行改为通过。未运行平台明确 Unverified，开发预览/Mock/scaffold 不可验收为产品。

构建、重试、测试、签名、E2E、验收和就绪只写 release/ 原子证据及任务输出，不触发 tracked 项目记忆。真实渠道分发或独立回顾审计才按各自事件建立验证记录。候选不等于渠道分发，构建不授权上传、GitHub Release、仓库发布或凭据创建。
