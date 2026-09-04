# Tauri GUI React 前端基线

这是 Tauri GUI 适配器的前端基线。

## 固定技术栈

- 使用 React 和 TypeScript 编写前端应用和组件代码。
- 使用 Vite 作为 Tauri 本地前端的开发与生产构建工具；不得把 Vite 开发服务器或远程页面作为打包运行时依赖。
- 使用 Mantine UI（`@mantine/core` 和 `@mantine/hooks`）作为组件和主题基础。只有已批准页面需要时才增加其他 Mantine 包。
- 使用 `@tabler/icons-react` 作为唯一图标库。直接依赖使用经最低直接版本验证的完整三段 caret 下界，组件采用命名导入；菜单、操作、状态、空态及图表周边控件存在适用图标时必须优先选择 Tabler，不得另装图标库或改用手写 SVG、字符、emoji。数据可视化本身不由图标库承担。
- 使用 TanStack Router（`@tanstack/react-router`）及其 Vite 插件建立文件路由和自动 route code splitting；`src/routeTree.gen.ts` 是唯一生成路由树，必须提交但禁止手改，并在 ESLint、Prettier 与中文注释门禁中按根相对路径精确排除。
- 使用 TanStack Query（`@tanstack/react-query`）管理命令支撑及其他异步资源状态，包括请求生命周期、缓存和失效。
- 使用 Jotai（`jotai`）管理确实需要跨组件共享的纯客户端状态。应用根 Provider/store 必须位于不会随 route unmount 重建的壳层；活动选项卡、已应用查询/筛选、排序、分页页码/每页数量及同类可恢复页面工作状态在页面模块顶层创建稳定 atom，并在当前应用进程内跨路由保留。已启用的关闭隐藏和单实例唤醒沿用同一 store，真正退出后由新 store 回到默认值；详细侧栏折叠偏好使用独立设备级存储，不进入该 store。
- 使用 `i18next` 与 `react-i18next` 作为界面文案国际化事实标准；初始化已经交付真实侧栏与设置页，并可能交付赞助页面，因此中性 GUI 脚手架也必须立即接入（见 ADR-20260806-001）。
- 使用 ESLint + `typescript-eslint`、Prettier、Vitest、Testing Library、`jest-dom`、`user-event` 与 jsdom 形成固定静态和测试基线。

实施适配器时为每个直接包声明满足实际 API、Node.js、Tauri WebView、平台和安全约束的最低兼容稳定范围。`dependencies`/`devDependencies` 使用带完整三段下界的 caret（例如 `^1.2.3`），或上游官方明确支持的兼容范围；禁止裸精确版本、`latest`、tag、通配符和无下界范围。确需因上游缺陷或互操作约束精确固定时，先记录硬规则例外与解除条件。

当前已核定的 GUI 前端直接兼容下界如下；写入 `package.json` 时全部保留 caret 兼容范围，不得改成精确锁定：

| 直接包 | 最低兼容范围 |
|---|---|
| `@tauri-apps/api` | `^2.11.1` |
| `@tauri-apps/plugin-dialog` | `^2.7.3` |
| `react` / `react-dom` | `^19.2.8` |
| `@mantine/core` / `@mantine/hooks` | `^9.6.0` |
| `@tabler/icons-react` | `^3.46.0` |
| `@tanstack/react-router` | `^1.170.32` |
| `@tanstack/router-plugin` | `^1.168.35` |
| `@tanstack/react-query` | `^5.102.8` |
| `jotai` | `^2.20.3` |
| `i18next` | `^26.4.0` |
| `react-i18next` | `^17.0.12` |
| `vite` | `^8.2.2` |
| `@vitejs/plugin-react` | `^6.1.1` |
| `typescript` | `^6.0.3` |
| `eslint` | `^10.9.1` |
| `typescript-eslint` | `^8.68.0` |
| `prettier` | `^3.9.6` |
| `vitest` | `^4.1.11` |
| `@testing-library/react` | `^16.3.3` |
| `@testing-library/jest-dom` | `^7.0.1` |
| `@testing-library/user-event` | `^14.6.6` |
| `jsdom` | `^30.0.1` |
| `@tauri-apps/cli` | `^2.11.4` |

`@tauri-apps/plugin-dialog` 是所有 GUI 的固定生产依赖，不按 profile 裁剪。前端仅通过官方包调用 dialog guest API；主窗口 capability 必须且只能用一个 `dialog:default` 覆盖 message、open、save 全部官方对话框类型，不得追加 partial allow、deprecated `ask`/`confirm` alias、deny 项、wildcard 或任何文件系统权限。dialog 返回的用户选择只表示路径值或取消结果；实际读取、写入与业务处理仍需独立批准的窄 adapter/core 能力。

`package.json` 必须以 `engines.node: "^24.15.0 || >=26.0.0"` 和 `engines.pnpm: ">=11.24.0"` 表达当前最低工具范围。不得把旧式精确 `packageManager` 字段当作兼容要求；若生成工具为 Corepack 溯源必须写入该字段，它只属于实际解析元数据，不能替代 `engines` 范围或下界验证。正常 `pnpm-lock.yaml` 固定当前解析版本，但不抬高清单下界。

新增或提高直接下界时，在临时副本的 `pnpm-workspace.yaml` 中设置 `resolutionMode: lowest-direct`，于声明的最低 Node.js/pnpm 环境运行受影响的类型检查、非空单元测试与生产构建。最低版本解析只用于证明下界，不覆盖提交的正常锁文件；正常安装可选择范围内较新的稳定版本。

## 状态所有权

| 状态 | 所有者 |
|---|---|
| URL、路由参数、已验证搜索参数和导航 | TanStack Router |
| 命令支撑的数据、请求状态、缓存、重试和失效 | TanStack Query |
| 仅限本地组件的交互 | React 组件状态 |
| 不具备核心权威性的跨组件客户端/交互状态 | Jotai；详细侧栏折叠偏好由组件与独立 local-storage 键持有，不进入页面会话 atom |
| 当前 UI 语言与切换交互 | Jotai（消费 `i18next` 语言变化事件），持久化到 GUI 适配器本地偏好存储，不进入 Query 缓存或 core |
| 当前主题偏好与切换交互 | Mantine color-scheme context；`light`/`dark`/`auto` 由唯一 color-scheme manager 持久化到 GUI 设备级本地存储，不进入 Jotai、Query 或 core |
| 领域规则、持久记录和权威应用状态 | Tauri 命令背后的共享核心/存储 |

不得把 Query 结果镜像到 Jotai，不得在 atom 中放置持久领域状态，也不得把路由状态用作第二持久层。视图必须从所有权来源派生。

## UI 与架构

- 完整遵守 [`docs/design_standards/README.md`](../../../../docs/design_standards/README.md) 精确命中的标准；[Mantine UI 实施入口](mantine-ui-guidelines.md)只负责指向统一事实源。每个 React 根只挂载一个 `MantineProvider`，主题和语义令牌只有一个入口。
- 优先使用 Mantine 组件、布局原语、焦点行为和主题令牌。自定义组件必须代表 Mantine 组合无法表达的已批准交互或样式需求。
- 保持路由定义和加载器轻量。当预取能防止瀑布请求时，将 TanStack Router 加载器与 TanStack Query 集成，但只保留一个 QueryClient/缓存。
- 只有本地组件状态不足且状态确需跨组件或跨路由保留时才使用 Jotai；atom 应保持小而且按用途命名。URL/search 只用于产品明确批准的可分享导航事实，不得作为本次进程页面会话的隐式持久层。
- 前端代码使用窄而有类型的 Tauri 命令。它不包含业务规则、迁移、平台无关验证或第二套持久存储。
- React event handler、Router loader、Query mutation 和 atom 只管理导航、请求生命周期或纯交互状态；它们不得编排多个命令来决定业务结果。需要条件、重试或状态决策的工作流必须由单个 core 用例通过窄 Tauri 命令暴露。
- React 事件必须绑定在拥有动作的语义控件本身，不得由 Card、`Table.Tr`、`Table.Td` 等父级代理按钮、链接、`Switch` 或 `Checkbox` 动作；父级有自己的独立动作时应隔离冲突传播。表格中的 `Switch` 只能因用户操作该控件而切换，点击所在行或单元格不得切换。
- 页面会话 atom 只保存控件值，不得接入 `atomWithStorage`、localStorage、sessionStorage、IndexedDB、Tauri Store、文件/数据库或 URL，也不得镜像 TanStack Query 数据或 core 权威状态；语言、主题与详细侧栏折叠等已批准的设备偏好沿用各自独立持久化契约。查询范围或每页数量变化时页码重置为 1；路由返回后只有 Query 成功、当前页大于 1 且结果为空时才回退第 1 页并以新 query key 重查，loading/error 与第 1 页空结果不回退。复用品牌包的 `pageSessionState.ts`/`PageSessionState.test.ts` 作为初始化实现与回归基线。
- 在 Tauri 中打包本地前端资产。GUI 下游保留该 Skill 的品牌源资产，运行时只复制所选页面需要的资源；初始化固定建立 `/settings`，`/sponsor` 按 profile 选择存在，`/about` 必须缺席。侧栏布局只从 [`docs/design_standards/tauri_sidebar.md`](../../../../docs/design_standards/tauri_sidebar.md) 取得：compact 为 `80px` 全宽居中竖排菜单且不折叠；detailed 为 `248px`/`76px`、`72px`/`44px`、统一 `22px` 图标，并由 AppShell 拥有折叠状态和同步主内容偏移。产品功能项从顶部向下增长，底部按已选赞助、固定设置生成。Mantine provider 使用 `defaultColorScheme="auto"`、显式 local-storage manager 和唯一 CSS variables resolver；设置页固定显示应用/版本、本地更新日志、语言和 `light`/`dark`/`auto`。标题固定为 `{applicationName} v{version}`。选择 Sponsor 时按 manifest 原样复制完整媒体并适配亮暗主题；QQ 仅作为 Sponsor 的赞助支持联系方式，不得进入标题、应用元数据、侧栏或设置页。未选赞助页不得有路由、导航入口或运行时资源。
- `releaseNotesResource.ts` 固定调用 `load_release_notes` Tauri 命令并把 IPC 值从 `unknown` 严格收窄；`SettingsPageTemplate` 使用该加载器，更新日志弹窗提供 loading、失败与自身绑定的重试，不显示原始本机错误。设置页的五版/十条裁剪只是防御性显示上限，不能替代 Rust、发布准备和构建资源门禁；不得改用通用前端文件系统插件或编译时假数据。
- 应用启动时使用 Tauri `tauri-plugin-os` 的 `locale()` 探测系统语言初始化 `i18next`；缺少对应资源时回退英文。界面必须提供 Mantine 组件实现的可发现语言切换入口，切换后的选择通过 GUI 适配器的本地偏好存储持久化，不写入 core；选择系统托盘时，还需通知 Rust adapter 无需重启地刷新当前托盘菜单标签。
- 翻译资源按功能域拆分文件并使用稳定的层级 key（如 `settings.language.label`），不得在组件中拼接原始中文/英文字符串；核心领域错误标识作为 key 的一部分由前端映射为当前语言文案，业务判断本身不得放入翻译资源或组件。初始化把品牌包的中英文 JSON 注册为 `brandSupport` namespace，仍复用唯一 i18next 实例和语言偏好；缺少对应系统语言资源时回退英文。
- 初始化设置页只包含固定的应用信息、语言、主题和按 profile 启用的宿主能力开关；不得增加未声明的网络状态或远程数据管线。

侧栏功能项以 `TablerIcon` 组件注入，所选赞助/固定设置由模板提供 Tabler 组件，不生成 About 图标或入口。Testing Library 按 `tauri-gui-sidebar-compact-80-v1` 锁定全宽居中、无固定 `em/ch` 盒和无折叠，按 detailed 标准锁定默认 `248px` 展开、`72px` Logo、`22px` 图标、自身折叠按钮、身份父级无动作、`76px`/`44px` 收起、AppShell 双宽度同步、图标-only + Tooltip 与偏好持久化；初始化 E2E 再从真实本机调试窗口复核可见结果。

## 工具链与质量门禁

- `tsconfig` 至少启用 `strict`、`noUncheckedIndexedAccess`、`noFallthroughCasesInSwitch` 和 `isolatedModules`；不得用大范围排除、`skip` 脚本或独立宽松配置绕过产品源码。
- ESLint 至少拒绝显式 `any`、非空断言和产品源码直接 `console.*`。确需桥接第三方无类型边界时先收窄为 `unknown` 并在单一边界验证；测试或统一日志 sink 的局部例外必须写成精确文件规则。
- 前端清单提供稳定的 `dev`、`build`、`test`、`typecheck`、`lint` 和 `format:check` 脚本；`lint` 必须同时运行 ESLint 与 [TypeScript AST 中文注释检查器](check-typescript-chinese-comments.cjs)，项目 validator 也独立调用同一检查器，避免只改脚本即可绕过。
- TypeScript Compiler AST 中文注释检查器及其 [专项测试](check-typescript-chinese-comments.test.ts) 是 GUI 下游保留的治理资产。复制到项目自有工具目录后只修改导入路径和扫描根，不扩张到局部变量/普通匿名回调，也不得增加自动批量注释功能。
- 门禁跟踪直接及后置命名/默认导出的箭头函数组件与 hook；`test`/`it` 只在 `.test.*`、`.spec.*`、`test/`、`tests/`、`__tests__/` 或显式从 `vitest` 导入的上下文中视为测试场景，避免业务同名调用误报。
- Testing Library 通过角色、可访问名称和用户交互验证可观察行为；不得用 DOM class、实现细节或大快照代替语义断言。测试运行环境使用 jsdom，并在需要 Mantine provider、Router、QueryClient 或 i18n 时装配真实最小 provider。初始化回归必须覆盖两种侧栏模式及详细模式持久化、功能区、按选择生成的底部顺序、固定设置内容与翻译键、设置固定路由、固定本地发布说明，以及赞助路由的存在与缺席；同时拒绝 About 字段、入口、路由和资源。选择 Sponsor 时再覆盖对应主题、媒体、QQ 赞助联系方式与内容约束；未选时锁定没有入口、路由和运行时资源。行内交互还要分别点击语义控件与父级周围区域，证明父级不会代理按钮、链接、`Switch` 或 `Checkbox`。页面会话回归用同一根 store 证明选项卡、查询/筛选、排序和分页跨 route unmount/remount 保留，用新 store 证明进程重启回到默认值，并覆盖成功空页回退、loading/error 不回退及第 1 页不循环。视频模板测试必须覆盖 controls、无 autoplay、字幕与文字稿。

## 配置、日志与产物

- `development`、`test`、`release` 是统一逻辑 profile；只有真实下游需要前端公开配置时才建立对应受管配置。全部 Vite 前端变量都视为最终用户可读，禁止放入密钥、令牌、Cookie、密码或其他凭据。
- 已批准的前端配置必须经过类型和边界校验，汇总为只读/冻结的单一配置对象。组件、route、Query、atom 和业务模块不得直接散落读取 `import.meta.env`；没有真实配置项时不创建占位 BaseURL、超时或环境文件。
- 产品源码不直接使用 `console.*`。真实需要前端日志时，使用稳定的 `level/scope/event/context` 结构、严格字段 allowlist 和脱敏；Tauri 下游通过窄命令把清理后的诊断事件汇入 Rust `tracing` 本地文件日志，不建立浏览器端第二套持久日志或任何远程导出管线。
- Release 前端构建必须拒绝 source map、开发/测试 endpoint、debug/info 哨兵、本机绝对路径、未脱敏秘密和未经批准的 `console.debug/info`。该静态扫描针对最终 `dist`，完成后才允许 Tauri 打包；它不替代日志行为测试、秘密扫描或真实产物验收。
- 前端不得持有远程服务或发布者长期秘密。可选支持能力只消费经 Tauri 窄命令映射的类型化结果；生产 `dist` 必须拒绝秘密实值、固定未批准 endpoint 和禁用能力的残留配置。

## 必需证据

- 使用 pnpm，记录 `engines` 兼容范围与实际运行版本，提交正常解析的 `pnpm-lock.yaml`；另保存最低直接版本解析及最低 Node.js/pnpm 环境通过相关检查的证据。
- 日常开发只运行本次前端变化需要的非空单元/回归测试。显式构建运行 `package.json` 与锁文件声明的完整非空单元测试套件和锁定 `pnpm build`；格式、类型、lint 和最终 `dist` 静态扫描只在本次变化需要、用户明确要求或发布/渠道硬要求时运行。
- 测试路由未找到/错误边界、Query 加载/错误/重新获取/失效、Jotai 转换、纯键盘使用和相关无障碍语义。
- 测试默认语言探测与回退、设置页应用/版本/本地更新日志、语言/三态主题切换的渲染与持久化、所选侧栏图标/文字/顺序与版本、设置固定路由、赞助路由按选择存在或缺席、About 零残留，以及缺失翻译 key 时不泄漏原始 key 给用户。选择托盘时，Rust/真实宿主测试另锁定托盘语言刷新和不泄漏 `tray.*` 原始键。
- 固定测试 `load_release_notes` 命令名、IPC 畸形/越界拒绝、候选资源加载成功、失败与重试、近五版/每类十条及关闭后复用；正式候选 E2E 必须从实际安装包设置页显示当前 JSON，注入测试夹具不构成打包证据。
- 发布阶段验收期间，在已打包或发布模式 Tauri 应用中使用真实构建前端完成已批准关键流程。

## 推荐边界

固定技术栈同时固定 Vite、pnpm、`@tabler/icons-react`、ESLint/`typescript-eslint`、Prettier、Vitest 与 Testing Library。它不预选模式定义/验证库、表单库、图表绘制库、网络 client 或持久化方案；产品诊断只使用本地 `tracing` 文件。只有真实下游需求使其他库选择成为必要时才推荐，并应用依赖准入和验证规则。

`i18next`/`react-i18next` 同属固定技术栈，不参与“是否采用”的推荐；只有具体已支持语言列表、翻译文案内容和资源目录组织是项目特定选择。

替换任何固定技术栈库都必须形成硬规则例外 ADR，其中包含未满足的约束、风险、范围、替代证据和恢复/迁移标准。
