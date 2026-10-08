---
name: desktop-build-gpui-release
description: 通过项目根 pnpm 工程入口为 GPUI Rust GUI 原生构建 macOS app/DMG 或 Windows x64 NSIS；独立处理本地试包和已发布源码候选。
---

# 构建 GPUI 安装包

只用于根 Cargo metadata 的 `interfaces` 含 `gui` 且 `gui-framework = "gpui"` 的终端下游。Harness 源维护、Tauri、Linux GUI 和跨系统打包不走此入口。不调用前端、Tauri 插件、远端、安装、启动或渠道分发。

## 选择模式

- 普通“构建/打包/安装试一下”使用 `local`：允许 dirty，但全量非空 Rust workspace 单元测试必须通过；只写项目忽略的 `target/gpui-packaging/local-*`，输出未签名、未安装、未运行且仅供本机检查的试包，不询问 E2E、不生成发布上下文或日志。
- 用户明确要求发布候选，或 Git 发布已确认 `local_package`，才使用 `candidate`：只消费 clean 已发布默认主分支、精确本地 tag 和已跟踪 `.harness/release-context.json`。每次单独解析当前请求的 E2E；先复用本次明确选择，否则结合 `e2e_hint` 询问一次。选择关闭时记录原因与风险，硬要求不能关闭。

读取 `AGENTS.md`、`docs/AGENT_POLICY.md` 的构建相关字段、`docs/RELEASE.md`、`docs/VERIFICATION.md` 与 [配置合同](references/configuration.md)。候选只读复用 `$desktop-prepare-release` 的发布上下文和日志校验、`$desktop-manage-version` 的版本检查；缺失时阻断，不在构建中补写。

## 原生执行

在项目根通过 `package.json` 的 `gpui:package` 脚本调用自含的 Node 标准库 helper。工程运行时要求稳定版 Node.js `>=24.21.0`、pnpm `>=12.4.1`，由根 `engines` 声明；不增加 npm 第三方依赖，不要求 `pnpm install`、前端测试或前端锁文件。`packaging/gpui.json` 与根工程入口由 GPUI 初始化模板生成；Cargo metadata 决定实际 workspace、GUI 包/二进制、版本、`target-platforms`、`gui-root`、`rust-test-manifests` 和锁策略。目录名不得代替这些事实。既有工程缺少入口时按[配置合同](references/configuration.md)安全合并，不覆盖已有产品脚本。

先只读预览：

```sh
pnpm run gpui:package preview --root . --mode local --format app
```

首次明确需要打包工具时，在项目私有 `target/harness-tools/cargo-packager-0.11.8` 安装固定版本；工具已存在则验证版本，不覆盖不同版本或残缺目录。此调用安装工具，不安装生成的应用，不修改产品依赖。

cargo-packager CLI 本身不依赖全局安装。0.11.8 内部下载的 create-dmg/NSIS 辅助工具使用其官方固定 OS 用户缓存 `.cargo-packager`，没有 tools-dir 配置；不把该缓存说成项目隔离，也不改 HOME。缓存不是可假设已存在的全局 CLI，真实缺失/下载失败仍阻断打包。

```sh
pnpm run gpui:package setup --root .
```

初始化已从已选 PNG 生成 ICNS/ICO 与中性 DMG 背景时无需补做图标。既有 GPUI 接入可用 `icons` 仅填充缺失或相同字节的项目本地图标；不同资产不得覆盖，回身份流程处理。源 PNG 必须为方形 256..2048px、非隔行、8-bit RGB/RGBA；中性 SVG 不能冒充确认后的打包 Logo。

```sh
pnpm run gpui:package icons --root .
```

macOS 原生支持当前架构或已安装 Apple target 的 `.app.zip`/DMG；Windows x64 原生支持 MSVC NSIS。目标必须已列入 Cargo metadata。Linux、Windows ARM、Docker/xwin 跨系统编译尚无受管产物证据，明确阻断；示例项目有 Docker 实验路线不等于当前 Skill 支持它。

```sh
pnpm run gpui:package build --root . --mode local --format app
pnpm run gpui:package build --root . --mode local --format dmg
pnpm run gpui:package build --root . --mode local --target x86_64-pc-windows-msvc --format nsis
```

显式候选示例；参数须来自当前选择：

```sh
pnpm run gpui:package build --root . --mode candidate --format dmg --e2e disabled --e2e-reason "本次未选择 E2E" --e2e-risk "安装与运行行为尚未验收" --signing disabled --signing-source not-requested
```

## 候选门禁

helper 在测试前验证独立 Git 根、clean 默认主分支/tag、发布上下文、base-100 版本、锁策略及资源；原子隔离旧 `release/`，拒绝链接/重解析、路径越界和覆盖。所有实际 Rust workspaces 分别列出非空全量测试并运行 `cargo test --workspace --all-targets --all-features`；`tracked` 的 metadata、测试、构建全部传 `--locked`，同时核对受跟踪且未被忽略的实际 Cargo.lock。pnpm 只提供 Node 工程命令入口；不触发 React、前端安装/测试或前端锁门禁。

关于页启用时，候选从已验证的根 schema v2 `release-notes.json` 生成临时 Rust 常量，设置 `HARNESS_GPUI_RELEASE_NOTES_RS`；GPUI `build.rs` 复制到 `OUT_DIR/release_notes.rs`，关于页直接使用静态双语条目，二进制必须真实保留同一原始 JSON 字节。关于页禁用时不注入日志 UI。两种情况都逐字节核对包内独立 JSON 与两份许可证；缺少已选关于页的模板合同就阻断候选，不能把“无发布记录”的中性占位当正式日志。

macOS 默认 `disabled/not-requested`、unsigned，不探测身份/凭据；当前 Skill 未实现完整 macOS 签名公证，任何启用请求或渠道要求在测试前失败关闭。不能关闭渠道要求继续构建。Windows 有批准的证书指纹与 HTTPS 时间戳配置时，显式 `--signing enabled --signing-source configured|requested|channel-required` 才执行 `signtool sign` 与 `verify /pa /all`，任何失败停止；不创建或输出凭据。机器恰有工具/环境变量不改变选择。

macOS `.app.zip` 用系统 `ditto` 打包，DMG 用 cargo-packager 固定原生配置；Finder 布局阶段有十分钟上限。候选 DMG 在最终字节上只读挂载，核对 `.DS_Store`、项目背景、唯一 `.app`、`/Applications` 拖拽目标与全部资源；headless 无可用 Finder 布局时阻断，不能把缺布局的 DMG 记为通过。Windows NSIS 候选要求项目内显式批准且已声明的 `7z/7zr` 解包器，以只读解包核对最终包内日志、许可证与二进制；缺少时阻断，不自动增加第三方工具。local NSIS 可打出试包，其最终资源/安装/运行保持 `Unverified`。

测试后、打包前及写 manifest 前再次核对源码和配置；源码、上下文或选择漂移均停止。最终安装包或 app 归档、相邻 SHA-256、manifest 在同根暂存区形成精确普通文件集合，再目录级原子提交到忽略的 `release/`。manifest 保留通用字段、实际 packager/GPUI 身份和 `milestoneAcceptance: pending`、`runtimeVerification: Unverified`；不伪造 updater/Tauri 版本，也不写 tracked 项目记忆。

E2E 为 `enabled` 或硬要求时，最终候选形成后调用 `$desktop-verify-delivery`；编译/打包期间不混跑安装、启动或 E2E。关闭时准确报告 `Not run` 与风险。构建通过只证明真实产物已生成，不能称已验收或发布就绪。

## 完成输出

报告模式、实际源码/dirty 状态、Rust 全量测试数量、target/format、工具版本、真实产物/哈希/manifest、签名与资源检查、当前 E2E 选择及未执行的安装/运行、未验证平台。失败保留本次隔离中间物和旧候选隔离目录，不覆盖源码、不安装应用、不改 Git 发布事实。
