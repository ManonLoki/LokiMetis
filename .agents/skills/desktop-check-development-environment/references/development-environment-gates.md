# 开发环境门禁

只在中性初始化主动门禁，或初始化后真实测试/构建命令已经因受管环境问题失败时使用本参考。它独立于初始化流程，并且在仅用于初始化的 Skills 和文档被删除后继续保留；显式构建、缺少环境证据、新任务或新会话本身都不得触发它。

## 触发与恢复闭环

- 初始化阶段在写入脚手架前对固定 GUI 工具链运行一次常规门禁，允许安装缺失的适用工具，并自动升级可证明低于最低下界的已有工具。
- 初始化完成后先运行真实测试或构建命令。只有保存的命令、退出状态与脱敏诊断明确指向本参考管理的工具链、目标或系统依赖时，才选择对应门禁；代码错误、测试断言、普通依赖解析/网络、产品配置、凭据或签名失败不适用。
- 门禁成功后只重试原失败命令一次。重试仍失败时停止并同时报告原错误与恢复结果，不循环安装/升级、不扩大到无关门禁。
- macOS→Windows x64 专用门禁只在实际失败命令就是已批准的 xwin 构建且错误属于该工具集合时运行；不得仅因选择了该目标或缺少历史证据提前安装/升级。

## 可执行入口

- macOS/Linux：`scripts/development-environment-gates.sh --install-missing`。保留的 `--install-missing` 入口名同时处理缺失安装和低于下界升级。
- Windows PowerShell 5.1 或更高版本：`scripts/development-environment-gates.ps1`；默认写入模式同样安装缺失项并升级低于下界的已有项。
- 仅在只读审计时使用 `--check-only` 或 `-CheckOnly`。该模式始终零写入：缺失项报告 `missing`，可证明低于最低下界的项报告 `upgrade-required`；退出码 `20` 表示至少一个必需工具处于这两类待恢复状态。
- 解析稳定的 `gate.<environment>.<field>=<value>` 行。Git、Rust、Node.js 与 pnpm 始终为 GUI-only 必需项，初始化完成输出必须复述 `gate.git.status/version/change`。Windows 还必须具备 MSVC Build Tools。
- macOS Tauri Windows x64 候选在常规 GUI 门禁成功后，额外运行 `scripts/macos-tauri-xwin-gates.sh --install-missing --target x86_64-pc-windows-msvc`。只读审计使用 `--check-only`；退出码 `20` 表示缺失或需要升级，`30` 表示非 macOS。

## 探测矩阵

| 环境 | 适用条件 | 探测命令 | 缺失或低于下界时行为 |
|---|---|---|---|
| Git | 始终 | `git --version` | 要求稳定版 `>=2.0.0`；缺失时安装，低于 2.0.0 时升级。macOS 使用既有 Homebrew，Linux 使用既有受支持系统包管理器，Windows 使用既有 winget 的 `Git.Git`，然后重新探测。 |
| Rust | 始终 | `rustup --version`、`rustc --version`、`cargo --version`、`rustc -vV` | rustc/cargo 必须属于同一 stable minor 且都达到 MSRV；缺失时从已验证的官方 rustup 制品安装稳定版 Rust，可证明低于 MSRV 时升级 stable，然后重复全部探测。 |
| MSVC Build Tools | Windows Rust 目标 | `cl`，随后使用 `vswhere` 查找 VC 工具组件 | 安装 Microsoft 已签名的 Visual Studio Build Tools C++ 工作负载，然后重新探测。 |
| Node.js | GUI-only | `node --version`、`npm --version` | 要求 `^24.15.0 || >=26.0.0` 且 npm 可解析；缺失、低于 24.15.0 或处于 25.x 时，从官方倒序索引选择第一个满足门禁的当前稳定版，验证宿主归档校验和后安装或升级并重新探测。25.x 按低于下一段允许下界 26.0.0 处理。 |
| pnpm | GUI-only | `pnpm --version` | 要求 `>=11.24.0`；缺失或低于下界时使用 Node.js 的 npm 客户端，从官方 npm 软件包仓库以 `pnpm@>=11.24.0` 解析当前稳定版并安装或升级到用户级前缀目录，然后重新探测。 |
| LLVM/LLD | macOS Tauri→Windows x64 | `llvm-rc`、`lld-link` | 使用既有 Homebrew 分别安装 `llvm` 与 `lld`，把两个 formula 的 `bin` 目录加入当前构建 PATH，然后复探；兼容 Homebrew 将 LLD 从 LLVM 拆包。 |
| NSIS | macOS Tauri→Windows x64 | `makensis` | 使用既有 Homebrew 安装 `nsis`，然后复探；macOS 不支持生成 WiX/MSI。 |
| Windows Rust target | macOS Tauri→Windows x64 | `rustup target list --installed` | 运行 `rustup target add x86_64-pc-windows-msvc` 并复探。 |
| cargo-xwin | macOS Tauri→Windows x64 | `cargo-xwin --version` | 要求稳定版 `>=0.23.1, <0.24.0`；缺失或可解析稳定版低于 0.23.1 时运行 `cargo install --locked --version '>=0.23.1, <0.24.0' cargo-xwin` 安装或升级并复探；`>=0.24.0`、预发布、无法解析或损坏时阻断。 |

现有 Git 稳定版低于 2.0.0、Rust 稳定版低于 MSRV、Node.js 低于 24.15.0 或处于 25.x、pnpm 稳定版低于 11.24.0，以及 `cargo-xwin` 稳定版低于 0.23.1，都是可恢复的 `upgrade-required`：写入模式必须走当前宿主受管路线升级并复探，不得把它们降级成警告。现有范围内稳定版本原样复用。`cargo-xwin >=0.24.0`、任何预发布、无法解析或损坏的工具不属于“低于最低下界”的自动升级路径，必须失败关闭。仅检测到工具存在不证明门禁已满足，更不证明单元测试或构建已经通过。

## 安装安全措施

- Git 只使用宿主既有受管包管理器安装或升级：macOS 的 Homebrew、Linux 的 `apt-get`/`dnf`/`yum`/`zypper`/`apk`/`pacman`，或 Windows 的 winget `Git.Git`。只有这些平台原生受信管理器明确要求时才允许系统级/管理员安装，必须显式显示权限边界且不得静默提权；没有既有 `sudo`/管理员上下文时阻断。门禁不得下载并执行临时 Git 安装脚本或安装新的包管理器；结束后必须先在同一进程、再在全新登录 shell/PowerShell 中解析同一路径并执行同一 `git --version`，即使本轮只有 Git 改变也不能跳过。
- Rust 使用来自 `https://static.rust-lang.org/rustup/dist`、与架构/libc 匹配的 `rustup-init`，验证相邻的 SHA-256 文件，以 `--no-modify-path` 安装最小 stable 配置档；固定使用当前用户受管 `.cargo`/`.rustup` 根并忽略继承的 `CARGO_HOME`/`RUSTUP_HOME`。rustc/cargo 必须属于同一 stable minor 且都达到 MSRV，随后重复全部探测。
- Node.js 使用 `https://nodejs.org/dist/index.tab` 的倒序稳定版索引，选择第一个满足 `^24.15.0 || >=26.0.0` 的版本，再验证该版本归档的 `SHASUMS256.txt`，把精确版本与归档摘要写入受管 marker 后安装到用户级目录。既有同名版本目录只有 marker 与本次已校验归档一致、可执行最终目标仍在受管根内且 `node --version` 精确等于所选版本时才可复用；最终复探仍锁定该精确版本。不得固定安装某个旧 major 或向下寻找兼容版本。
- pnpm 在 GUI-only 项目中通过 Node.js 附带的 npm 客户端安装或升级 `pnpm@>=11.24.0`，并把 registry 固定为 `https://registry.npmjs.org/`，由官方仓库解析当前稳定版。npm 必须验证软件包仓库完整性元数据；门禁绝不得执行下载的文本、禁用 TLS、使用 `latest` tag、选择预发布版 pnpm 或回退到更旧依赖。
- Rust、Node.js 与 pnpm 固定安装在当前用户受管全局根并同时接入当前复探 PATH 与持久 User PATH。写入前逐级确认安装根、profile/config/env/fish 都是普通非 symlink/reparse 路径，受管文件首行 marker 正确；稳定用户链接仅可替换最终目标仍在同一受管根内的既有链接，冲突必须零下载、零安装失败。Unix profile 使用同目录随机临时文件、原字节快照比较和原子替换，检测到并发变化即保留原文件并停止。Windows 新 PowerShell 只能从持久 User/Machine PATH，macOS/Linux 新登录 shell 只能从持久 profile/系统 PATH，解析同一路径和版本并实际执行 Git、Rust 与 Node/npm/pnpm；该验证覆盖任何工具变化而非仅受管工具变化。两端都丢弃空段、所有相对/cwd 段与重复项；POSIX 解析还保持绝对字面 glob 不展开。
- `AFH_PREREQ_PATH`、测试下载镜像、测试安装根、测试 registry、跳过持久化或安全校验等危险覆盖只能用于隔离回归，并且必须在同一进程显式设置 `AFH_TEST_MODE=1`；否则门禁在运行任何受管工具前失败关闭。生产 pnpm registry 不接受覆盖。
- Windows MSVC 使用 `https://aka.ms/vs/17/release/vs_BuildTools.exe`，要求有效的 Microsoft Authenticode 签名；仅在微软安装器明确要求的管理员边界内安装 `Microsoft.VisualStudio.Workload.VCTools`，不得静默获取或绕过权限，然后重新探测。
- Tauri xwin 只在 macOS 且 Windows x64 NSIS 已批准时触发。LLVM、LLD 与 NSIS 使用既有 Homebrew，但只作为宿主能力探测，不把 formula 当前解析版本写成项目兼容契约；Rust target 使用 rustup，`cargo-xwin` 使用 Cargo 的 `--locked --version '>=0.23.1, <0.24.0'` 安装或升级。缺少 Homebrew 时阻断并报告，不执行远程 shell 安装器。已存在但缺失必需命令的 formula 视为损坏并阻断；低于 0.23.1 的可解析稳定 `cargo-xwin` 升级，`>=0.24.0`、预发布、无法解析或损坏的 `cargo-xwin` 阻断。
- 只能在本参考规定的触发条件与写入模式下升级现有工具。不得降低最低门禁、回退依赖或锁文件、注入 shim、改用旧版工具或寻找替代工具链来适配旧环境。安装程序、校验和、签名、提权、重启、策略、链接器、软件包仓库完整性或安装/升级后探测发生失败时，必须阻断开发。

## 证据记录

记录宿主、固定 GUI/发布目标、工具需求状态、探测命令、观测版本或 `Missing`、`missing` / `upgrade-required`、安装/升级来源和 `installed` / `upgraded` 变更、写入后复探、最终结果，并将其他平台标记为 `Unverified`。xwin 成功仍把 Windows runtime 记为 `Unverified`。遮盖令牌和不必要的用户主目录路径。该证据不得替代项目构建、测试、产物、冒烟、E2E 或人工复核证据。
