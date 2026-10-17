# 诡秘神谕 · LokiMetris

LokiMetris 是一个面向 Windows、macOS 与 Linux 的 GPUI 原生 Rust 桌面应用，提供本机 AI Agent 用量看板、Hooks 监控与桌宠展示，并整合应用换肤。当前目标是参照历史 LokiMetis 源码和已安装 App 复刻整体体验；批准范围见 [`docs/product_spec/20261018_product_spec.md`](docs/product_spec/20261018_product_spec.md)，进度见 [持续计划](docs/work_plan/20261009_work_plan.md)。整体体验复刻尚未完成。

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
- 字体随 App 内嵌，不进行全局包或字体安装；历史源码目录和已安装参考 App 都不是运行时依赖。
- 保留已选金色神谕之眼 Logo 和独立 `com.loki-metris.app` 身份，不会覆盖参考 App。

## 目录

```text
.
├── loki_metris_core/      # 平台无关的领域规则、用例与稳定错误
├── loki_metris_gui/       # GPUI 原生宿主与展示层
├── vendor/                # 带补丁的 GPUI 与 accesskit_macos（原样上游 + 校验过的补丁）
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

macOS 默认派生数据位于 `~/Library/Application Support/app-loki-metris/data`。验证时可用绝对路径环境变量 `LOKI_METRIS_DATA_DIR` 隔离派生库；它不会改变原始 Agent 目录，也不会复制历史 App 数据库。

```sh
LOKI_METRIS_DATA_DIR="$PWD/target/parity-data" pnpm run dev
```

皮肤正式运行资源来自安装包的 `Contents/Resources/skins`（macOS）或安装根 `resources/skins`（Windows/Linux）；缺失时显示错误。源码运行由 `pnpm run dev` 设置开发资源变量，release 二进制忽略该变量。

## 构建与发布边界

- 本机试包使用 `pnpm run gpui:package build --root . --mode local --format dmg`；macOS 完整 App 使用无证书 ad-hoc 本地签名，Windows 保持 unsigned。试包不代表 Developer ID、公证、系统通知或最终候选验收通过。
- 版本只由 `$desktop-manage-version` 在真实变化完成并通过相关测试后提交。
- 正式候选必须来自干净且明确的源码提交，产物写入被忽略的 `release/`。
- 调试预览不构成安装包或最终候选验收。
- tag、push、上传、签名、公证和渠道发布均需要各自明确授权。

版本、构建和候选合同见 [`docs/RELEASE.md`](docs/RELEASE.md)。

Linux GNU x64/ARM64的安装、检查、同系统交叉构建和deb/tar.gz打包统一见[Linux GPUI工程知识库](docs/LINUX_GPUI_TOOLCHAIN.md)。当前声明目标与实际运行验收分开，完整托盘/热键、桌宠及窗口原生能力不能据构建成功推断通过。

## 许可

本项目使用仓库根目录的企业专有商业许可证：[`LICENSE.zh-CN.md`](LICENSE.zh-CN.md) 与 [`LICENSE.en.md`](LICENSE.en.md)。字体许可与来源在 `loki_metris_gui/assets/fonts/`；第三方许可不被本项目许可覆盖。

## 赞助

如果 LokiMetris 对你有帮助，可以使用微信支付或支付宝扫码赞助。收款码仅作为静态图片展示，不会触发自动支付。

| 微信支付 | 支付宝 |
|---|---|
| <img src="loki_metris_gui/assets/sponsor/wechat-pay.png" alt="LokiMetris 微信支付赞助收款码" width="300"> | <img src="loki_metris_gui/assets/sponsor/alipay.jpg" alt="LokiMetris 支付宝赞助收款码" width="300"> |
