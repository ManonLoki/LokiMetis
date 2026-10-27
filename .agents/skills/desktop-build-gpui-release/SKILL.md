---
name: desktop-build-gpui-release
description: 通过项目根 pnpm 入口构建 GPUI 的 macOS app/DMG、Windows x64 NSIS、Linux GNU x64/ARM64 deb/tar.gz，区分本地试包与已发布候选。
---

# 构建 GPUI 安装包

只用于根 Cargo 已选择 GPUI GUI 的终端下游。Harness 源只维护工具，不在模板根打产品包。一个入口复用源码、测试和候选合同，按平台读取细节；不为每个平台再建一套发布生命周期。

## 模式与路由

- 普通构建/试包使用 local：允许初始 dirty，随后源码与资源快照保持稳定；运行全部非空 Rust workspace 测试，只写忽略的 target/gpui-packaging/local-*。不创建发布上下文，不执行安装/启动，不询问候选 E2E。
- 明确发布候选或已冻结 local_package 才用 candidate：只消费 clean 已发布默认主分支、精确 tag、已跟踪发布上下文和双语日志。复用本次 E2E 明确选择，否则按策略询问一次；偏好不能跳过硬要求。

读取项目构建相关 Policy、RELEASE、存在时的 VERIFICATION，再按需要读取 [configuration](references/configuration.md)。macOS 读 [macos-packaging](references/macos-packaging.md)，Windows 读 [windows-packaging](references/windows-packaging.md)，Linux 读 [linux-packaging](references/linux-packaging.md)。仅当前平台需要时加载对应引用。

Linux 应用中心发现、安装图标、升级或卸载问题，再读 [应用目录与桌面安装集成](references/linux-desktop-integration.md)。本项目已通过 `gpui_linux_appstream.mjs` 生成 DEP-11 catalog；文档中的中性上游 helper 缺口不撤销本地实现。包内身份、原生解析、包管理事务和桌面效果分别取证。

项目根入口为 pnpm run gpui:package；Node >=24.21.0、pnpm >=12.4.1，仅用 Node 标准库，不需要先安装 npm 依赖。Cargo metadata 决定 GUI package/binary、目标、版本、额外测试工作区和锁策略，配置不重复存产品版本。

## 执行合同

先 preview 解析所选 target/format/mode；它不安装工具、不运行测试或探针。setup 是有副作用的显式入口，具体行为见平台引用，不因普通开发自动执行。icons 只补缺失或同字节的品牌资产；不同资产回身份流程，不覆盖。

~~~sh
pnpm run gpui:package preview --root . --mode local --format app
pnpm run gpui:package build --root . --mode local --format app
~~~

实际 build 对每个真实 Rust workspace 先列出再运行全部非空单元测试。tracked 锁要求 metadata/test/build 使用 --locked；ignored 不假装冻结解析。Linux 交叉架构测试运行真实宿主并单独报告目标构建。失败或零测试阻断。

关于页启用或 releaseNotesUi:true 时，候选生成临时 Rust 静态日志模块，build.rs 通过 HARNESS_GPUI_RELEASE_NOTES_RS 消费，二进制保留同一原始 JSON 字节。其他情况不注入日志 UI，所有候选仍核对包内独立日志与双语许可证。local 不生成发布记录。

资源按配置的显式平台映射捕获精确文件集与摘要，包括 Git 忽略的资源；测试后、封包前、manifest 前复核源码/配置/资源/上下文。路径、链接、大小写/前缀冲突及漂移失败关闭。最终包真实解包核对资源与二进制，不能只验证打包前目录。

candidate 的最终包、SHA-256 与 manifest 在同根暂存后原子发布到忽略的 release/。保留 milestoneAcceptance:pending、runtimeVerification:Unverified；签名、解包、目标运行和 E2E 分别报告。E2E 启用或硬要求时最终候选形成后才交 $desktop-verify-delivery，不混入构建。失败保留中间物与隔离的旧候选，不改 Git 发布事实。

## 完成输出

报告模式、源码与 dirty 摘要、宿主测试数量、host/target/buildMode、格式、实际工具路径和版本、产物/哈希/manifest、资源/签名与当前 E2E 选择。安装、运行及未验证平台据实标注。当前产品的专属阻断以其 RELEASE 为准，不写成全部下游的能力限制。
