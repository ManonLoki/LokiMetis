# 开发环境门禁

只在中性 GUI 初始化或真实命令已经暴露受管环境故障时使用。

## 入口

- macOS/Linux：`scripts/development-environment-gates.sh --install-missing`；只读为 `--check-only`。
- Windows：`scripts/development-environment-gates.ps1`；只读为 `-CheckOnly`。
- 常规脚本不接收界面参数。退出码 `20` 表示至少一个必需工具缺失。
- macOS→Windows x64 NSIS 的真实构建失败才可调用 `scripts/macos-tauri-xwin-gates.sh --install-missing --target x86_64-pc-windows-msvc`。

## 常规探测矩阵

| 工具 | 要求 | 缺失时行为 |
|---|---|---|
| Git | 稳定版 `>=2.0.0` | macOS 使用既有 Homebrew，Linux 使用既有受支持系统包管理器，Windows 使用既有 winget `Git.Git`；安装后复探。 |
| Rust | stable 且满足项目 MSRV | 使用官方 rustup 制品与相邻 SHA-256，最小 profile 安装并复探。 |
| Node.js | `^24.15.0 || >=26.0.0` | 从官方倒序稳定版索引选择兼容版本，验证 `SHASUMS256.txt` 后安装并复探。 |
| pnpm | 稳定版 `>=11.24.0` | 通过 Node.js 附带 npm 按 `pnpm@>=11.24.0` 安装到用户级前缀并复探。 |
| MSVC Build Tools | Windows 必需 | 验证 Microsoft Authenticode 签名后安装 C++ 工作负载并复探。 |

现有版本不兼容时停止；不得把它当作缺失而替换。Node.js 25.x 不在兼容范围。所有 GUI 项目都检查 Node.js 与 pnpm。

## 交叉构建探测

macOS→Windows x64 NSIS 路径要求 LLVM/LLD、NSIS、`x86_64-pc-windows-msvc` target 与稳定版 `cargo-xwin >=0.23.1, <0.24.0`。只使用既有 Homebrew、rustup 和 Cargo；缺失 Homebrew 或现有工具损坏/越界时失败关闭。

## 安全与结果

- 只允许 HTTPS 官方来源或隔离测试显式开启的本地镜像，所有下载先校验摘要或签名；
- 不安装新的包管理器，不静默升级现有工具，不记录敏感路径或令牌；
- 成功后只把 `gate.path.prepend` 用于当前失败命令的单次重试；
- 记录宿主、工具要求、观测版本、安装来源、复探、重试与未验证平台。环境结果不替代测试、构建、E2E 或人工复核。
