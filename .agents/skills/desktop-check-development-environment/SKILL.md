---
name: desktop-check-development-environment
description: 仅在真实测试/构建命令已因受管环境问题失败时检查并安装缺失工具、升级低于最低门槛的工具；不得按任务、会话、显式构建或环境证据状态例行执行。
---

# 检查开发环境

观察到真实环境错误后恢复开发工具链：缺失时安装，已确认低于最低下界时按当前宿主的受支持标准路线升级，不依赖生成下游项目后会被删除的初始化 Skills。

Linux GPUI GNU x64/ARM64 专项入口 `scripts/linux-gpui-dependencies.sh` 仅在用户明确要求该工具链或真实相关失败后调用，默认检查，`--install` 才写入，不静默提权。系统阶段与原用户 Rust target 接续、旧进程 PATH 和下载诊断见 [Linux 专项恢复](references/development-environment-gates.md#linux-gpui-专项恢复)。

## GUI 框架范围

接口含 GUI 时消费根 Cargo metadata 的 `gui-framework`；新初始化必须显式保存 `tauri` 或 `gpui`，旧 GUI 缺省为 `tauri`，非法值失败关闭。POSIX 门禁传 `--gui-framework gpui`，Windows 传 `-GuiFramework gpui`；省略参数保持旧 Tauri 行为。Tauri 与 GPUI 共用 Node.js/npm 和 pnpm 门禁，供发布、验证等 Node 工程脚本使用；两者都要求 Node.js 稳定版 `>=24.21.0`、pnpm 稳定版 `>=12.4.1`。WebView、Tauri 系统库与 xwin 规则仍只适用于 Tauri。通用 POSIX helper 不检查或安装 Linux WebKitGTK/GTK；其真实缺包错误按 [Linux Tauri 系统库人工接续](references/development-environment-gates.md#linux-tauri-系统库人工接续) 人工接续，不得借用 Linux GPUI 安装脚本或把通用门禁通过视为库已恢复。GPUI 同时要求 Git、Rust 与当前原生编译宿主能力，其平台依赖及真实编译检查见 `$desktop-add-gpui-adapter`，不得探测或安装 Tauri 的 WebKit/WebView 依赖，也不进入 Tauri xwin 路线。

## 工作流程

本项目Linux GNU x64/ARM64的编译器、目标开发库与打包工具使用[Linux工程知识库](../../../docs/LINUX_GPUI_TOOLCHAIN.md)及`scripts/linux-gpui-dependencies.sh`。仅明确要求安装/配置该工具链或真实相关构建失败时进入；它不安装新的包管理器，不扩展为日常例行检查。交互权限受阻时可使用既有pkexec显示系统Polkit认证；不注入密码、不修改sudo策略。通用Node/Rust门禁保持下文来源与校验合同。

1. 读取 `AGENTS.md`、Agent Policy、`docs/RUST_CLI_TEMPLATE.md` 和已选接口记录；只在发布、完整验收、长期审计或用户明确要求持久证据时读取适用 Verification。既有环境证据可以提供诊断上下文，但其缺失、过期或指纹变化绝不是调用本 Skill 的触发器。
2. 只接受本次真实测试/构建命令已经失败的触发，且保存的命令、退出状态和脱敏诊断明确指向门禁管理的工具链、目标或系统依赖缺失/不兼容。不得仅因首次修改代码、新任务、新会话、显式构建、缺少环境证据或工具链版本可能变化而增加探测。
3. 非初始化触发必须先排除代码编译错误、测试断言失败、普通依赖解析/网络失败、产品配置错误、凭据/签名失败和其他非环境原因。确认属于受管环境错误后，读取 [references/development-environment-gates.md](references/development-environment-gates.md)，选择与失败命令对应的常规或 xwin 门禁；不得为了“顺便检查”运行无关门禁。
4. 常规门禁在 macOS/Linux 上运行 `scripts/development-environment-gates.sh --install-missing --interfaces <comma-separated-selection>`，在 Windows 上运行 `scripts/development-environment-gates.ps1 -Interfaces <selection>`。`--install-missing` 是保留的兼容入口名：写入模式同时安装缺失适用项、升级可证明低于最低下界的适用项；`--check-only` / `-CheckOnly` 保持零写入，并将后者报告为 `upgrade-required`。只因已观察到的常规工具链错误进入该门禁；本项目必须同时传 GUI 与 GPUI 框架参数。
5. Git 与 Rust 始终是常规阻断门禁；Git 要求稳定版 `>=2.36.0`，以覆盖 Git 生命周期和并行任务使用的 `git worktree list --porcelain -z`。Rust 没有 LTS 通道，本项目MSRV按根Cargo使用 `1.98.1`，不将该门槛称为当前最新稳定版；必须同时探测 `rustup`、`rustc`、`cargo` 与 `rustc -vV`，要求 rustc/cargo 属于同一 stable minor 且版本 `>=1.98.1`，任何更高稳定版直接通过。范围内工具原样复用，缺失时安装，已确认低于下界时升级并将变化报告为 `upgraded`。Rust 只使用带摘要校验的官方 rustup 制品，采用 rustup 的标准当前用户安装；默认使用 `~/.cargo`/`~/.rustup`，用户已设置的标准 `CARGO_HOME`/`RUSTUP_HOME` 只有在其值为用户主目录内的绝对安全路径、且可由新 login shell 或 Windows User 作用域持久恢复并与当前进程一致时才必须尊重，一次性进程值不能决定安装目标。`rustup-init` 必须传 `--no-modify-path`，由门禁在完整预检后把标准 Cargo bin 纳入普通用户 PATH；该参数只阻止安装器绕过事务改写 profile/注册表，不改变标准安装根，也不建立私有环境。不创建 Harness 私有工具环境变量，不得强制改写为 Harness 私有根或要求项目专用工具链。Git 在 macOS/Linux 只使用既有受支持系统包管理器，Windows 只使用既有 `winget` 的 `Git.Git`；Git 和 Windows MSVC 仅在这些平台原生受信管理器明确要求时允许系统级/管理员边界，必须显式显示提权需求且不得静默提权，没有既有权限路线就停止。门禁不安装新的包管理器，也不得降低项目门槛、回退依赖、注入 shim 或寻找替代工具链来迁就旧环境。预发布、无法解析或损坏的现有工具不属于“可证明低于下界”，必须失败关闭。
6. Node.js 是所有下游 Harness helper 的通用工程运行时，始终作为常规阻断门禁，并同时要求 npm 可解析和参与复探；只有已记录的接口选择包含 `GUI` 时才要求 pnpm。Node.js 稳定版必须满足 `>=24.21.0`，pnpm 稳定版必须满足 `>=12.4.1`；已安装的 25.x、26.x 或任何更高正式版本直接通过，不得为了贴合 LTS 降级或重装。Node.js 低于 24.21.0 时升级，GUI 项目的 pnpm 低于 12.4.1 时升级。缺失或需升级的 Node.js 从官方索引识别当前最高 LTS 系列，再选择该系列的最新稳定补丁，校验归档后把所选精确版本与摘要写入安全 marker，并在复用、当前进程复探和新 shell 复探中锁定同一版本；pnpm 只从 `https://registry.npmjs.org/` 以 `pnpm@>=12.4.1` 让 npm 解析当前稳定版。两者均不得改用更旧“兼容版”、强制把更高稳定版降到 LTS、使用 `latest` tag 或选择预发布版。不含 GUI 的项目仍必须探测、安装或升级 Node.js/npm，但把 pnpm 报告为 `not-required` 且不得探测、安装、升级或添加 pnpm。
7. Rust、Node.js 与 pnpm 使用操作系统惯例下的当前用户标准全局根，不建立 Harness 私有持久环境或专用前缀：Rust 使用有效的标准 `CARGO_HOME`/`RUSTUP_HOME` 或默认 `~/.cargo`/`~/.rustup`，Unix Node.js 使用 `~/.local/lib/nodejs/<version>` 并经 `~/.local/bin` 暴露命令，Unix pnpm 使用 npm 的用户级 `~/.local` 前缀；Windows Node.js 使用 `%LOCALAPPDATA%\Programs\nodejs\<version>`，pnpm 使用 npm 的 `%APPDATA%\npm` 用户全局前缀。写入前必须拒绝任何夹带平台 PATH 分隔符的单一路径根，并逐级确认安装根、profile/config/fish 路径都是普通非 symlink/reparse 目录或文件，marker 正确，且稳定链接的最终目标只有位于相应标准根时才可替换，任何冲突都在下载/安装前停止。Unix profile 只通过同目录随机临时文件、原字节快照比较和原子替换维护，不覆盖并发写入；发现旧版 Harness 私有 env source 精确行时，只安全移除该行并保留其他内容及旧文件。安装后先在当前进程完整复探；工具变化再持久写入用户范围 PATH，而包括仅 Git 改变在内的任何工具变化都必须启动新会话：Windows 全新 PowerShell 只从持久 User/Machine PATH，macOS/Linux 全新登录 shell 从持久 profile/系统 PATH，解析同一路径、版本并实际执行 Git、Rust 与适用 Node/npm/pnpm。两端都必须删除空段、所有相对/cwd 段与重复项；POSIX 解析还必须保持绝对字面 glob 不展开，且不覆盖既有非受管文件。
8. 初始化完成后，只有失败命令本身属于 macOS 上已批准的 Windows x64 NSIS 交叉构建，且诊断指向该路径的受管工具时，才运行 `scripts/macos-tauri-xwin-gates.sh --install-missing`。该门禁检查并按需安装缺失的 LLVM/LLD、NSIS、`x86_64-pc-windows-msvc` Rust target 与 cargo-xwin，或升级低于 0.23.1 的可解析稳定 cargo-xwin；其允许范围固定为 `>=0.23.1, <0.24.0`。普通 GUI 开发、尚未失败的构建或仅缺少 xwin 环境证据不得触发这些发布专用安装或升级。
9. xwin 门禁只能使用既有 Homebrew、rustup、Cargo 和 pnpm。它不自动安装 Homebrew；既有 `cargo-xwin` 必须为满足 `>=0.23.1, <0.24.0` 的稳定版本，低于 0.23.1 的可解析稳定版在写入模式下升级并复探；`>=0.24.0`、预发布、无法解析或残缺工具仍失败关闭，不得通过降级、shim 或其他兼容方案继续。门禁不接受非 macOS 宿主或其他 Windows target，安装或升级后必须逐项复探，并把 `gate.path.prepend` 仅用于重试当前失败的 Tauri 构建命令。
10. 通用受管工具为Git、Rust、Node.js/npm、pnpm；Linux已批准的GNU编译器、目标开发库、binutils、tar与dpkg工具由专用入口管理，Tauri xwin路线不适用。未经明确请求不全局安装；不得引入无关解释器、语言运行时、包管理器或虚拟环境作为补救。Harness helper缺失运行时只按第6步恢复Node.js。
10. Windows 自动安装或升级仍优先；如提权、UAC、组织策略或当前宿主能力使适用门禁不能完成，按 [Windows 自动安装受阻后的人工接续](references/development-environment-gates.md#windows-自动安装受阻后的人工接续) 提供本次已证实缺失项的官方入口和可复探诊断，等待用户自行安装并告知继续。不得用静默参数绕过管理员授权。用户告知完成后先在原宿主只读复探，不凭转述判定通过。门禁成功后只重试原失败命令一次；重试成功才继续原任务，重试仍失败则保留两个结果并停止。错误不在门禁支持范围内时报告精确阻断，不得临时拼装安装/升级命令或把非环境失败改判为环境问题。
11. 把触发类型、原失败命令/退出状态的脱敏摘要、已选接口、宿主、`gate.git.*` 在内的观测版本、`missing` / `upgrade-required` 判定、`installed` / `upgraded` 变更、复探、单次重试结果和未验证平台返回当前任务输出；只有发布、完整验收、长期审计或用户明确要求持久环境证据时才按 `docs/VERIFICATION.md` 写入日期证据卷。不得为普通开发预建 Verification，也不得记录不必要的用户主目录路径或敏感信息。
12. 必需门禁受阻时必须停止开发/构建任务。人工安装等待期间保留已确认的表单、项目根、接口选择和失败诊断；恢复时复用这些事实，不重新初始化或扩大安装范围。门禁成功只授权单次重试，不构成构建、测试、产物、验收或人工复核证据。测试所需的探测路径、下载镜像、安装根、registry 或安全校验覆盖只有在同一进程显式设置 `AFH_TEST_MODE=1` 时才允许；生产运行不得静默接受这些覆盖。

## 持久不变量

- 本 Skill 及其脚本在下游初始化后必须保留。
- 初始化入口已删除；`AGENTS.md` 仍必须在真实测试/构建命令已因受管环境错误失败时路由到本 Skill，并禁止基于构建类型或证据状态预先调用。
- 环境结果与当前宿主及已选接口指纹绑定；不得把该结果推断到未经检查的 Windows、macOS 或 Linux 宿主。
- xwin 发布能力结果还与 `x86_64-pc-windows-msvc` 目标绑定，且只证明交叉工具可用，不证明 Windows 运行时。

## 完成输出

报告触发类型、原失败命令与退出状态的脱敏摘要（初始化时为 `not-applicable`）、已选接口/发布能力、目标、Git/Rust 等必需和 `not-required` 工具、观测版本、`upgrade-required`、自动安装/升级、复探与单次重试结果、阻断失败，以及未验证平台。
