---
name: desktop-add-gpui-adapter
description: 为已明确选择 GPUI 的中性 Rust 下游建立原生桌面薄适配器，固化双语身份、三态主题、设置、条件支持页、侧栏与按需托盘/通知/自启/单实例/热键，并在选完后协助配置。
---

# 增加 GPUI 原生适配器

只在初始化已选择 `gui_framework = gpui`，或已初始化下游明确批准增加 GPUI 接口时使用。默认桌面框架仍为 Tauri。GPUI 原生视图使用 Rust；根 pnpm 入口运行保留的 Node 工程检查与发布工具，默认无第三方 npm 依赖，无需前端。不得混装 React、WebView 或 Tauri plugin。当前 Harness 根只维护模板和工程规则，不能把产品业务写入这里。

## 最小工作流程

1. 按 `AGENTS.md` 读取当前任务命中的工程与 Rust 规范；GUI 设计只读 `docs/design_standards/README.md` 与 `docs/design_standards/gpui_gui.md`。初次中性初始化复用初始化器已经确认的身份、接口、目标平台和九项 GUI profile，不要求 Product Spec、ADR、Work Plan、版本分类或发布上下文。初始化后新增接口才按 `$desktop-manage-version` 处理已批准 feature。
2. 框架由独立问题选择：`1. Tauri（默认；未回答时使用）`、`2. GPUI`。九项 profile 保持原顺序：`system_tray`、`system_notification`、`autostart`、`about_page`、`sponsor_page`、`single_instance`、`deep_link`、`global_shortcut`、`sidebar_mode`。托盘、系统通知、自启、单实例与全局快捷键允许 enabled/disabled，只按选择生成依赖和实现。深链接仍 unavailable/disabled；托盘和后台热键当前限 macOS/Windows，目标含 Linux 时显式 enabled 在写入前失败关闭。关于/赞助页分别允许 `enabled` 或 `disabled`；侧栏为 `compact` 或 `detailed`。全部选择结束后必须读取 [原生能力与配置询问](references/native-capabilities.md)，汇总已选能力并询问配置协助；具体产品配置留到终端下游唯一根目录，不在 Harness 源收集业务内容。
3. 普通初始化继续复用三个原始 Logo 候选 → 用户选择 → 只验证并标准化所选项的流程；GPUI 只消费已选择的普通 PNG，不生成 Tauri 平台图标。工程验证可在用户明确授权时使用本 Skill 自绘的中性 SVG 或明确给定 PNG，并在 profile 中标记工程夹具，不能冒充正式品牌确认。双语展示名称必须来自同一确认结果；至少一种来自用户，另一种可按初始化流程翻译。不得把 machine ID 当英文展示名称。
4. 初始化必须完整读取 [基础依赖技术选型](references/dependency-baseline.md)：示例工程全部直接库按固定壳层、条件宿主和实际用途列入标准，禁止无用途安装整个列表。根 `[workspace.dependencies]` 固定声明 `gpui-kit = "0.7.1"`、`rust-i18n = "4.2.0"`、`sys-locale = "0.3.2"`，分别提供原生界面/图标、编译期双语词典和真实系统语言。固定端侧日志使用 `tracing = { version = "0.1.44", default-features = false, features = ["std"] }`、`tracing-subscriber = { version = "0.3.23", default-features = false, features = ["fmt", "registry", "std"] }`、`tracing-appender = { version = "0.2.5", default-features = false }`；标准库不足以替代跨模块结构化 span、队列与滚动日志。GUI member 只用 `workspace = true`，测试才给同一 Kit 启用 `test-support`。core 不装 subscriber、文件 sink 或 GUI/runtime；获批业务需要日志时只使用工作区 tracing facade。GPUI Kit 自身 ABI 对齐的精确传递版本由上游负责，不复制来源不明的 pins。日志 API 已于 2026-10-08 核对 [tracing](https://docs.rs/tracing/0.1.44/tracing/)、[tracing-subscriber](https://docs.rs/tracing-subscriber/0.3.23/tracing_subscriber/) 与 [rolling Builder](https://docs.rs/tracing-appender/0.2.5/tracing_appender/rolling/struct.Builder.html)。
5. 本 Skill 只面向已有共享 core 的下游。使用 [scripts/add_gpui_adapter.mjs](scripts/add_gpui_adapter.mjs) 的 `addGpuiAdapter(options)`，必须验证已有根 Cargo、直接 core member、项目/目标平台身份，没有 GUI 且依赖无冲突，随后增加 GUI/profile、独立 `packaging/gpui.json` 与项目内中性背景/所选 PNG 衍生图标，并定点合并 root Cargo 与根 `package.json` 工程入口；不创建或覆盖 core、不创建 Git、不写 Agent Policy 或 Product Spec。已有打包配置也必须保留，冲突时零写入失败。复杂或多行 Cargo 数组目前失败关闭，按明确人工合并处理。[scripts/gpui_adapter_files.mjs](scripts/gpui_adapter_files.mjs) 的 `renderGpuiAdapterFiles(options)` 返回上述文件的纯 Map，没有创建 workspace/core 的写入能力；打包配置和图标使用保留的 `$desktop-build-gpui-release` 标准库 renderer。普通所选 PNG 应已标准化为 256..2048 方形、8-bit RGB/RGBA、非交错格式；中性 SVG 工程夹具可以初始化，但缺少实际 PNG 时打包明确阻断。全新中性工程的源专用流程由源专用初始化器负责，终端下游不得通过本入口继续派生项目。
6. 根 `[workspace.metadata.agent-first-harness]` 固定记录 `gui-framework = "gpui"`，`interfaces` 含 `gui`，`target-platforms` 来自实际选择。GUI 代码放 `<id>_gui/src/`；有子文件的模块使用 `<module>/mod.rs`。壳层 Entity 只拥有页面、语言、主题与设备侧栏偏好；业务规则、业务状态、值域、权限、稳定错误和跨调用编排仍属于 core，并由端侧直接调用 core 用例；适配器不执行 CLI 或解析 CLI 输出。事件绑定实际 Button 自身。主入口唯一日志订阅器经 1024 行有界后台队列同时写 stderr 和平台标准本地日志目录，按日滚动最多保留 7 份；INFO 以上仅接收自身 GUI/core。只记录结构化生命周期事件和稳定错误类别，禁止用户输入、路径、凭据或业务载荷。`lifecycle` 在 GPUI `on_app_quit` 原生不可取消退出回调中同步完成偏好 worker 刷新 → `application_stopped` → `WorkerGuard` 刷新；同一状态提供 `run` 返回回退且只执行一次。macOS `NSApplication terminate` 不保证 `run` 返回，禁止把唯一收尾放在 `run` 之后，也不让短期异步退出超时丢掉最终日志。初始化失败返回失败退出码。
7. 原生窗口第一次或无效状态使用 1440×900、最小 960×640、居中且可调整。后续只恢复有效、与当前显示器相交的尺寸/位置/最大化；不恢复全屏/最小化/隐藏。未选托盘或实际创建失败时不拦截关闭，最后窗口关闭必须实际退出进程；实际创建托盘才关闭隐藏，双语菜单显示/退出。原生服务由 GPUI Global/Task 拥有，退出移除并回收 owned worker；第三方内部线程限制按引用如实报告。GPUI 事件循环留在同步主线程；模板不需要 Tokio，不得为壳层另建或嵌套 runtime。未来业务需要后台异步执行时，复用该适配器拥有的 runtime，不能把整个 GPUI loop 放进 `#[tokio::main]` 或 `block_on` 根任务。
8. 系统通知启用才生成默认关闭的应用偏好开关与 Kit 提交入口；不在启动时请求权限或发测试消息，不把 Submitted 当授权/送达。自启启用才生成真实 OS 状态开关、unknown/error/重试；后台串行读写，初始化只读不注册，Windows 限当前用户，无法安全编码的路径失败关闭。单实例启用才用私有目录锁与有界本地 IPC，在第二进程只恢复窗口后退出；不转发业务载荷。全局快捷键启用才声明真实 global-hotkey 能力，独立空 action contract 零默认键位/零注册，后续 typed action 按明确需求调用 core，部分失败保留未释放 owned 资源，退出只注销 owned。固定设置页包含当前语言名称/实际包版本、中文/英文/跟随系统与浅色/深色/跟随系统三态主题。默认语言来自 `sys_locale::get_locale()`，未知语言回退英文；系统主题观察真实 `WindowAppearance`，显式选择覆盖。标题从同一双语名称、Cargo 包版本和受管品牌联系人生成，仅一个小写 `v`。主题使用 Kit 的完整语义颜色，视图不散落判断深浅。设备偏好用有限白名单文本，在事件循环前有界读取；渲染和 handler 零磁盘 I/O。后台有界合并队列写同目录临时文件并替换，原生不可取消退出钩子等待最后快照完成。
9. compact 为 80px、6px 内容内边距、36px Logo、22px 图标、56px 全宽居中竖排名称，完全没有折叠按钮或折叠翻译键。detailed 为展开 248px、收起 76px、72px/44px Logo、22px 图标、44px 菜单行与 Tooltip；展开图标/名称在满宽内行左对齐，收起图标居中。折叠按钮为 24px 悬浮控件，中心落在侧栏右边界并与顶部 Logo 垂直居中，展开/收起均跨边界各 12px；最后绘制且整个按钮可点击，不占独立菜单行；折叠为独立设备偏好。侧栏与内容偏移消费同一宽度，布局不额外补固定 margin。功能菜单向下增长；底部按已选赞助 → 设置 → 已选关于生成。原生列表页使用 GPUI 的类型化实体/组件设计，不要求 Mantine、TanStack 或 Jotai；跨页面会话视图由进程内实体持有，不能持久化业务数据副本。
10. `about_page: enabled` 才生成 `app/about.rs`、枚举/导航/展示状态与词典键；页面包含当前应用、负责人、品牌联系方式、免责声明、双语更新日志入口和真实不可用更新状态。中性基线未实现 GPUI updater，按钮明确反馈 unavailable/零出站，不能伪装已安装 Tauri updater 或检查成功。`build.rs` 在候选工具提供 `HARNESS_GPUI_RELEASE_NOTES_RS` 时，把经过根 schema v2 校验并安全转义的最近十版 `(date, version, features zh/en, bugs zh/en)` 静态条目及原 JSON 字节复制到 Cargo `OUT_DIR`；env 与路径内容变化均触发重编译。普通开发/初始化未提供该变量时明确空状态；窗口线程不读取磁盘、JSON 或网络。禁用关于页不生成不可达页面或翻译，候选仍嵌入并打包原更新日志字节供独立 GPUI 构建 Skill 核对。
11. `sponsor_page: enabled` 才生成 [固定原生赞助模板](assets/gui/src/app/sponsor.rs)、三档权益卡片、独立支付区、完整媒体和词典。赞助页拥有完整背景/留白，父内容槽不生成额外“赞助”标题、顶部底线或内边距；页面 `flex_grow(1)`、`flex_shrink_0` 与父列滚动约束共同防止整页位移。完整居中介绍含维护边界、能力图标与 PC 说明；按扣除当前侧栏后的内容宽度 ≥1000px 三列，否则一列，支付区 ≥924px 左右排，否则上下排。源码布局参考优化过的 gpui-demo，内容来自自含受管品牌源 [assets/brand-support](assets/brand-support)。固定品牌、价格、联系方式、支付二维码与免责声明为已有批准的产品家族例外；媒体逐字节校验 `media-manifest.json` 大小/SHA-256，不复制参考应用业务、凭据、Logo 或远端地址。二维码保留 120×136、白底与原比例，只静态展示，不授权支付自动化。禁用时 sponsor 源码/回归、词典、媒体和运行时引用全部缺席。来源和许可责任见 [SOURCE.md](assets/brand-support/SOURCE.md)。
12. GPUI 当前固定宿主基线与 Tauri 分开报告：真实系统语言、三态主题、受限窗口恢复、最后窗口关闭退出已经实现；updater 与通用原生 dialog 当前 `unavailable`，没有 Tauri plugin、ACL 或伪造实现。新增这些真实宿主能力必须单独建立可复用实现、测试和当前平台的可观察证据，再调整 capability facts；不能用 stub 提升为 supported。
13. 原生能力变更运行 `node --test .agents/skills/desktop-add-gpui-adapter/scripts/gpui_native_capabilities.test.mjs`；profile 纯解析器拒绝缺字段/乱序/重复块/非法中性合同。运行本次 add-only 回归 `node --test .agents/skills/desktop-add-gpui-adapter/scripts/add_gpui_adapter.test.mjs`；变更 Node 工程入口时同时运行 [gpui_node_tooling.test.mjs](scripts/gpui_node_tooling.test.mjs)。生成的同名 `_test.rs` 使用 Kit 真实渲染树验证详细菜单左对齐、两档折叠按钮中心与侧栏边界/Logo中心对齐、跨边界部分点击、壳层无水平偏移，以及赞助模板在宽窄窗口/双语/明暗主题中的全页槽、等宽等高卡片与二维码尺寸；不以字符串扫描替代几何证据。源专用全新工作区 wrapper、对应测试和 core 模板仅由初始化器拥有，初始化完成与升级时裁掉，不得从终端下游恢复。真实初始化执行非空 `pnpm run test` 与 `cargo build --workspace`；首次当前宿主初始化再通过 Computer Use 观察窗口、Logo/标题/双语、主题、所选侧栏、设置/条件支持页、所选五项桌面能力、关闭隐藏或退出与重启偏好；只对明确安全绑定执行真实按键，自启必须恢复原登录项，需打包权限的通知横幅单列待验证。它们只是 debug 原生初始化验证；明确 GPUI 构建/打包转交独立 `$desktop-build-gpui-release`，不混入 Tauri 管线。未执行平台如实报告。

## 已有工作区 CLI 契约

```sh
node .agents/skills/desktop-add-gpui-adapter/scripts/add_gpui_adapter.mjs \
  --target /absolute/existing/workspace \
  --project-id loki_metris \
  --name-zh 示例工具 \
  --name-en 'Example Tool' \
  --owner 'Confirmed Owner' \
  --system-tray enabled \
  --system-notification enabled \
  --autostart enabled \
  --single-instance enabled \
  --global-shortcut enabled \
  --about-page enabled \
  --sponsor-page enabled \
  --sidebar-mode detailed \
  --target-platforms macos \
  --logo /absolute/selected-logo.png
```

目标必须已有身份匹配的共享 core 与 Cargo 工作区；空目录、已有 GUI、依赖冲突、profile 覆盖或目标平台漂移都失败关闭。`--logo` 省略只表示明确标注的中性工程夹具；普通产品初始化必须消费已选择 Logo。`--target-platforms` 接受与既有工作区一致且去重的 `macos,windows,linux` 子集，不把声明平台当已验证平台。支持页/侧栏使用已经确认的配置，交互选择归初始化器或已批准的新增接口请求。成功仅报告增加的文件与保留 core 事实，不把源码生成当运行验收完成。

## 根 pnpm 工程入口

[gpui_node_tooling.mjs](scripts/gpui_node_tooling.mjs) 的 `renderGpuiPackageJson(existingBytes = null)` 是标准库纯函数。新 `package.json` 为 `private: true`，仅声明 `engines.node = ">=24.21.0"`、`engines.pnpm = ">=12.4.1"` 和以下脚本；无 npm 依赖，无需先执行安装。已有 package 的公开性、`packageManager`、用户脚本、依赖和其他字段原样保留，只添加缺失的工程脚本与 engine。非等值同名脚本或 engine（包括更严格范围）、重复 JSON 键、非法内容及链接都在任何写入前失败；不得降低已有要求或新增精确 `packageManager` pin。add-only 在安装前复核 package/Cargo/core 快照并在失败时撤销本次文件。

| 命令 | 实际行为 |
| --- | --- |
| `pnpm run validate` | [gpui_validate.mjs](scripts/gpui_validate.mjs) 串行汇总保留的文件行数、Rust 中文声明注释和 `check_core_first.mjs --workspace-root .` 工程门禁，返回首个非零状态；不依赖源专属 runner。 |
| `pnpm run test` | `cargo test --workspace --all-targets --all-features`。 |
| `pnpm run release:inspect` | `release_git.mjs inspect --project-root .`，只读工作区快照。 |
| `pnpm run release:notes check --file release-notes.json` | 原更新日志 helper；`upsert`、`render` 及其参数仍遵守原发布 Skill。 |
| `pnpm run release:context check --project-root .` | 原发布上下文 helper；`write`、`verify` 及其必填参数保持不变。 |
| `pnpm run release:git --version <version> --release-context-sha256 <sha256>` | 原 `git_lifecycle.mjs release --project-root .`；只在明确发布授权后调用，版本与上下文摘要门禁不可省略。 |
| `pnpm run gpui:package preview --root .` | 原独立 GPUI 构建 helper；`setup`、`icons`、`build` 与 E2E/签名/候选边界仍由构建 Skill 管理。 |

升级继续把根 `package.json` 视为 protected，不由通用升级器覆盖。确认需要迁移入口后，显式运行 `node .agents/skills/desktop-add-gpui-adapter/scripts/merge_gpui_node_tooling.mjs merge --root .`；该受限迁移仅接受已有 GPUI workspace 和实际保留工具，只写 package，冲突零写入，重复调用保留文件原字节。pnpm 参数直接跟在脚本后，不添加额外 `--` 分隔。

## 完成报告

报告实际 GUI 框架、九项选择、真实依赖、系统语言/主题/窗口/关闭行为、支持页与媒体条件裁剪，以及当前宿主真实测试/E2E 结果。单列五项实际选择、配置协助答复/移交、已声明依赖与平台限制；深链接及 updater/dialog 如实标为 unavailable；未执行的跨平台构建、候选打包和签名不得称通过。
