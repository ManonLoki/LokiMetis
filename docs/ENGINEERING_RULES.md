# 工程规则

本文件是 LokiMetis 在架构边界、文件组织、代码注释、文档维护、测试、例外和机械检查方面的唯一详细事实来源。`AGENTS.md` 只负责启动路由，语言和 GUI 文档只补充各自技术事实。

## 1. 规则等级与产品边界

- 硬规则保护架构、正确性、跨平台能力、安全和交付可信度；违反时必须修复，确需例外时按第 6 节记录。
- 默认规则用于保持可读性和一致性；有真实、局部且可说明的原因时可以偏离。
- 当前 `productDefinitionRequired = false`。产品业务以最新 Approved Product Spec 为准；新增或改变目标、范围、数据来源、远程地址或成功标准时必须重新进入产品定义门禁。

## 2. 架构、文件与依赖

### 2.1 Core-first 硬规则

- 不依赖 Tauri、WebView 或操作系统才能成立的领域类型、业务规则、语义校验、默认值、用例编排、状态转换、权限、权威业务状态、迁移、持久化策略和稳定错误必须实现于 `loki_metis_core`，并通过 core 自有类型的明确 API 暴露。
- `loki_metis_gui` 是唯一 adapter，只负责 Tauri/React 装配、IPC 结构解析、展示与纯交互状态、桌面宿主机制、调用 core，以及把结果映射为界面状态。薄层按职责判断，不按行数判断。
- IPC 缺少结构字段或违反宿主约束可由 adapter 拒绝；值域、跨字段关系、业务权限、资源状态、幂等性、可执行性及改变业务结果的默认值由 core 判定。
- adapter handler 若需要通过条件、重试或状态决策编排多个 core 调用，应把编排提升为一个 core 用例 API。core 不得直接或间接依赖 GUI、Tauri、WebView 或前端框架。
- 文件系统、网络、外部进程与 OS API 集中在职责明确的 adapter/基础设施模块。何时调用、允许什么、失败语义以及结果怎样改变领域状态仍由 core 决定；不得为假想未来预建接口或 crate。
- workspace 固定只有 `loki_metis_core` 与 `loki_metis_gui/src-tauri` 两个成员，不得新增 CLI、TUI 或 MCP adapter。

### 2.2 GUI 与能力边界

- `docs/GUI_APP_PROFILE.md` 必须有且只有一个 `gui-initialization-config` 代码块，依次包含 `system_tray`、`system_notification`、`autostart`、`sponsor_page`、`single_instance`、`deep_link`、`global_shortcut`、`sidebar_mode`。当前值不得静默推断或修改。
- 当前启用托盘、系统通知、自启、单实例与深链接。单实例回调只恢复已有主窗口；深链接只精确接受 `app-loki-metis://restore`，在冷启动和热启动时均先验证再恢复，不承载业务参数。
- 当前赞助支持启用并按已批准产品差异嵌入 `/settings` 底部，只打包用户明确提供的微信与支付宝收款码；不建立独立 `/sponsor` 路由或侧栏入口。全局快捷键禁用，其依赖、feature、配置、命令、ACL、状态、i18n 与生命周期接线必须缺席。
- system-locale、window-state 和 dialog 是所有 GUI 的固定基线。系统语言只取官方 OS locale；窗口状态只恢复 `SIZE | POSITION | MAXIMIZED`；dialog 只向主窗口开放精确 `dialog:default`，不得附带文件系统授权。
- Tauri Builder 的相对顺序固定为 single-instance、deep-link、os、window-state、dialog、opener、notification、autostart，每项恰好一次。opener 只向主窗口开放 `https://github.com/ManonLoki/LokiMetis`；托盘从 `.setup(...)` 与 `.on_window_event(...)` 接线，不是插件。
- 中央 Builder 只有一个合并后的 `invoke_handler`。固定命令为 `get_app_metadata`、`get_system_locale`、`set_interface_language`、`load_release_notes`；通知与自启的窄状态命令按当前 profile 加入。不得公开通用 OS、窗口、文件系统或通知 JavaScript API。
- 通知由 Rust-only 串行 worker 拥有，应用偏好默认关闭，权限/发送失败可见且不伪造成功；关闭时回收 worker。自启以 OS 注册状态为权威，初始不注册，设置切换失败时恢复真实状态。
- 托盘使用 `show_window` 与 `quit` 两个稳定菜单 ID，文案由 `rust_i18n` 随界面语言更新；左键与显示项恢复并聚焦主窗口，窗口关闭只隐藏，退出项才终止应用。
- 应用不安装 updater 或等价客户端，不实现联网检查、强制更新、下载/安装更新、更新制品、产品统计、崩溃上报、远程观测 exporter 或其他远程遥测。结构化 tracing 只能写入本地、滚动且脱敏的日志。

### 2.3 GUI 展示与交互

- 窗口标题固定为 `LokiMetis`，不带版本号。设置页等用户可见版本在展示边界先移除已有 `v`/`V`，再添加且只添加一个小写 `v`。机器版本不带前缀。
- 首次启动或持久状态缺失、损坏、越界时，主窗口回退为 1440×900、最小 960×640 并居中；只恢复仍与当前显示器工作区相交的尺寸、位置和最大化状态。
- 固定 `/settings` 页面展示应用、版本、本地更新日志、唯一 GitHub 仓库入口、语言和浅色/深色/跟随系统主题、已启用的通知和自启 Switch、唯一 Agent 复选面板，以及底部双收款码赞助区。用量看板在「数据源」后保留 `/dashboard/settings` 「设置」子路由，只承载全局扫描间隔与自动清理，在「全部」视图不显示。Hooks 目录与写入表单只位于 `/monitor/settings` 且消费统一选择，不得复制 Agent 复选；`/about`、独立 `/sponsor`、`/test` 和相应导航必须缺席。
- 所有可见文案进入 `zh-CN`/`en-US` i18n。标题、应用元数据与侧栏不得包含联系人；设置页不得包含联系人、隐私/统计/遥测或应用更新控件。收款码只做本地静态展示，不解析、不重编码且不触发支付。
- UI 先按 `docs/design_standards/README.md` 精确匹配。当前侧栏使用 `tauri-gui-sidebar-compact-80-v1`：80px 宽、6px 内容内边距、36px Logo、22px 图标、全宽居中名称，不可折叠。
- GUI 操作必须绑定在真正拥有动作的语义元素上。按钮、链接、Switch、Checkbox 和菜单项不得由父级 Card、行、单元格或 `div` 代理；标题和说明用稳定 ID 与 `aria-labelledby`/`aria-describedby` 关联。
- 页面工作上下文若需跨路由保持，由应用根 Jotai store 中稳定的页面 atom 持有，仅存活于当前进程。不得写入 localStorage、sessionStorage、IndexedDB、Tauri Store、配置文件、数据库或 URL；语言与主题等批准的设备偏好不受此限制。
- 异步数据与缓存由 TanStack Query 拥有，Jotai 不镜像结果。筛选、范围或每页数量变化时页码归 1；只有查询成功、当前页大于 1 且结果为空时才回退第 1 页，加载、取消、超时和错误不得被解释为空结果。
- Tabler 图标作为唯一通用图标库；存在合适图标时不得手写 SVG、使用字符或 emoji 代替。

### 2.4 文件规模与模块

- Rust 生产/测试文件超过 400 行进入拆分复核，超过 800 行失败；前端生产/测试文件超过 500 行进入复核，超过 1000 行失败；其他人工维护文本超过 500 行进入语义复核，超过 2000 行失败。
- Rust 模块拆为目录时使用 `mod.rs` 保持稳定入口；前端按高内聚页面、组件、hook、状态和适配器组织，不强制创建桶式 `index.ts`。
- 建议阈值不是机械拆分命令。职责单一、场景高内聚且拆分会制造循环依赖时可保留，但必须完成语义复核；硬上限无例外。
- 只有独立发布、生命周期、权限或工具链强制要求时才能新增 crate/包，并记录为何不能属于现有模块。

### 2.5 依赖与异步

- 优先使用标准库和已有直接依赖。新增或主动更新时选取官方 registry 当前兼容的稳定版本，验证 Rust 1.95、Node/pnpm、peer、平台与实际 API/features 后声明完整下界；不得写 `latest`、tag、通配符或依赖全局安装。
- 锁文件固定当前解析结果，不替代最低兼容范围。版本下界变化必须在最低支持工具链和当前锁定环境分别验证。
- 每个后台任务必须有 owner、取消路径、超时/重试上限和关闭回收；不得启动 detached task，锁不得跨越网络、进程、用户交互或其他不受控 `.await`。
- 日志至少包含时间、级别、target 与消息，不记录秘密、令牌、个人数据、完整本机路径或未脱敏业务载荷，不配置网络 exporter。

## 3. 中文代码注释

- 新增或修改的 Rust `struct`、`enum`、`trait`、`type`、`fn`，以及 TypeScript/TSX 的 `class`、`interface`、`type`、`enum`、具名函数、方法和命名函数变量，必须有紧邻声明的中文注释。
- 公共 API 使用适合工具链的文档注释；内部声明可用普通注释。注释说明业务职责、关键约束、非直观思路、错误和副作用，不复述语法。
- 模块顶部说明职责、边界和禁止承担的工作；复杂算法或状态机先写整体思路，再为关键分支解释“为什么”。
- `TODO`、`FIXME`、`HACK` 必须说明问题、触发条件、影响和移除标准；已完成或失效标记立即删除。
- 机械检查只能证明注释存在与归属，内容质量和 core-first 仍需语义审查。

## 4. 文档与事实来源

- `README.md` 是用户入口；`AGENTS.md` 是任务路由；`docs/AGENT_POLICY.md` 保存 Agent 持久策略；`docs/ENGINEERING_RULES.md` 保存工程规则；`docs/RUST_GUI_TEMPLATE.md` 保存当前 Rust/Tauri 基线；`docs/GUI_APP_PROFILE.md` 保存 GUI 身份、选择和设计标准；`docs/RELEASE.md` 保存版本、构建和候选合同。
- 根 `Cargo.toml` 的 `[workspace.package].version` 是当前版本唯一事实源；`.harness/version-state.json` 只保存发布周期与缺陷去重状态；根 `release-notes.json` 只在正式发布准备时创建。
- Product Spec、Product Status、Work Plan、ADR、Changelog、Verification 和技术债只在各自事件真实触发时创建或更新，不生成占位文档。构建结果只进入候选 manifest 和最终回复；完整验收或审计另行留证。
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

### 5.2 构建与候选

- 显式构建运行 Rust workspace 与前端全部非空单元测试。零测试或失败阻断构建；完整命令以当前 Cargo/package 脚本和对应 Skill 为准。
- Windows 普通本地安装试包是开发制品：不创建发布说明或 `release/`，不提交、不安装、不解析候选 E2E/性能，也不声称可分发。
- 正式候选必须来自明确的 clean HEAD。每次在测试/编译前解析当次 E2E 与性能选择；性能启用或渠道要求时先对同一提交运行 release-profile 探针，E2E 启用或渠道要求时只在最终字节形成后运行。
- 性能测量必须隔离并在所有退出路径恢复 window-state；无法定位、恢复或复核时失败。仅可豁免的纯指标失败能在用户明确确认后记为 `waived`，不得改判为通过。
- 最终候选绑定源码提交、目标平台、产物摘要、签名状态、发布说明、测试、性能和 E2E 结果。任何后处理改变字节或运行行为都形成新候选并重新验证。
- mock、stub、中性脚手架、开发预览、源码片段或只调用内部函数的结果不具备候选验收资格。未实际执行一律记录 `Not run` 或 `Unverified`。

## 6. 规则例外

- 硬规则例外必须在实施前获得负责人批准，并在同日 `docs/adr/YYYYMMDD_ADR.md` 记录规则、理由、风险、范围、替代验证和恢复条件。
- 第 2.4 节硬行数上限不接受 ADR 例外。默认规则的局部偏离可在计划或复核中说明，不要求正式 ADR。
- 例外必须尽可能小；临时例外必须有删除条件。人工批准不会把失败或未执行的验证改判为通过。

## 7. 机械检查边界

- 可确定规则可以自动化，但日常开发不因此自动执行所有检查。显式构建运行全量非空单元测试和真实构建；格式、lint、静态、链接或治理检查仅在本次变化、用户要求或发布/渠道门禁需要时运行。
- 自动检查应覆盖 core/adapter 依赖方向、文件上限、临时标记格式、中文声明注释存在性，以及 updater、赞助、全局快捷键、产品统计和远程遥测残留。
- 自动检查失败必须返回非零状态并报告精确位置。业务逻辑归属、注释质量、UI 可用性和最终候选行为仍需语义或真实宿主验证，不能由关键词和行数替代。
