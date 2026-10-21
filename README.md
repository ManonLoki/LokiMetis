# 诡秘神谕 · LokiMetris

LokiMetris 是一个面向 Windows、macOS 与 Linux 的 GPUI 原生 Rust 桌面应用，提供本机 AI Agent 用量看板、Hooks 监控与桌宠展示，并整合应用换肤。当前目标是参照历史 LokiMetis 源码和已安装 App 复刻整体体验；批准范围见 [`docs/product_spec/20261021_product_spec.md`](docs/product_spec/20261021_product_spec.md)，进度见 [持续计划](docs/work_plan/20261018_work_plan.md)。整体体验复刻尚未完成。

## 当前状态

- 项目标识：`loki_metris`
- 负责人：ManonLoki
- 当前版本：以根 `Cargo.toml` 为准
- Rust 架构：`loki_metris_core` + `loki_metris_gui`
- 界面：GPUI 原生（无 WebView）
- 目标平台：Windows、macOS、Linux（GNU x64/ARM64工具链；原生运行差异见Linux知识库）
- 产品定义：已批准；Windows 运行与完整安装包尚未验收

## 当前实现

- Codex、Claude Code、Grok 读取本机会话并写当前 App 自己的派生索引；WorkBuddy 只读其两个固定根的 project JSONL，联合视图遵循 core 核算口径。
- 看板、调用/统计/图表、来源发现与索引、用量设置、监控档案/图库/Hooks、独立桌宠与公共偏好。
- 皮肤页已接入六套内置资源、用户主题创建、ZIP 预检/导入/导出和宿主应用编排。
- 设备管理提供桌面LAN服务与四位码配对，Android/iPhone主动发现和连接；推送Token/Hooks，缺失图片按摘要补取并缓存。Watch经iPhone配套同步，iOS组件读取最近缓存，见[联动协议](docs/DEVICE_LINK_PROTOCOL.md)。
- 字体随 App 内嵌，不进行全局包或字体安装；历史源码目录和已安装参考 App 都不是运行时依赖。
- 保留已选金色神谕之眼 Logo 和独立 `com.loki-metris.app` 身份，不会覆盖参考 App。

Linux 已接通 KSNI 托盘、X11/XWayland 窗口与桌宠宿主、freedesktop 通知；v0.3.11 ARM64 GNOME XWayland 最终候选的16项必需场景已验证，v0.3.13双架构deb已实际打包、运行仍为Unverified。纯 Wayland 的桌宠全局定位/置顶和 Portal/X11 后台热键服务尚未接通，当前快捷键合同为空，不注册系统键位。已验证范围及其证据见 [Linux 工程知识库](docs/LINUX_GPUI_TOOLCHAIN.md#已验证能力与证据索引)，能力选择见 [GUI 资料](docs/GUI_APP_PROFILE.md)。

## 技术栈

Rust 2024 / GPUI Kit `0.7.3`，业务数据采用 SeaORM `2.0.4` + SQLite，时间处理采用 Jiff `0.2.38`，后台服务使用应用拥有的 Tokio `1.53.3` 运行时。中英词典使用 rust-i18n `4.2.4`；Node.js/pnpm 只运行工程脚本，根工程没有 npm 第三方依赖。

以上为 Cargo 声明范围，完整依赖、features、原生平台条件和本地补丁见[技术选型与依赖清单](docs/TECHNOLOGY.md)。当前工作区不跟踪 `Cargo.lock`，不能把声明版本当作冻结解析结果。

## 目录

```text
.
├── loki_metris_core/      # 平台无关的领域规则、用例与稳定错误
├── loki_metris_gui/       # GPUI 原生宿主与展示层
├── vendor/                # GPUI、AccessKit、tray-icon 兼容层及共享补丁生成器
├── docs/                  # 工程、GUI、版本与发布事实
├── .agents/skills/        # 本项目保留的工作流入口
├── packaging/             # 打包配置
├── scripts/               # 开发与预览脚本
└── Cargo.toml             # workspace、目标平台与当前版本事实
```

Core-first 是硬规则：不依赖 GPUI 或操作系统才能成立的规则必须进入 `loki_metris_core`；`loki_metris_gui` 只负责展示、交互与原生机制。

## 开发

先阅读 [`AGENTS.md`](AGENTS.md) 的任务路由。Rust >= 1.98.1、Node.js >= 24.21.0、pnpm >= 12.4.1；根工程入口无 npm 第三方依赖，`Cargo.lock` 被忽略。

```sh
pnpm run dev
cargo test -p loki_metris_core --lib
cargo test -p loki_metris_gui --bin loki_metris_gui
```

业务派生数据的默认目录：

| 平台 | 路径 |
| --- | --- |
| macOS | `~/Library/Application Support/app-loki-metris/data` |
| Windows | `%LOCALAPPDATA%/app-loki-metris/data` |
| Linux | `~/.local/share/app-loki-metris` |

验证时可用绝对路径环境变量 `LOKI_METRIS_DATA_DIR` 替换业务派生数据根；它不改变原始 Agent 目录、独立的 GUI 偏好目录或系统注册，也不会复制历史 App 数据库。以下为 Bash 示例：

```sh
LOKI_METRIS_DATA_DIR="$PWD/target/parity-data" pnpm run dev
```

皮肤正式运行资源来自安装包的 `Contents/Resources/skins`（macOS）或安装根 `resources/skins`（Windows/Linux）；缺失时显示错误。源码运行由 `pnpm run dev` 设置开发资源变量，release 二进制忽略该变量。macOS 需要完整 bundle 身份的开发预览使用 `pnpm run gpui:preview`，仅准备本地 ad-hoc App，不自动启动。

## 构建与发布边界

- 本机试包按平台选择下表命令；每次实际打包会运行全 workspace 非空测试。macOS 完整 App 使用无证书 ad-hoc 本地签名，Windows/Linux 试包保持 unsigned。试包不代表系统通知或最终候选验收通过。
- 版本只由 `$desktop-manage-version` 在真实变化完成并通过相关测试后提交。
- 正式候选必须来自干净且明确的源码提交，产物写入被忽略的 `release/`。
- 调试预览不构成安装包或最终候选验收。
- tag、push、上传、签名、公证和渠道发布均需要各自明确授权。

| 原生宿主 / 目标 | 本地试包命令 |
| --- | --- |
| macOS | `pnpm run gpui:package build --root . --mode local --format dmg` |
| Windows x64 MSVC | `pnpm run gpui:package build --root . --mode local --target x86_64-pc-windows-msvc --format nsis` |
| Linux GNU x64 | `pnpm run gpui:package build --root . --mode local --target x86_64-unknown-linux-gnu --format deb` |
| Linux GNU ARM64 | `pnpm run gpui:package build --root . --mode local --target aarch64-unknown-linux-gnu --format tar.gz` |

这些是使用入口，不表示本次已执行打包。macOS/Windows 使用项目内固定 `cargo-packager 0.11.8`；Linux 使用系统 `tar`、`dpkg-deb`、`readelf`，允许同系统跨架构构建。版本、工具准备和候选合同见 [`docs/RELEASE.md`](docs/RELEASE.md)与[打包 Skill](.agents/skills/desktop-build-gpui-release/SKILL.md)。

Linux GNU x64/ARM64的安装、检查、同系统交叉构建和deb/tar.gz打包统一见[Linux GPUI工程知识库](docs/LINUX_GPUI_TOOLCHAIN.md)。当前声明目标与实际运行验收分开，完整托盘/热键、桌宠及窗口原生能力不能据构建成功推断通过。

## 文档导航

- [技术选型、依赖声明与 Harness 工程层](docs/TECHNOLOGY.md)、[Rust/GPUI 运行时与命令](docs/RUST_CLI_TEMPLATE.md)
- [产品规格索引](docs/product_spec/README.md)、[当前工作计划](docs/work_plan/README.md)
- [GUI 身份与实现边界](docs/GUI_APP_PROFILE.md)、[通知与出站入口](docs/GUI_SUPPORT_SURFACES.md)
- [工程规则](docs/ENGINEERING_RULES.md)、[Agent 策略](docs/AGENT_POLICY.md)、[vendor 维护说明](vendor/docs/VENDOR_OVERVIEW.md)
- [Hooks 协议](docs/HOOKS_PROTOCOLS.md)、[性能知识库](docs/PERFORMANCE_OPTIMIZATION.md)、[已知技术债](docs/TECH_DEBT.md)

本仓库保留 Harness 文档与 Skill 供本项目维护，是独立终端下游，不继续派生项目。受管 Tauri 设计资料不代表当前应用采用 Tauri。

## 许可

本项目使用仓库根目录的企业专有商业许可证：[`LICENSE.zh-CN.md`](LICENSE.zh-CN.md) 与 [`LICENSE.en.md`](LICENSE.en.md)。字体许可与来源在 `loki_metris_gui/assets/fonts/`；第三方许可不被本项目许可覆盖。

## 赞助

如果 LokiMetris 对你有帮助，可以使用微信支付或支付宝扫码赞助。收款码仅作为静态图片展示，不会触发自动支付。

| 微信支付 | 支付宝 |
|---|---|
| <img src="loki_metris_gui/assets/sponsor/wechat-pay.png" alt="LokiMetris 微信支付赞助收款码" width="300"> | <img src="loki_metris_gui/assets/sponsor/alipay.jpg" alt="LokiMetris 支付宝赞助收款码" width="300"> |
