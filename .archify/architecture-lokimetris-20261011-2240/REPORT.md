# LokiMetris 项目架构分析报告

- 分析对象:`/Users/manonloki/Documents/my-work/ai/loki_metris`,提交 `6917ba1`(分支 `main`,共 125 个提交)
- 架构图:[`lokimetris.html`](lokimetris.html),由 archify 生成。校验、交付、严格检查、浏览器检查四道门禁全部通过。
- 范围声明:只静态读源码、文档和 Cargo 元数据,没有运行应用。下文所有"现状"结论都出自读到的源码位置;没读到的部分标为"未核实"。

---

## 1. 一句话概括

LokiMetris(诡秘神谕)是一个 Windows/macOS 原生 Rust 桌面应用,UI 用 GPUI。它读取本机 AI Agent 的会话日志做 Token 用量看板,通过 Hook 接收 AI 工具事件驱动桌宠与监控页,并能给 Codex / WorkBuddy 桌面应用换肤。整个工程是历史项目 LokiMetis(Tauri + React)的原生复刻,体验复刻尚未完成。

## 2. 工程规模

| 项 | `loki_metris_core` | `loki_metris_gui` |
|---|---|---|
| Rust 文件数 | 173 | 482 |
| 代码行数 | 约 37,200 | 约 107,200 |
| 测试标注数(`#[test]` 等) | 457 | 761 |
| 测试文件数(文件名含 test) | 45 | 165 |
| `unsafe` 关键字出现次数 | 0 实际(原统计的 2 处是测试字符串里的单词) | 约 130(生产代码) |
| `target_os` 条件编译出现次数 | 38 | 293 |

其他事实:
- 最近一次全量 `cargo test --workspace`(依赖升级后)通过,GUI 主测试 1064 个,core 454 个,0 失败。
- 工作区 `vendor/` 共 15MB,含 6 个带补丁的 GPUI/accesskit crate(见第 7 节)。
- 没有发现超过 800 行的 Rust 文件,最大约 500 行,符合 `docs/ENGINEERING_RULES.md` 的行数门禁。

## 3. 总体架构

两个 workspace 成员加一组 vendored 依赖:

```
loki_metris_core   平台无关:领域规则、SQLite 索引、Hook 状态机、窗口/桌宠位置规则
loki_metris_gui    GPUI 原生壳 + 全部本机 I/O(文件、进程、CDP、托盘、通知)
vendor/            官方 gpui-pre* 0.3.8 + accesskit_macos 的补丁副本
```

`AGENTS.md` 规定 Core-first:宿主无关的业务规则、值域、权威状态和稳定错误归 core,GUI 只管展示、交互和原生机制。实际代码结构与之基本一致:core 只暴露 `lib.rs` 里整理过的 API,GUI 的 `backend/` 在 core 定义的端口(如 `LocalUsageScanner`,`client_ports.rs:157`)后面实现读取。

### 3.1 GUI 内部模块(按代码量)

| 模块 | 文件 | 行数 | 职责 |
|---|---|---|---|
| `app/` | 144 | 约 29.8k | GPUI 壳层:导航、仪表盘、监控、皮肤、设置各页 |
| `backend/` | 102 | 约 25.9k | 只读 Agent 数据源驱动:`local_index`、`workbuddy`、`scan_orchestration` |
| `skins/` | 72 | 约 20.4k | Codex/WorkBuddy 换肤:资源库、宿主进程、CDP 连接与注入 |
| `monitor/` | 48 | 约 11.7k | Hook 中继、环回监听、监控档案与图片资源 |
| `usage_service/` | 47 | 约 8.3k | 应用拥有的查询与单 writer 索引服务 |
| `pet/` | 33 | 约 5.9k | 桌宠窗口、位置、穿透、设置窗口 |
| `native/` | 25 | 约 4.2k | 托盘、通知、单实例、自启、菜单、窗口原生细节 |

### 3.2 core 内部模块

最大的是 `agent_hooks`(约 8.4k 行,按工具分文件:Claude Code、Codex、Cursor、Gemini CLI、Copilot、Grok、Kimi Code、Hermes 等)、`local_index`(约 6k 行,SQLite 索引)、`workbuddy_stats`(约 2.8k)、`policy`(约 2.1k)、`scan`(约 1.9k)、`aggregate`(约 1.9k)。其余是各视图的查询构建(调用/统计/图表/今日用量)、时间口径、桌宠位置规则等。

## 4. 启动与生命周期

入口 `loki_metris_gui/src/main.rs`,顺序如下(行号来自该文件):

1. `:27` 若命令行带 `--loki-metris-hook-relay`,直接以中继模式运行并退出,不启动 UI。
2. `:32` 取得单实例锁;若已有实例,通知它显示窗口后退出。
3. `:37` 初始化日志。
4. `:45` 读取偏好,拿到专用后台写入线程的句柄。
5. `:50` 创建 `UsageService`。它自建一个 2 工作线程的 tokio runtime,并拉起扫描队列、查询队列和通知监视器(`usage_service/mod.rs:101-137`)。
6. `:54` `start_monitoring()` 启动 Hook 环回监听,并把托管 Hook 配置写入各 AI 工具的配置(`usage_service/mod.rs:139-146`)。
7. `:61` `application.run` 进入 GPUI 事件循环,创建主窗口、托盘和桌宠。

退出由 `lifecycle::Shutdown` 统一回收偏好写入、日志、通知和 `UsageService`。

**设计要点:** 前台线程只处理窗口与展示,所有文件和网络 I/O 都在应用自己拥有的后台 runtime 或专用线程里。查询队列和扫描队列分开,所以"只读查询不取消、不等待后台扫描"(`usage_service/mod.rs` 开头的模块说明)。

## 5. 四条主要数据链路

### 5.1 用量统计链路

```
Agent 会话文件 --只读--> 扫描编排 --写入--> 派生 SQLite 索引 <--查询-- UsageService <-- GPUI 页面
```

- **来源:** Codex、Claude Code、Grok 读取各自本机会话;WorkBuddy 只读国内版 `~/.workbuddy` 与国际版 `~/.workbuddy-ai` 两个固定根下的 `projects/<project>/<session>.jsonl` 及一层 `subagents/*.jsonl`(`backend/workbuddy/mod.rs:1-8`)。
- **隐私约束:** WorkBuddy 适配器解析完整 JSON 行,但只投影 `providerData.usage`、模型、时间戳和积分,正文不进入返回值、索引或日志。
- **写入:** `backend/scan_orchestration/` 提供单 writer 的历史扫描编排,支持增量与重建,不绑定 GUI 或 IPC。
- **存储:** 产品自己的派生库,文件名 `usage-index.sqlite3`(`client_ports.rs:59`),经 `private_sqlite.rs` 打开并设置私有权限,用 sea-orm + sqlx(SQLite)访问。数据目录 macOS 为 `~/Library/Application Support/app-loki-metris/data`,Windows 为 `%LOCALAPPDATA%\app-loki-metris\data`,可用绝对路径环境变量 `LOKI_METRIS_DATA_DIR` 覆盖(`usage_service/mod.rs:101-110`)。
- **不碰原始数据:** 只读原始 Agent 目录,不复制历史 App 数据库,历史源码和已安装参考 App 都不是运行时依赖。
- **全部概览的完整性:** 联合视图里 WorkBuddy 读取失败时,界面明示数据不完整,而不是静默少算(`UsageData.workbuddy_unavailable`,`usage_service/mod.rs`)。

### 5.2 Hook 监控与桌宠链路

```
AI 工具 --执行 Hook 命令--> 本程序 --relay 子进程--> 环回 HTTP 监听 --> HookStateMachine --> 桌宠 / 监控页
```

- **配置写入:** 启动时 `hook_config_io` 把"托管条目"合并进各工具的配置文件(例如 Claude Code 的 `.claude/settings.json`,`core/agent_hooks/claude_code.rs:118-122`),并支持 WSL 配置集的对账(`hook_config_io.rs:116`)。
- **中继:** Hook 命令实际执行的是本程序自己,带 `--loki-metris-hook-relay`。它读取 stdin,归约成最小信封,经会合文件找到当前实例的监听端点,再可靠投递(`monitor/relay.rs`)。输入上限 `MAX_NATIVE_HOOK_INPUT_BYTES`。
- **监听:** 绑定环回地址的 TCP 监听,只接受受支持 AI 工具的最小信封(`monitor/listener/mod.rs`、`loopback_server.rs:35`)。会合文件由端点租约保护,避免多实例冲突(`loopback_server.rs:55`)。
- **状态机:** `listener/lifecycle.rs` 串行推进 core 的 `HookStateMachine`,把结果映射成桌宠展示状态。
- **取舍:** 用"自己调用自己"的方式做中继,省去单独的中继二进制,但意味着 Hook 配置里记录的是应用可执行文件路径,应用被移动或重装后需要重新对账。

### 5.3 换肤链路

```
UI 换肤页 -> skins 服务(资源库/校验) -> CDP 连接 -> Codex / WorkBuddy 桌面应用
```

- **规则归属:** 平台无关规则逐步委托 core(`codex_skin.rs` 里有 ID、批量大小、CSS/图片大小上限、代码执行同意校验);`skins/` 只持有文件系统、进程和 CDP 适配(`skins/mod.rs:1-3`)。
- **宿主连接:** 通过 chromiumoxide 走 Chrome DevTools Protocol,连接宿主应用的调试端口(`skins/cdp_connect.rs`)。macOS 和 Windows 各有平台专属的进程、身份与启动代码(`macos_codex.rs`、`macos_identity.rs`、`windows_codex/`)。
- **资源:** 正式运行时的皮肤资源来自安装包(macOS 为 `Contents/Resources/skins`,Windows 为安装根 `resources/skins`),缺失时显示错误,release 二进制忽略开发资源变量。
- **风险面:** 这是整个项目对外部进程影响最大的一块。图中用"security"样式标出了 CDP 注入这条边。

### 5.4 通知与托盘

`native/` 持有托盘、系统通知、单实例锁、自启和菜单,由唯一的 `Services` 全局持有,"不增加第二条事件循环"(`native/mod.rs`)。托盘事件经有界轮询映射为显示窗口、退出、桌宠开关等动作。macOS 通知用 `objc2-user-notifications`。

## 6. 值得肯定的设计

1. **分层纪律明确。** core 禁止依赖 GPUI(`#![deny(missing_docs)]`,所有公开类型有文档),GUI 通过端口 trait 注入驱动,测试可用临时目录和隔离 runtime。
2. **并发模型清晰。** 单 writer 索引、扫描/查询分队列、所有后台任务有 owner(`background::BackgroundTaskOwner`、`thread_owner.rs`),并有任务容量测试。
3. **隐私与数据边界克制。** 派生库私有权限、不迁移旧库、不读正文、监听只在环回地址、更新检查零出站。
4. **工程门禁成体系。** 行数门禁、中文注释规范、`docs/` 下的 ADR/变更记录/计划/技术债,以及大量按职责命名的回归测试文件。
5. **平台代码隔离到位。** 293 处 `target_os` 集中在 `native/`、`skins/`、`pet/` 的平台子目录,核心逻辑没有被平台分支污染。

## 7. vendor 层(与本次会话讨论的结论一致)

`vendor/` 保存官方 `gpui-pre* 0.3.8`、`accesskit_macos` 的原样上游,外加带校验的补丁。构建脚本在 `OUT_DIR` 里生成补丁后的源码,不改写 upstream。主要补丁:macOS 可变字体、全屏失败/缩放通知、`applicationShouldHandleReopen`、按需帧(frame demand)、`parent-rounded-clip` 特性、菜单动作所有权。

- **依赖关系:** `loki_metris_gui` 直接启用 `gpui_apple` 的 `parent-rounded-clip`,该特性只存在于 vendor 补丁里。本次我在临时副本里去掉 `[patch.crates-io]` 实测,依赖解析直接失败,所以 vendor 不能移除。
- **升级:** crates.io 上这 5 个 `gpui-pre*` 最新版就是 0.3.8,没有可升级的新版本;`accesskit_macos` 上游有 0.27.1,vendor 里是 0.26.3,可能受 GPUI 锁定,未试。
- **告警:** vendored 代码里约 1230 条 `cocoa` 弃用告警已通过 shim 的 `allow` 压制(提交 `d43f7df`),剩 1 条来自传递依赖 `block v0.1.6` 的"未来 Rust 不兼容"提示。

## 8. 风险与技术债

按我认为的优先级排序。前三项来自 `docs/TECH_DEBT.md` 或本次会话的实测,其余是我阅读代码后的判断。

| # | 项 | 说明 | 严重度 |
|---|---|---|---|
| 1 | Windows 未验证 | `TECH_DEBT.md` 明确写"Windows 尚未编译或运行",托盘、自启、单实例、热键、NSIS 都待补验。GUI 里 293 处 `target_os` 中相当一部分是 Windows 路径。 | 高 |
| 2 | 补丁依赖 vendor | GPUI 和 accesskit 的行为补丁只存在于本仓库。上游发版后需要逐项对账补丁是否已被吸收,否则越拖越难合并。 | 中 |
| 3 | `block 0.1.6` 未来不兼容 | 来自传递依赖,工具链升级时可能直接编译失败。 | 中 |
| 4 | GUI 体量远大于 core | GUI 约 107k 行对 core 约 37k 行。其中含大量测试,但 `app/`(约 29.8k)与 `skins/`(约 20.4k)体量值得复查是否有可上移 core 的业务规则。我没有逐文件核实,这是需要后续审计的假设,不是已确认的违规。 | 中 |
| 5 | `unsafe` 集中在 GUI | 已逐处盘点,见附录 A:Windows 约 85 处无法在本机编译验证,macOS 手写 FFI 已收窄。 | 中 |
| 6 | Hook 中继依赖可执行文件路径 | 应用移动或重装会让已写入各工具的 Hook 命令失效,依赖启动时对账修复。 | 低 |
| 7 | 换肤依赖外部应用的调试端口 | 宿主应用升级后 CDP 行为可能变化,存在兼容性追赶成本。 | 低 |
| 8 | 工作区有未跟踪文件 | `git status` 显示 `vendor/docs/`(含 `VENDOR_OVERVIEW.md`)和本次生成的 `.archify/` 未纳入版本控制。我没有创建 `vendor/docs/`,来源未核实。 | 低 |

## 9. 建议的后续动作

1. **补 Windows 编译与冒烟。** 这是最大的缺口,也是 `TECH_DEBT.md` 已承认的事项。
2. **建立 vendor 补丁对账清单。** 每次 GPUI 上游发版,逐个补丁标注"已被上游吸收 / 仍需保留 / 可提 PR"。字体、zoom、reopen 这几项通用性较强,适合先向上游提交。
3. **审计 `app/` 与 `skins/` 中的业务规则。** 抽样检查是否有跨字段校验、重试编排等本应进 core 的逻辑。
4. **审查 `unsafe`。** 为 157 处 `unsafe` 补充或核对不变量注释,重点是 objc2 与 Windows API 边界。
5. **处理 `block` 传递依赖。** 跟踪 GPUI 上游是否迁移到 objc2,避免未来工具链升级时被动。

## 10. 方法与局限

- 用 `cargo check --all-targets`、`cargo test --workspace`、目录与行数统计、源码阅读取得事实。架构图每个节点都带了 `sources`(文件与行号),并在 `--repo-root` 下通过了 archify 的证据校验。
- 图中把 `core` 的 SQLite 索引、GUI 的扫描编排等放在同一画布上,用节点标签 `core` / `gui` 区分归属,没有画 crate 边界框(archify 的边界只支持 region / security-group)。
- 局限:没有运行应用,没有做性能测量;`unsafe`、Windows 分支、各 Agent 解析器的正确性没有逐处审查;`monitor/` 里图片驻留、`pet/` 的窗口放置等较细的子系统只做了概览。


---

## 附录 A:`unsafe` 盘点与收窄(2026-10-11 追加)

### A.1 盘点结果

| 区域 | 数量(生产代码) | 性质 |
|---|---|---|
| `loki_metris_core` | 0 | 之前报告写的 2 处是 `workbuddy_stats` 测试里的字符串 "unsafe",不是 Rust `unsafe` |
| GUI · Windows 专属文件 | 约 85 | Win32/COM/注册表/进程查询,如 `pet/host/windows.rs`(17)、`skins/windows_codex/*`(约 40)、`native/*windows*`、`monitor/wsl/process.rs` |
| GUI · macOS 专属文件 | 约 40 | objc2 通知/窗口 API(标为 unsafe 的由 objc2 决定)、CoreFoundation/Security/proc_pidpath/kill 的 `extern "C"` |
| GUI · 其余 | 2 | `logging.rs`、`platform_index.rs` 里只是文本里的 "unsafe",无实际 `unsafe` |

大多数 `unsafe` 块已有"安全:"/`SAFETY:` 说明。

### A.2 已做的收窄

1. **`loki_metris_core` 加 `#![forbid(unsafe_code)]`。** core 当前确实 0 处 `unsafe`,这条把"core 无 unsafe"从约定变成编译器强制。
2. **`skins/macos_codex.rs` 去掉全部 `unsafe`。** 原来手写 `objc_getClass`、`sel_registerName`、`objc_msgSend` 并用 `transmute` 转函数指针(含 2 处 `transmute` 和 5 个 `extern "C"` 声明),现改为 objc2-app-kit 的 `NSRunningApplication::runningApplicationWithProcessIdentifier` 和 `activateWithOptions`,配合 `objc2::rc::autoreleasepool`。选项位保持原值(`ActivateAllWindows | ActivateIgnoringOtherApps`),因后者在 macOS 14+ 已弃用,用 `#[allow(deprecated)]` 显式标注。文件从 67 行降到 24 行。
3. **依赖改动:** 根 `Cargo.toml` 里已有的 `objc2-app-kit` 增加 `NSRunningApplication` 与 `libc` 两个特性。没有新增 crate,`libc` 已在依赖树里。

验证:`cargo check`、`cargo test --workspace` 全部通过(GUI 1064、core 454、其余 7,0 失败)。**没有实机验证"激活 Codex 窗口"这一行为**,它依赖真实的 Codex 进程。

### A.3 未处理及原因

- **Windows 的约 85 处:** 本机交叉检查 `x86_64-pc-windows-msvc` 在 `ring` 的 C 编译处失败,无法编译验证。在拿不到编译反馈的情况下改 Win32 `unsafe`,风险大于收益,所以保持原样。需要 Windows 主机或带 MSVC/clang 交叉工具链的环境。
- **macOS 的 `kill` / `proc_pidpath` / CoreFoundation / Security `extern "C"`(`skins/macos_identity.rs`):** 没有现成的安全封装可替换而不引入新 crate(如 `libc` 或 `nix`)或改变行为。这些调用已有 `SAFETY:` 说明,缓冲区长度和 PID 重新绑定都有显式检查,我认为保留比硬换更稳妥。
- **objc2 通知 API(`macos_host.rs` 等):** `unsafe` 来自 objc2 对这些方法的标注,调用点已经是单行小块并带说明,没有可再收窄的空间。

## 附录 B:GUI 体量检查(2026-10-11 追加)

### B.1 数字要分清生产与测试

| | 生产代码 | 测试代码 | 合计 |
|---|---|---|---|
| `loki_metris_core` | 约 25,200 | 约 11,900 | 约 37,200 |
| `loki_metris_gui` | 约 65,300 | 约 41,800 | 约 107,200 |

按生产代码算,GUI 与 core 之比约 2.6:1,不是总行数的 2.9:1;GUI 总量里约 39% 是测试。

GUI 生产代码的分布:`app/` 16.5k、`backend/` 15.5k、`skins/` 13.5k、`monitor/` 7.4k、`usage_service/` 5.0k、`pet/` 3.4k、`native/` 3.4k。

### B.2 大部分体量是合理的

- `app/`:GPUI 渲染、键盘、无障碍、窗口状态,本来就是展示与交互。
- `backend/`:Codex/Claude/Grok/WorkBuddy 的会话文件发现与解析,属于文件格式驱动,按 `ENGINEERING_RULES` 应留在宿主。
- `native/`、`pet/`、`monitor/` 的窗口、托盘、通知、环回监听、Hook 文件写入:都是 OS 与进程边界。
- 没有超过 800 行的文件,最大约 500 行。

### B.3 确认的越界:`skins/` 里的纯业务规则

`AGENTS.md` 要求"宿主无关的业务规则、值域、跨字段关系、权威状态属于 core"。我读到 `skins/validation.rs` 里这些函数不碰文件系统,只做规则判断:

- `validate_theme_appearance`:支持模式非空且不重复,`requirements` 只能用于已声明支持的模式(跨字段关系)。
- `validate_appearance_requirement`:`codeThemeId`/`uiFont`/`codeFont` 非空且不超过 160 字符,颜色必须是 `#RRGGBB`,`contrast` 在 0–100。
- `is_valid_requirement_color`、`parse_theme_css` 的变量表约束(`--skin-*` 调色板、`html.codex-dream-skin` 选择器合同)。
- `skins/conversion.rs` 中的 `normalize_theme_color`、`converted_theme_id/name`、`inferred_legacy_id`、旧调色板到新调色板的转换。

对照 core:`loki_metris_core/src/codex_skin.rs` 只有 335 行,覆盖 ID、批量大小、文本与图片名、颜色模式等较粗的规则,**上面这些外观要求规则没有在 core 里**。按我的估计这批纯规则合计约 300–400 行(未逐行测量),迁入 core 后 GUI 只需保留"读文件、调用 core、映射成 `AppError`"。

### B.4 其他没有发现明显越界的地方

我抽查了 `usage_service/agents.rs`(Agent 选择只存监控配置)、`usage_service/hooks.rs`、`monitor/profiles.rs`("缺失补齐与默认值由 core 决定")、`monitor/store/`,这些文件的头注释和函数名都表明规则委托给 core。我没有逐文件通读 `app/`,因此不能断言它完全干净。

### B.5 建议

1. 把 B.3 列的外观规则迁入 `loki_metris_core::codex_skin`,GUI 的 `validation.rs`/`conversion.rs` 只保留 I/O 和错误映射,并把对应单元测试一并搬到 core。这是行为保持的重构,需要走 `$desktop-refactor-code`。
2. 迁移前先给这些规则补足特征测试(现有测试多数通过 GUI 的文件系统路径间接覆盖)。
3. 此项**我没有动手**。它是跨 crate 重构,影响面比 `unsafe` 收窄大,等你确认后再做。
