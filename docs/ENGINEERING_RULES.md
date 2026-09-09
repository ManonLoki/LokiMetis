# 工程规则

本文件是 LokiMetis 在架构边界、文件组织、代码注释、文档维护、测试、例外和机械检查方面的唯一详细事实来源。`AGENTS.md` 只负责启动路由，语言和 GUI 文档只补充各自技术事实。

## 1. 规则等级与产品边界

- 硬规则保护架构、正确性、跨平台能力、安全和交付可信度；违反时必须修复，确需例外时按第 6 节记录。
- 默认规则用于保持可读性和一致性；有真实、局部且可说明的原因时可以偏离。
- 当前 `productDefinitionRequired = false`。产品业务以最新 Approved Product Spec 为准；新增或改变目标、范围、数据来源、远程地址或成功标准时必须重新进入产品定义门禁。

## 2. 架构、文件与依赖

### 2.1 Core-first 硬规则

- 不依赖 Tauri、WebView 或操作系统才能成立的领域类型、业务规则、语义校验、业务默认值、应用用例与工作流编排、状态转换、平台无关权限、权威业务状态、迁移、持久化策略和稳定错误必须实现于 `loki_metis_core`，并通过 core 自有类型的明确 API 暴露。
- `loki_metis_gui` 是唯一 adapter，只负责 Tauri/React 装配、IPC 结构解析、展示与纯交互状态、桌面宿主机制、调用 core，以及把结果映射为界面状态。薄层按职责判断，不按行数判断。
- IPC 缺少结构字段或违反宿主约束可由 adapter 拒绝；值域、跨字段关系、业务权限、资源状态、幂等性、可执行性及改变业务结果的默认值由 core 判定。
- adapter handler 若需要通过条件、重试或状态决策编排多个 core 调用，应把编排提升为一个 core 用例 API。core 不得直接或间接依赖 GUI、Tauri、WebView 或前端框架。
- 文件系统、网络、外部进程与 OS API 集中在职责明确的 adapter/基础设施模块。何时调用、允许什么、失败语义以及结果怎样改变领域状态仍由 core 决定；真实边界需要时可由 core 定义运行时中立的能力接口并由 adapter 装配，不得为假想未来预建接口或 crate。
- 当前产品已批准的宿主边界包括 LokiMetis 应用数据目录内的皮肤文件操作、经官方安装路径验证且由用户动作触发的 Codex/WorkBuddy 进程操作，以及仅连接回环地址并验证目标页面的 CDP。路径穿越、链接、超限、非官方进程、非回环端点、未确认的破坏性重启和后台自动注入必须拒绝；超时、取消、失败与进程回收必须可观察，路径、进程参数和 CDP 响应不得原样进入日志。
- workspace 固定只有 `loki_metis_core` 与 `loki_metis_gui/src-tauri` 两个成员，不得新增 CLI、TUI 或 MCP adapter。

### 2.2 GUI 与能力边界

- `docs/GUI_APP_PROFILE.md` 必须有且只有一个 `gui-initialization-config` 代码块，依次包含共享的 `system_tray`、`system_notification`、`autostart`，本项目已批准扩展 `sponsor_page`，再包含共享的 `single_instance`、`deep_link`、`global_shortcut`、`sidebar_mode`。八项当前值不得静默推断、遗漏或修改；`deep_link = enabled` 必须同时有 `single_instance = enabled`。
- 当前启用托盘、系统通知、自启、单实例与深链接。单实例回调只恢复已有主窗口；深链接只精确接受 `app-loki-metis://restore`，在冷启动和热启动时均先验证再恢复，不承载业务参数。
- 当前赞助支持启用并按已批准产品差异嵌入 `/settings` 底部，只打包用户明确提供的微信与支付宝收款码；不建立独立 `/sponsor` 路由或侧栏入口。全局快捷键禁用，其依赖、feature、配置、命令、ACL、状态、i18n 与生命周期接线必须缺席。
- system-locale、window-state 和 dialog 是所有 GUI 的固定基线。系统语言只取官方 OS locale；窗口状态只恢复 `SIZE | POSITION | MAXIMIZED`；dialog 只向主窗口开放精确 `dialog:default`，不得附带文件系统授权。
- Tauri Builder 的相对顺序固定为 single-instance、deep-link、os、window-state、dialog、opener、notification、autostart，每项恰好一次。opener 只向主窗口开放 `https://github.com/ManonLoki/LokiMetis`；托盘从 `.setup(...)` 与 `.on_window_event(...)` 接线，不是插件。
- 中央 Builder 只有一个合并后的 `invoke_handler`。固定命令为 `get_app_metadata`、`get_system_locale`、`set_interface_language`、`load_release_notes`；通知与自启的窄状态命令按当前 profile 加入。不得公开通用 OS、窗口、文件系统或通知 JavaScript API。
- 通知由 Rust-only 串行 worker 拥有，应用偏好默认关闭，权限/发送失败可见且不伪造成功；关闭时回收 worker。macOS 仅在用户开启通知时读取授权状态，仅 `NotDetermined` 请求并复读，只有 `Authorized`、`Provisional` 或 `Ephemeral` 才持久化开启；`Denied`、`Restricted` 或请求后仍未授权时，通过只接收内部 `AppHandle` 的固定 Rust opener 打开当前 bundle identifier 对应的 Notifications 系统设置并保持关闭，打开失败也必须可观察。应用启动不得查询、请求、打开设置或发送通知。自启以 OS 注册状态为权威，初始不注册，设置切换失败时恢复真实状态。
- 托盘保留 `show_window`、`quit` 基线并按已批准产品差异增加 `toggle_pet_overlay`，文案由 `rust_i18n` 随界面语言更新；左键与显示项恢复并聚焦主窗口，显隐项只改变当前会话桌宠真实可见性，窗口关闭只隐藏，退出项才终止应用。托盘标题复用“全部”视图设备当地当天的联合 Token 总量，不另建聚合、轮询或远端数据源；无记录或读取失败时清空，Windows 只显示图标，Linux 允许按桌面实现降级。
- `$desktop-prepare-gui-support-surfaces` 只提供设置、侧栏、主题、i18n 和本地发布说明的共享中性基线，不得重新引入共享模板的赞助档位、背景、联系人或媒体。本项目的双收款码是 `docs/GUI_APP_PROFILE.md` 与 Approved Product Spec 明确批准的独立产品差异，不属于应清除的共享支持资产。
- 应用不安装 updater 或等价客户端，不实现联网检查、强制更新、下载/安装更新、更新制品、产品统计、崩溃上报、远程观测 exporter 或其他远程遥测。结构化 tracing 只能写入本地、滚动且脱敏的日志。

### 2.3 GUI 展示与交互

- 窗口标题固定为 `LokiMetis`，不带版本号。设置页等用户可见版本在展示边界先移除已有 `v`/`V`，再添加且只添加一个小写 `v`。机器版本不带前缀。
- 首次启动或持久状态缺失、损坏、越界时，主窗口回退为 1440×900、最小 960×640 并居中；只恢复仍与当前显示器工作区相交的尺寸、位置和最大化状态。
- 固定 `/settings` 页面展示应用、版本、本地更新日志、唯一 GitHub 仓库入口、语言和浅色/深色/跟随系统主题、已启用的通知和自启 Switch、唯一 Agent 复选面板，以及底部双收款码赞助区。用量看板在「数据源」后保留 `/dashboard/settings` 「设置」子路由，只承载全局扫描间隔与自动清理，在「全部」视图不显示。Hooks 目录与写入表单只位于 `/monitor/settings` 且消费统一选择，不得复制 Agent 复选；`/about`、独立 `/sponsor`、`/test` 和相应导航必须缺席。
- `/pet` 与 `/pet-settings` 是已批准的独立桌宠 WebView 窗口而非主壳路由；`/skins` 是应用换肤业务入口。桌宠显隐、分页、布局、缩放、锁定和置顶以 Rust/core 权威状态为准，关闭设置窗不得改变主窗口或 Hook listener 生命周期；换肤继续遵守本地文件、官方进程与回环 CDP 边界。
- 所有可见文案进入 `zh-CN`/`en-US` i18n。标题、应用元数据与侧栏不得包含联系人；设置页不得包含联系人、隐私/统计/遥测或应用更新控件。收款码只做本地静态展示，不解析、不重编码且不触发支付。
- UI 先按 `docs/design_standards/README.md` 精确匹配。当前侧栏使用 `tauri-gui-sidebar-compact-80-v1`：80px 宽、6px 内容内边距、36px Logo、22px 图标、全宽居中名称，不可折叠。
- GUI 操作必须绑定在真正拥有动作的语义元素上。按钮、链接、Switch、Checkbox 和菜单项不得由父级 Card、行、单元格或 `div` 代理；标题和说明用稳定 ID 与 `aria-labelledby`/`aria-describedby` 关联。
- 页面工作上下文若需跨路由保持，由应用根 Jotai store 中稳定的页面 atom 持有，仅存活于当前进程。不得写入 localStorage、sessionStorage、IndexedDB、Tauri Store、配置文件、数据库或 URL；语言与主题等批准的设备偏好不受此限制。
- 异步数据与缓存由 TanStack Query 拥有，Jotai 不镜像结果。筛选、范围或每页数量变化时页码归 1；只有查询成功、当前页大于 1 且结果为空时才回退第 1 页，加载、取消、超时和错误不得被解释为空结果。
- Tabler 图标作为唯一通用图标库；存在合适图标时不得手写 SVG、使用字符或 emoji 代替。

### 2.4 文件规模与模块

- Rust 生产/测试文件 401–800 行、前端生产/测试文件 501–1000 行、其他人工维护文本 501–2000 行只成为发布审查候选，日常任务不因软阈值追加复核或阻断；只有明确发布且当次 `reviewSelection: enabled` 时才按高内聚、职责单一和职责相近性集中判断是否拆分。Rust 801 行、前端 1001 行、其他文本 2001 行起始终失败，不能用 ADR、压缩可读性或空壳转发规避。
- Rust 模块拆为目录时使用 `mod.rs` 保持稳定入口；前端按高内聚页面、组件、hook、状态和适配器组织，不强制创建桶式 `index.ts`。
- 建议阈值不是机械拆分命令。职责单一、场景高内聚且拆分会制造循环依赖时可保留，但必须完成语义复核；硬上限无例外。
- 只有独立发布、生命周期、权限或工具链强制要求时才能新增 crate/包，并记录为何不能属于现有模块。

### 2.5 依赖与异步

- 优先使用标准库和已有直接依赖。新增或主动更新时选取官方 registry 当前兼容的稳定版本，验证 Rust 1.95、Node/pnpm、peer、平台与实际 API/features 后声明完整下界；不得写 `latest`、tag、通配符或依赖全局安装。
- 锁文件固定当前解析结果，不替代最低兼容范围。版本下界变化必须在最低支持工具链和当前锁定环境分别验证。
- 已初始化项目只在真实测试或构建因受管环境失败后调用 `$desktop-check-development-environment` 恢复一次：缺失工具安装官方当前兼容稳定版，可证明低于最低下界时沿当前宿主受管路线升级，支持范围内稳定版原样复用；存在显式上界时超界、预发布、无法解析或损坏状态均失败关闭。只读检查必须零写入并明确报告 `upgrade-required`，不得降低项目门禁、回退依赖或锁文件、注入 shim、改用旧工具或替代工具链迁就宿主。
- 受管工具写入当前用户的受管全局根并持久去重 PATH，随后必须在当前进程和只读取持久 PATH 的新 shell 复探同一精确版本；写入前拒绝安装根、profile/config/env/fish 的符号链接、reparse point 或非普通对象，并以同目录临时文件和原字节比较完成原子替换。Git 作者身份与提交模板仅由 `$desktop-configure-git-commits` 在独立仓库 local 作用域维护，不写 global/system。
- 每个后台任务必须有 owner、取消路径、超时/重试上限和关闭回收；不得启动 detached task，锁不得跨越网络、进程、用户交互或其他不受控 `.await`。
- 日志至少包含时间、级别、target 与消息，不记录秘密、令牌、个人数据、完整本机路径或未脱敏业务载荷，不配置网络 exporter。

## 3. 中文代码注释

- 人工维护的 Rust `struct`、`enum`、`union`、type alias、trait、具名函数、关联函数、方法和测试函数必须有声明自身紧邻且包含中文的 outer doc comment；attribute 可以位于文档注释与声明之间。TypeScript/TSX 的 `class`、`interface`、type alias、`enum`、具名函数、方法/访问器/构造器、直接或后置命名/默认导出的组件与 hook，以及真实 Vitest `test`/`it` 场景必须有紧邻中文注释；局部变量、循环绑定和普通匿名/极短内联回调由所属声明覆盖。
- 公共 API 使用适合工具链的文档注释。注释说明业务职责、关键约束、不变量、非直观思路、错误和副作用，不复述语法；简单构造、取值与薄转发也要准确说明，但不得批量生成套话。
- 模块顶部说明职责、边界和禁止承担的工作；复杂算法或状态机先写整体思路，再为关键分支解释“为什么”。
- `TODO`、`FIXME`、`HACK` 必须说明问题、触发条件、影响和移除标准；当前任务不能解决时同步登记真实技术债，已完成或失效标记立即删除。
- Rust 与 TypeScript 门禁必须对无有效扫描对象、语法残缺、非法 UTF-8、NUL、源码符号链接、坏配置或超时失败关闭，并报告根相对路径、行列、声明类别和名称。TypeScript 只精确排除声明的生成物与依赖/产物目录，不得因任意目录片段名为 `build` 等通用词而跳过源码。
- 机械检查只证明注释存在与归属，并仅在本次变化需要、用户明确要求或发布/渠道硬要求时运行；内容质量和 core-first 仍需沿真实执行路径做语义审查。

## 4. 文档与事实来源

- `README.md` 是用户入口；`AGENTS.md` 是任务路由；`docs/AGENT_POLICY.md` 保存 Agent 持久策略；`docs/ENGINEERING_RULES.md` 保存工程规则；`docs/RUST_GUI_TEMPLATE.md` 保存当前 Rust/Tauri 基线；`docs/GUI_APP_PROFILE.md` 保存 GUI 身份、选择和设计标准；`docs/RELEASE.md` 保存版本、构建和候选合同。
- 根 `Cargo.toml` 的 `[workspace.package].version` 是当前版本唯一事实源；`.harness/version-state.json` 只保存发布周期与缺陷去重状态，不是第二版本源。根 `release-notes.json` 已采用 `schemaVersion: 2` 保存近五个正式版本的中英文更新日志，只由正式发布准备根据真实差异更新；构建只读校验并逐字节嵌入，任何字节变化都要求重新提交、构建和验收。
- Product Spec、Product Status、Work Plan、ADR、Changelog、Verification 和技术债只在各自事件真实触发时创建或更新，不生成占位文档。构建、收集、E2E、完整验收和就绪复核的事实只进入忽略的 `release/` 候选原子集合、manifest 声明的相邻证据和最终回复，不写入 tracked 项目记忆；真实渠道发布成功后的受管记录阶段或独立人工复核/长期审计才按对应事实源留证。
- 产品边界变化写最新 Product Spec；长期决定和硬规则例外写同日 ADR；重要阻断或跨会话交接才更新 Product Status；只有用户要求持久计划或高风险协调时创建 Work Plan。
- 标题和首段先给结论；示例必须标明性质；内部链接使用相对路径；日期用 `YYYY-MM-DD`。删除或替换规则时同时清理失效入口、链接和摘要。
- `AGENTS.md` 不超过 120 行或 20,000 UTF-8 字节；所有人工维护文档适用第 2.4 节文本阈值。

## 5. 测试与交付

### 5.1 日常开发

- 每个行为变化由本次需要的相关非空单元/回归测试覆盖成功路径和最高风险失败路径；缺陷修复增加能复现缺陷的测试。纯文档、元数据、格式或机械变化只做解析与差异完整性所需的最小检查。
- 新业务行为先由 core 测试覆盖领域成功/失败，再由 adapter 测试覆盖输入、协议、映射和宿主生命周期。每个公开操作应可追溯“GUI 操作 → core 用例 → core 测试”。
- 测试应确定、隔离、可重复，不依赖真实网络、当前时间、随机顺序、用户主目录或机器已有状态；文件系统测试使用独立临时目录。
- 被跳过或依赖特定环境的测试必须说明原因、启用条件和影响。测试名使用英文 `snake_case`，声明仍需中文注释。
- 日常开发只运行本次需要的测试；不因多步骤、多模块或 Agent 偏好自动运行全仓检查、构建、E2E 或完整验收。
- 已初始化项目在实施前由 `$desktop-manage-version` 只读分类，变化完成并通过相关测试后才提交版本：每个发布周期首个 `feature` 提升 Minor 并锁到真实发布成功；每个具有新稳定 ID 的 `bug-fix` 提升 Patch 且不受功能锁影响；显式 Major 仍需用户批准。新生成 Minor/Patch 仅为 `0..99` 并按 base-100 进位（例如 `0.0.99 → 0.1.0`、`0.99.99 → 1.0.0`）；自动数值进位不替代显式 Major 授权，Major 只受 Cargo `u64::MAX` 限制。历史 Minor/Patch `100` 只在下一次真实提升时规范化当前目标，`check`、`plan`、`maintenance`、构建和 Harness 同步均不得为此写版本或重置周期。

### 5.2 构建与候选

- 显式构建运行 Rust workspace 与前端全部非空单元测试。零测试或失败阻断构建；完整命令以当前 Cargo/package 脚本和对应 Skill 为准。
- Windows 普通本地安装试包是开发制品：不创建发布说明或 `release/`，不提交、不安装、不解析候选 E2E/性能，也不声称可分发。
- 正式发布先由 `$desktop-prepare-release` 在任何写入或提交前各解析一次 `reviewSelection`、`performanceSelection` 与 macOS 签名意图/来源并封存到候选信封，构建只能只读消费，不能重新询问、翻转选择或补造证据。安全、隐私、不可逆操作、对外兼容契约或产品/渠道硬要求强制启用发布审查；其余情况按当前请求或一次确认决定。关闭只省略非必要语义审查，不能跳过范围、秘密、测试、clean、分支/HEAD、签名、渠道和必需人工签署。
- 发布准备要求独立 Git 根、具名 attached 分支和可解析的 40 位 HEAD，用绑定分支、HEAD、工作区、index 与未跟踪字节的摘要冻结已复核范围。提交前用隔离 index 计算精确预期 patch，只暂存复核路径并正常运行 hooks；提交后必须仍在同分支、形成直接非 merge 子提交，且 committed diff 与冻结 patch 逐字节一致。分支/HEAD/字节变化、范围外 staged 内容、秘密、hook 夹带或签名交互都必须失败关闭并重新复核，不能覆盖、贮藏或吸收范围外用户变化。
- `reviewSelection: enabled` 时审查上次正式发布提交（首发为仓库起点）到 clean `sourceHead` 的累计行为、core/adapter 边界、对外契约、职责/规模候选与临时标记，全部完成后才记录结构化通过证据；修复后必须重新测试、提交和审查。`disabled` 时记录 `Not run`、非空原因与剩余风险，并要求审查证据字段缺席。
- 发布构建在测试/编译前单独解析本次 E2E 选择；性能选择只由发布准备解析。性能启用或渠道要求时，对同一 clean HEAD 运行 `gui-release-v2` release-profile no-bundle 探针：一次预热后恰好五次冷启动、至少二十次代表性交互、全部 Long Task、整棵进程树 CPU/RSS、重复操作内存增长和退出回收都必须形成原始证据与 `performanceRuntimeBinding`。每次样本前恢复同一隔离 window-state 基线，所有成功、失败、超时和取消路径都恢复并复核用户原状态；无法定位/恢复窗口状态、绑定探针与最终 runtime 字节或回收进程时失败且不可豁免。只有允许豁免的纯指标失败可在用户查看原始证据并明确确认后记为 `waived`，不得改判通过；关闭且无硬要求时固定 `Not run`，保存原因/风险并要求所有探针、阈值、证据、豁免和运行时绑定字段缺席。
- macOS 签名意图来源按 `channel-required > requested > configured > not-requested` 唯一化；未获批准、未被请求且无渠道硬要求时默认为 `disabled/not-requested`，不得探测本机证书、身份、公证凭据或 profile。启用后签名、公证、stapling 与 Gatekeeper 验证必须全有或全无，任一失败不能回退 unsigned；当前 `system_notification = enabled`，因此 macOS 候选必须启用并完成签名。构建、收集与验收只能复核已封存意图和最终证据，不能自动补签。
- 最终候选绑定源码提交、目标平台、产物摘要、签名状态、发布说明、发布审查、测试、性能与 E2E 结果。manifest 状态只允许 `pending`、`rejected` 或 `accepted`；`ready` 仅是完整 `accepted` 原子集合的只读复核结论，不得持久化。构建、收集及验收都必须先在项目根同级 staging 写入整组、重新枚举精确集合并复算每个字节，再以目录级原子替换 `release/`；状态更新同样整组提交，禁止 mixed 状态或逐 manifest 更新。签名、公证、打包或任何后处理改变产物字节、启动器、依赖或运行行为时即形成新候选，必须重新验收。
- mock、stub、中性脚手架、开发预览、源码片段或只调用内部函数的结果不具备候选验收资格。未实际执行一律记录 `Not run` 或 `Unverified`。

## 6. 规则例外

- 硬规则例外必须在实施前获得负责人批准，并在同日 `docs/adr/YYYYMMDD_ADR.md` 记录规则、理由、风险、范围、替代验证和恢复条件。
- 第 2.4 节硬行数上限不接受 ADR 例外。默认规则的局部偏离可在计划或复核中说明，不要求正式 ADR。
- 例外必须尽可能小；临时例外必须有删除条件。人工批准不会把失败或未执行的验证改判为通过。

## 7. 机械检查边界

- 可确定规则可以自动化，但日常开发不因此自动执行所有检查。显式构建运行全量非空单元测试和真实构建；格式、lint、静态、链接或治理检查仅在本次变化、用户要求或发布/渠道门禁需要时运行。
- 自动检查应覆盖 core/adapter 依赖方向、文件硬上限、中文声明注释存在性，以及 updater、未批准的独立赞助页、共享模板赞助档位/背景/联系人/媒体、全局快捷键、产品统计和远程遥测残留。`TODO`、`FIXME`、`HACK` 与软行数候选只在当次发布 `reviewSelection: enabled` 时报告；不得把本项目已批准且逐字节保留的双收款码误判为残留。
- 自动检查失败必须返回非零状态并报告精确位置。业务逻辑归属、注释质量、UI 可用性和最终候选行为仍需语义或真实宿主验证，不能由关键词和行数替代。
