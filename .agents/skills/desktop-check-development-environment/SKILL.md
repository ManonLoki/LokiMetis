---
name: desktop-check-development-environment
description: 仅在 GUI 中性初始化，或初始化后真实测试/构建已因受管环境问题失败时检查并补齐工具链；不得例行预检。
---

# 检查开发环境

为 GUI-only Rust/Tauri 项目建立或恢复受管工具链。

## 工作流程

1. 读取 `AGENTS.md`、Agent Policy、`docs/RUST_GUI_TEMPLATE.md` 与 [环境门禁](references/development-environment-gates.md)。只接受两类触发：中性初始化首次写入前；或真实测试/构建命令已失败且诊断明确指向受管工具缺失或不兼容。
2. 排除代码编译错误、测试断言、普通依赖网络、产品配置、凭据与签名失败。缺少历史环境证据、新任务、新会话或显式构建都不构成触发。
3. macOS/Linux 运行 `scripts/development-environment-gates.sh --install-missing`；Windows 运行 `scripts/development-environment-gates.ps1`。只读审计分别使用 `--check-only` 或 `-CheckOnly`。
4. GUI-only 常规门禁始终要求 Git、Rust、Node.js 和 pnpm；Windows 另要求 MSVC C++ 工作负载。不得按界面参数跳过 Node.js/pnpm，也不得接受旧接口选择参数。
5. Git 要求稳定版 `>=2.0.0`，Rust 使用 `docs/RUST_GUI_TEMPLATE.md` 声明的 MSRV，Node.js 要求 `^24.15.0 || >=26.0.0`，pnpm 要求 `>=11.24.0`。现有兼容稳定版本原样复用；现有不兼容版本失败关闭，不静默替换。
6. 缺失工具只按参考中的受管来源安装并复探。门禁不安装新的系统包管理器，不使用未经校验的下载，不追逐 `latest` tag。
7. 仅当真实失败命令是已批准的 macOS→Windows x64 NSIS 构建，且诊断指向交叉工具时，运行 `scripts/macos-tauri-xwin-gates.sh --install-missing`。其成功只证明交叉工具可用，不证明 Windows 运行时。
8. 非初始化恢复成功后只重试原失败命令一次；仍失败则停止并报告两个结果，不循环安装或扩大范围。
9. 返回触发类型、原命令/退出状态的脱敏摘要、宿主、`gate.git.*`、`gate.rust.*`、`gate.node.*`、`gate.pnpm.*`、安装变更、复探、重试和未验证平台。普通开发不预建 Verification。

## 持久不变量

- 本 Skill 在下游初始化后保留；
- 常规脚本没有界面选择参数，四项工具始终是 GUI 初始化门禁；
- 环境通过不是测试、构建、候选或验收通过。
