# 诡秘神谕 · LokiMetis

LokiMetis 是一个面向 Windows、macOS 与 Linux 的 Tauri 2 GUI-only 本机 AI 工作台，提供本机 AI Agent 用量看板、Hooks 监控与桌宠展示，并整合 Codex 与 WorkBuddy 本机外观管理。当前批准范围与成功标准见 `docs/product_spec/20260908_product_spec.md`。

## 当前状态

- 项目标识：`loki_metis`
- 负责人：ManonLoki
- 初始版本：`0.1.0`；当前版本以根 `Cargo.toml` 为准
- Rust 架构：`loki_metis_core` + `loki_metis_gui`
- 界面：Tauri 2 + React + TypeScript + Mantine
- 目标平台：Windows、macOS、Linux
- 产品定义：已批准

产品目标、范围、约束或成功标准变化时，使用 `$desktop-define-product` 更新最新 Product Spec；日常实现仍遵守 core-first、最小权限与本次相关测试边界。

## 目录

```text
.
├── loki_metis_core/       # 平台无关的领域规则、用例与稳定错误
├── loki_metis_gui/        # Tauri 宿主与 React 展示层
├── docs/                  # 工程、GUI、版本与发布事实
├── .agents/skills/        # 本项目保留的工作流入口
├── Cargo.toml             # workspace、目标平台与当前版本事实
└── Cargo.lock             # 当前锁定依赖
```

Core-first 是硬规则：不依赖 Tauri、WebView 或操作系统才能成立的规则必须进入 `loki_metis_core`；`loki_metis_gui` 只负责展示、IPC 映射和桌面宿主机制。

## GUI 基线

当前固定启用系统托盘、系统通知、开机自启、设置页赞助支持、单实例与受限深链接；深链接仅接受 `app-loki-metis://restore`。侧栏使用不可折叠的 80px 紧凑模式。赞助内容位于设置页底部，不增加独立侧栏入口或 `/sponsor` 路由；全局快捷键未启用。

所有 GUI 固定提供中英文界面、浅色/深色/跟随系统主题、窗口状态恢复、受限 dialog，以及包含应用、版本和本地更新日志的设置页。详细事实见 [`docs/GUI_APP_PROFILE.md`](docs/GUI_APP_PROFILE.md) 与 [`docs/RUST_GUI_TEMPLATE.md`](docs/RUST_GUI_TEMPLATE.md)。

## 开发

先阅读 [`AGENTS.md`](AGENTS.md) 的任务路由。命令、脚本名称和参数必须以当前 `Cargo.toml`、`loki_metis_gui/package.json` 及相应 Skill 中实际声明的内容为准；不要假定全局第三方工具存在，也不要使用 README 中的示例替代项目脚本事实。

日常变化只运行本次需要的相关非空单元/回归测试。真实测试或构建已因受管工具缺失或明确低于下界而失败时，才由 `$desktop-check-development-environment` 安装或升级到官方兼容稳定版；范围内版本原样复用，只读检查保持零写入。完整 workspace 测试、Tauri 构建、性能门禁、安装包 E2E 和发布流程仅在对应任务明确触发时执行。

## 构建与发布边界

- 普通 Windows 本地安装试包是开发制品，不是发布候选。
- 新生成的 Minor/Patch 使用 `0..99` base-100 自动进位；版本只由 `$desktop-manage-version` 在真实变化完成并通过相关测试后提交。
- 正式发布先锁定当次 `reviewSelection`、`performanceSelection` 和 macOS 签名意图；构建只读消费这些选择并另行解析本次 E2E。性能启用时使用 `gui-release-v2`。
- macOS 签名默认 `disabled/not-requested` 且不探测本机身份或凭据；但本项目启用了系统通知，因此 macOS 最终候选必须启用并完成签名、公证、stapling、Gatekeeper 与最终验证，失败不能回退 unsigned。
- 正式候选必须来自干净且明确的源码提交，绑定真实平台、产物摘要、测试和选择；最终字节先在仓库同级同文件系统 staging 中形成并复核，再以目录级原子替换提交 `release/`。整组 manifest 只能一致为 `pending`、`rejected` 或 `accepted`。
- 应用不提供 updater、联网检查、强制更新、更新制品、产品统计或远程遥测。
- 根 `release-notes.json` 已存在，只由正式发布准备维护并作为候选内双语本地资源使用。
- tag、push、上传、签名、公证、商店提交和渠道发布均需要各自明确授权。

版本、构建和候选合同见 [`docs/RELEASE.md`](docs/RELEASE.md)。

## 许可

本项目使用仓库根目录的企业专有商业许可证：[`LICENSE.zh-CN.md`](LICENSE.zh-CN.md) 与 [`LICENSE.en.md`](LICENSE.en.md)。分发时还必须遵守第三方依赖许可证并提供适用的 NOTICE。

## 赞助

如果 LokiMetis 对你有帮助，可以使用微信支付或支付宝扫码赞助。收款码仅作为静态图片展示，不会触发自动支付。

| 微信支付 | 支付宝 |
|---|---|
| <img src="loki_metis_gui/public/brand-support/sponsor/wechat-pay.png" alt="LokiMetis 微信支付赞助收款码" width="300"> | <img src="loki_metis_gui/public/brand-support/sponsor/alipay.jpg" alt="LokiMetis 支付宝赞助收款码" width="300"> |
