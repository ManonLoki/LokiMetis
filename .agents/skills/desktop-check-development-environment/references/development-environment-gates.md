# 开发环境门禁

只在中性初始化主动门禁，或初始化后真实测试/构建命令已经因受管环境问题失败时使用本参考。它独立于初始化流程，并且在仅用于初始化的 Skills 和文档被删除后继续保留；显式构建、缺少环境证据、新任务或新会话本身都不得触发它。

GUI 调用方必须从已确认表单或 Cargo `gui-framework` 分派：Tauri 使用默认参数，GPUI 使用 `--gui-framework gpui`（Windows：`-GuiFramework gpui`）。参数闭集为 `tauri|gpui`，未知值在工具探测前失败。Tauri 与 GPUI 都要求 Node.js/npm 及 pnpm，分别遵守 Node.js 稳定版 `>=24.21.0`、pnpm 稳定版 `>=12.4.1`，供发布、验证等 Node 工程脚本使用。GPUI 不探测或安装 Tauri 的 WebKit/WebView 依赖，不进入 Tauri xwin 路线；具体原生平台库由 `$desktop-add-gpui-adapter` 的实际编译需求决定。

## 触发与恢复闭环

- 初始化阶段在写入脚手架前对已选接口运行一次常规门禁，允许安装缺失的适用工具，并自动升级可证明低于最低下界的已有工具。
- 初始化完成后先运行真实测试或构建命令。只有保存的命令、退出状态与脱敏诊断明确指向本参考管理的工具链、目标或系统依赖时，才选择对应门禁；代码错误、测试断言、普通依赖解析/网络、产品配置、凭据或签名失败不适用。
- 门禁成功后只重试原失败命令一次。重试仍失败时停止并同时报告原错误与恢复结果，不循环安装/升级、不扩大到无关门禁。
- macOS→Windows x64 专用门禁只在实际失败命令就是已批准的 xwin 构建且错误属于该工具集合时运行；不得仅因选择了该目标或缺少历史证据提前安装/升级。

## 可执行入口

- macOS/Linux：`scripts/development-environment-gates.sh --install-missing --interfaces <comma-separated-selection>`。保留的 `--install-missing` 入口名同时处理缺失安装和低于下界升级。
- Windows PowerShell 5.1 或更高版本：`scripts/development-environment-gates.ps1 -Interfaces <selection>`；默认写入模式同样安装缺失项并升级低于下界的已有项。
- 仅在只读审计时使用 `--check-only` 或 `-CheckOnly`。该模式始终零写入：缺失项报告 `missing`，可证明低于最低下界的项报告 `upgrade-required`；退出码 `20` 表示至少一个必需工具处于这两类待恢复状态。
- 解析稳定的 `gate.<environment>.<field>=<value>` 行。Git、Rust 与 Node.js/npm 始终为必需项，初始化完成输出必须复述 `gate.git.status/version/change`。Windows 还必须具备 MSVC Build Tools。仅当选择包含 `GUI` 时才需要 pnpm；否则 pnpm 必须为 `not-required`，Node.js/npm 仍必须通过。
- macOS Tauri Windows x64 候选在常规 GUI 门禁成功后，额外运行 `scripts/macos-tauri-xwin-gates.sh --install-missing --target x86_64-pc-windows-msvc`。只读审计使用 `--check-only`；退出码 `20` 表示缺失或需要升级，`30` 表示非 macOS。

## 探测矩阵

| 环境 | 适用条件 | 探测命令 | 缺失或低于下界时行为 |
|---|---|---|---|
| Git | 始终 | `git --version` | 要求稳定版 `>=2.36.0`，以覆盖 Git 生命周期和并行任务使用的 `git worktree list --porcelain -z`；缺失时安装，低于 2.36.0 时升级。macOS 使用既有 Homebrew，Linux 使用既有受支持系统包管理器，Windows 使用既有 winget 的 `Git.Git`，然后重新探测。 |
| Rust | 始终 | `rustup --version`、`rustc --version`、`cargo --version`、`rustc -vV` | Rust 没有 LTS 通道；截至 2026-09-12 以当前稳定版 `1.98.1` 作为 MSRV。rustc/cargo 必须属于同一 stable minor 且版本 `>=1.98.1`；任何更高稳定版直接通过。缺失时从已验证的官方 rustup 制品安装稳定版 Rust，可证明低于 MSRV 时升级 stable，然后重复全部探测。 |
| MSVC Build Tools | Windows Rust 目标 | `cl`，随后使用 `vswhere` 查找 VC 工具组件 | 安装 Microsoft 已签名的 Visual Studio Build Tools C++ 工作负载，然后重新探测。 |
| Node.js | 所有下游接口 | `node --version`、`npm --version` | 作为 Harness helper 的通用工程运行时，要求正式稳定版 `>=24.21.0` 且 npm 可解析；已装 25.x、26.x 或任何更高正式版本直接通过。缺失或低于下界时，从官方索引识别最高 LTS 系列并选择该系列的最新稳定补丁，验证宿主归档校验和后安装或升级并重新探测。 |
| pnpm | 已选择 `GUI` | `pnpm --version` | 要求稳定版 `>=12.4.1`；缺失或低于下界时使用 Node.js 的 npm 客户端，从官方 npm 软件包仓库以 `pnpm@>=12.4.1` 解析当前稳定版并安装或升级到标准用户级全局前缀，然后重新探测。 |
| LLVM/LLD | macOS Tauri→Windows x64 | `llvm-rc`、`lld-link` | 使用既有 Homebrew 分别安装 `llvm` 与 `lld`，把两个 formula 的 `bin` 目录加入当前构建 PATH，然后复探；兼容 Homebrew 将 LLD 从 LLVM 拆包。 |
| NSIS | macOS Tauri→Windows x64 | `makensis` | 使用既有 Homebrew 安装 `nsis`，然后复探；macOS 不支持生成 WiX/MSI。 |
| Windows Rust target | macOS Tauri→Windows x64 | `rustup target list --installed` | 运行 `rustup target add x86_64-pc-windows-msvc` 并复探。 |
| cargo-xwin | macOS Tauri→Windows x64 | `cargo-xwin --version` | 要求稳定版 `>=0.23.1, <0.24.0`；缺失或可解析稳定版低于 0.23.1 时运行 `cargo install --locked --version '>=0.23.1, <0.24.0' cargo-xwin` 安装或升级并复探；`>=0.24.0`、预发布、无法解析或损坏时阻断。 |

现有 Git 稳定版低于 2.36.0、Rust 稳定版低于 1.98.1、Node.js 稳定版低于 24.21.0、pnpm 稳定版低于 12.4.1，以及 `cargo-xwin` 稳定版低于 0.23.1，都是可恢复的 `upgrade-required`：写入模式必须走当前宿主受支持路线升级并复探，不得把它们降级成警告。现有范围内稳定版本原样复用，尤其不得把 Node.js 25.x、26.x 或未来更高正式稳定版本降级到 LTS。`cargo-xwin >=0.24.0`、任何预发布、无法解析或损坏的工具不属于“低于最低下界”的自动升级路径，必须失败关闭。仅检测到工具存在不证明门禁已满足，更不证明单元测试或构建已经通过。

## 安装安全措施

- Git 只使用宿主既有受管包管理器安装或升级：macOS 的 Homebrew、Linux 的 `apt-get`/`dnf`/`yum`/`zypper`/`apk`/`pacman`，或 Windows 的 winget `Git.Git`。只有这些平台原生受信管理器明确要求时才允许系统级/管理员安装，必须显式显示权限边界且不得静默提权；没有既有 `sudo`/管理员上下文时阻断。门禁不得下载并执行临时 Git 安装脚本或安装新的包管理器；结束后必须先在同一进程、再在全新登录 shell/PowerShell 中解析同一路径并执行同一 `git --version`，即使本轮只有 Git 改变也不能跳过。
- Rust 使用来自 `https://static.rust-lang.org/rustup/dist`、与架构/libc 匹配的 `rustup-init`，验证相邻的 SHA-256 文件并安装最小 stable 配置档。安装沿用 rustup 的标准当前用户布局：默认使用 `~/.cargo`/`~/.rustup`；非默认 `CARGO_HOME`/`RUSTUP_HOME` 只有在值为用户主目录内的绝对安全路径、且可由 Unix 新 login shell 或 Windows User 作用域持久恢复并与当前进程一致时才可使用，一次性进程值必须在下载前失败。两端的 `rustup-init` 都传 `--no-modify-path`，避免安装器在完整预检与原子写入之外修改 shell profile 或 User PATH；门禁随后把标准 Cargo bin 写入普通用户 PATH，该参数不改变安装根，也不建立 Harness 私有环境。不得覆盖为 Harness 私有根或建立项目专用工具链。rustc/cargo 必须属于同一 stable minor 且版本 `>=1.98.1`，随后重复全部探测。
- Node.js 使用 `https://nodejs.org/dist/index.tab` 识别最高 LTS 系列并选择该系列的最新稳定补丁，再验证该版本归档的 `SHASUMS256.txt`，把精确版本与归档摘要写入安全 marker 后安装到当前用户标准目录。Unix 使用 `~/.local/lib/nodejs/<version>` 并经 `~/.local/bin` 暴露命令；Windows 使用 `%LOCALAPPDATA%\Programs\nodejs\<version>`。既有同名版本目录只有 marker 与本次已校验归档一致、可执行最终目标仍在相应标准根内且 `node --version` 精确等于所选版本时才可复用；最终复探仍锁定该精确版本。安装选择 LTS 不改变连续门禁：既有任何 `>=24.21.0` 的正式稳定版都直接通过。
- pnpm 仅在 GUI 项目中通过 Node.js 附带的 npm 客户端安装或升级 `pnpm@>=12.4.1`，并把 registry 固定为 `https://registry.npmjs.org/`，由官方仓库解析当前稳定版。Unix 使用 npm 的 `~/.local` 用户全局前缀，Windows 使用 npm 的 `%APPDATA%\npm` 用户全局前缀；不得建立 Harness 私有 prefix。npm 必须验证软件包仓库完整性元数据；门禁绝不得执行下载的文本、禁用 TLS、使用 `latest` tag、选择预发布版 pnpm 或回退到更旧依赖。
- Rust、Node.js 与 pnpm 安装在操作系统惯例下的当前用户标准全局根并同时接入当前复探 PATH 与持久 User PATH，不建立 Harness 私有持久 env、prefix 或 PATH 文件。写入前拒绝单一路径根中夹带平台 PATH 分隔符，并逐级确认安装根、profile/config/fish 都是普通非 symlink/reparse 路径，marker 正确；稳定用户链接仅可替换最终目标仍在相应标准根内的既有链接，冲突必须零下载、零安装失败。Unix profile 使用同目录随机临时文件、原字节快照比较和原子替换，检测到并发变化即保留原文件并停止；发现旧版 Harness 私有 env source 精确行时，只安全移除该行，保留其他 profile 内容与旧 env 文件。Windows 新 PowerShell 只能从持久 User/Machine PATH，macOS/Linux 新登录 shell 只能从持久 profile/系统 PATH，解析同一路径和版本并实际执行 Git、Rust 与适用 Node/npm/pnpm；该验证覆盖任何工具变化而非仅受管工具变化。两端都丢弃空段、所有相对/cwd 段与重复项；POSIX 解析还保持绝对字面 glob 不展开。
- `AFH_PREREQ_PATH`、测试下载镜像、测试安装根、测试 registry、跳过持久化或安全校验等危险覆盖只能用于隔离回归，并且必须在同一进程显式设置 `AFH_TEST_MODE=1`；否则门禁在运行任何受管工具前失败关闭。生产 pnpm registry 不接受覆盖。
- Windows MSVC 使用 `https://aka.ms/vs/17/release/vs_BuildTools.exe`，要求有效的 Microsoft Authenticode 签名；仅在微软安装器明确要求的管理员边界内安装 `Microsoft.VisualStudio.Workload.VCTools`，不得静默获取或绕过权限，然后重新探测。
- Tauri xwin 只在 macOS、GUI 已选且 Windows x64 NSIS 已批准时触发。LLVM、LLD 与 NSIS 使用既有 Homebrew，但只作为宿主能力探测，不把 formula 当前解析版本写成项目兼容契约；Rust target 使用 rustup，`cargo-xwin` 使用 Cargo 的 `--locked --version '>=0.23.1, <0.24.0'` 安装或升级。缺少 Homebrew 时阻断并报告，不执行远程 shell 安装器。已存在但缺失必需命令的 formula 视为损坏并阻断；低于 0.23.1 的可解析稳定 `cargo-xwin` 升级，`>=0.24.0`、预发布、无法解析或损坏的 `cargo-xwin` 阻断。
- 只能在本参考规定的触发条件与写入模式下升级现有工具。不得降低最低门禁、回退清单依赖、注入 shim、改用旧版工具或寻找替代工具链来适配旧环境。安装程序、校验和、签名、提权、重启、策略、链接器、软件包仓库完整性或安装/升级后探测发生失败时，必须阻断开发。

## Windows 自动安装受阻后的人工接续

Windows 写入门禁先尝试现有静默路线：Git 使用 `winget --silent --disable-interactivity`，Rust、Node.js、pnpm 安装到当前用户环境，MSVC 使用验签后的微软 Build Tools 引导程序 `--quiet --wait --norestart`。静默表示安装界面最少，不代表可以绕过 UAC、管理员授权或组织策略；微软的 Visual Studio 安装要求管理员权限，标准用户不能以 `--quiet` 绕过。Agent 能在已授权的管理员上下文继续时，仍优先完成自动安装和复探。

若 Windows 宿主中的 Codex 进程无法完成提权、UAC 被拒绝、组织策略阻断或安装器不可由当前会话运行，先停止当前初始化/开发/构建，再在同一项目根与已选接口执行一次 `-CheckOnly -Interfaces <selection>` 只读复探。写入门禁可能在首次失败时提前退出，不能仅凭该退出码推断所有工具状态。复探可运行时，给用户一份**仅含本次已证实未达标必需项**的人工安装清单；复探本身无法运行时，只列已证实的失败项，并将其余适用项标为“未判定”，不得把它们写成已通过或已失败。清单写明原门禁命令、失败阶段和退出码或诊断、目标架构、已选择接口、最低版本、已成功安装项，以及以下相应官方入口；不得给未经核验的聚合包或把用户安装视为已经通过。

| 必需项 | 官方下载/安装入口 | 用户安装要点 |
|---|---|---|
| Git（全部接口） | [Git for Windows](https://git-scm.com/install/windows) | 选择当前 Windows 架构的安装器；安装后 `git --version` 至少为稳定版 2.36.0。已有 `winget` 且可用时仍优先由门禁自动静默安装。 |
| Rust（全部接口） | [Rust 官方安装页](https://rust-lang.org/tools/install/) | 下载当前架构的 `rustup-init.exe`，选择 stable 的 MSVC 工具链；`rustc --version` 至少为稳定版 1.98.1，同时确认 `cargo --version` 与同一 stable minor。 |
| MSVC C++ Build Tools（Windows Rust） | [微软 Build Tools 2022 引导程序](https://aka.ms/vs/17/release/vs_BuildTools.exe)、[微软安装说明](https://learn.microsoft.com/en-us/visualstudio/install/install-visual-studio?view=vs-2022) | 由用户或设备管理员完成 UAC；在安装器中选择 **Desktop development with C++**（`Microsoft.VisualStudio.Workload.VCTools`）及推荐组件，按提示重启。仅安装 Visual C++ Redistributable 不能替代编译工具。 |
| Node.js（所有下游接口） | [Node.js 官方下载页](https://nodejs.org/en/download) | 选择当前架构、满足 `>=24.21.0` 的稳定版；已有 25.x 或更高正式稳定版可继续使用。安装后核对 `node --version` 与 `npm --version`。 |
| pnpm（仅 GUI） | [pnpm 官方安装说明](https://pnpm.io/installation) | Node.js 合格后用 npm 在标准用户级全局前缀安装满足 `>=12.4.1` 的稳定版，并核对 `pnpm --version`；不要用远程文本直接执行或降低版本门槛。 |

用户完成安装并明确告知继续后，Agent 在同一项目根和已确认接口下先重新打开进程环境/刷新 PATH，再运行 `.agents/skills/desktop-check-development-environment/scripts/development-environment-gates.ps1 -CheckOnly -Interfaces <selection>` 只读复探。所有适用项均为 `passed` 才继续：初始化返回原初始化流程；真实测试/构建错误恢复只重试原失败命令一次。仍未达标时报告具体剩余项及新的诊断并保持阻断，不循环自动安装，也不将未验证的用户操作记为通过。若 Codex 在该 Windows 宿主完全无法运行，用户可在本机 PowerShell 核对上述版本并把结果告知 Agent；Agent 恢复访问后仍必须亲自执行只读门禁，不能仅凭转述宣称环境通过。

## 证据记录

记录宿主、已选接口/发布目标、工具需求状态、探测命令、观测版本或 `Missing`、`missing` / `upgrade-required`、安装/升级来源和 `installed` / `upgraded` 变更、写入后复探、最终结果，并将其他平台标记为 `Unverified`。xwin 成功仍把 Windows runtime 记为 `Unverified`。遮盖令牌和不必要的用户主目录路径。该证据不得替代项目构建、测试、产物、冒烟、E2E 或人工复核证据。
