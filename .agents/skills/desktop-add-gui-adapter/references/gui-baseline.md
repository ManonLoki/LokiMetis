# GUI 基线

## 固定结构

- 根 Cargo metadata 必须是 `interfaces = ["gui"]`；workspace 至少包含共享 core 与唯一 `<project-id>_gui`。
- GUI 使用 Tauri 2、Vite、React、TypeScript、Mantine UI、`@tabler/icons-react`、TanStack Router 文件路由、TanStack Query、Jotai、i18next/react-i18next 与 rust-i18n。
- 固定宿主插件是 system-locale、window-state、dialog。根 `[workspace.dependencies]` 声明稳定三段下界，GUI member 只用 `workspace = true`；dialog 前端生产依赖直接声明 `@tauri-apps/plugin-dialog`。
- 中央 Builder 顺序：启用时 single-instance、启用时 deep-link、固定 os、固定 window-state、固定 dialog、启用时 notification、autostart、global-shortcut。托盘从 `.setup(...)` 与 `.on_window_event(...)` 接线，不是插件。
- 主窗口 capability 的 dialog 权限精确为 `dialog:default`，覆盖 message/open/save；禁止 wildcard、deny 混用和额外文件系统权限。
- Harness 不安装或配置应用内版本插件，不提供联网检查、下载安装、阻断入口、相关资源或网络端点；也不建立产品数据上传管线。

### 固定宿主子契约

- system-locale：根依赖固定为 `tauri-plugin-os = "2.3.2"`，GUI member 只用 `workspace = true`。它保持 Rust-only，不安装 `@tauri-apps/plugin-os` 或授予 `os:*` ACL；启动时只通过 `locale()` 取得系统语言，在单一 helper 中规范化 BCP-47，已保存的用户语言优先，未知或空值回退 `en-US`。React、Rust 原生文案、托盘和通知共同消费该结果。固定回归覆盖官方插件来源、单次规范化、英文回退和偏好优先。
- window-state：根依赖固定为 `tauri-plugin-window-state = "2.4.1"`，GUI member 只用 `workspace = true`。它保持 Rust-only，不安装前端包或授予 ACL；只使用 `StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED`，明确排除隐藏、最小化、全屏与装饰状态。插件恢复后必须从 Builder `.setup(...)` 调用可找回性检查：小于 960×640、与当前显示器无交集或 DPI/显示器变化导致几何无效时，恢复 1440×900、最小 960×640、居中并防溢出；合法几何不得覆盖。固定回归覆盖精确 flags、忽略可见性、无效/离屏回退和首启默认值。
- dialog：根依赖固定为 `tauri-plugin-dialog = "2.7.3"`，GUI member 只用 `workspace = true`；前端生产依赖固定为 `@tauri-apps/plugin-dialog = "^2.7.3"`。中央 Builder 只注册一次官方插件，主窗口只授予 `dialog:default`，不得展开子权限、混入 deny/deprecated alias、加入自有 `invoke_handler`，也不得连带安装或授权 fs。固定回归覆盖依赖、唯一有序注册、默认权限集合与无文件系统授权。
- 初始化真实 E2E 观察默认语言与语言切换，分别用合法、损坏、离屏和首启空窗口状态执行重启，并验证原生 dialog；无法观察或恢复宿主状态时失败关闭。性能探针另按发布契约隔离并恢复 window-state 文件。
- macOS 发布默认记录 `macosSigningSelection = disabled`、`macosSigningSource = not-requested`，使用 `--no-sign` 且不探测签名或公证环境；只有已配置、当次请求或渠道硬要求时启用。启用后必须在候选摘要前完成签名、公证与 ticket stapling，禁止中间态或失败后回退 unsigned。`system_notification = enabled` 的 macOS 候选必须选择并完成签名。

## 上游七项初始化配置与产品扩展

`docs/GUI_APP_PROFILE.md` 的唯一 `gui-initialization-config` 代码块依次包含：

1. `system_tray`
2. `system_notification`
3. `autostart`
4. `single_instance`
5. `deep_link`
6. `global_shortcut`
7. `sidebar_mode`

前六项为 `enabled|disabled`，侧栏为 `compact|detailed`。省略侧栏时初始化器写入 `detailed`；字段集合、顺序或值不精确匹配时失败。深链接要求单实例；全局快捷键启用时存在唯一 contract，中性初始化为 `actions: []` 且不注册 OS chord。

上述前六项中每个未选能力都必须从依赖、feature、插件/生命周期、配置、命令、ACL、状态、设置项、翻译键、路由、媒体与测试中缺席。

LokiMetis 的同一代码块在保持上述七字段、相对顺序和值域不变的同时，于 `autostart` 与 `single_instance` 之间保留第八字段 `sponsor_page = enabled`。它是 `docs/GUI_APP_PROFILE.md` 与已接受 ADR 批准的 product-owned 扩展，不是共享 Harness 的第七种条件能力，也不恢复通用赞助模板。该字段只由受保护的产品文档与既有实现消费：`/settings` 底部展示用户提供的微信支付和支付宝双收款码；不得建立 `/sponsor` 路由、侧栏或导航入口、赞助档位、联系人、共享 sponsor template assets，也不得把二维码复制进本 Skill 或其它受管资产目录。

## 窗口与侧栏

- 首次主窗口：1440×900、最小 960×640、`center: true`、`preventOverflow: true`；只恢复尺寸、位置和最大化，损坏或离屏状态回退默认值。
- compact：80px 栏宽、6px padding、36px Logo、22px 图标、56px 菜单项、无折叠按钮。
- detailed：248px 展开、76px 收起、72px/44px Logo、22px 图标；折叠动作只绑定自身按钮，Tooltip 位于右侧且无延迟，AppShell 内容偏移与宽度同源。
- Logo 后显示单个小写 `v` 版本。产品功能项从顶部增长，固定设置入口按规范贴底；固定支持导航只包含设置入口。LokiMetis 的 product-owned 双码区属于设置页内容，不是导航项。

## 设置、语言与页面状态

- `/settings` 永远存在，包含应用/版本、本地更新日志、中英文切换和浅色/深色/跟随系统；不显示联系人、隐私、统计或未配置占位。LokiMetis 另在页面底部保留产品自有 `SponsorPaymentPanel` 展示微信支付与支付宝双码，它只静态展示、不解析或触发支付。
- 通知/自启 Switch 只在相应能力启用时存在，默认关闭；成功使用宿主返回的最终值，失败重读真实状态。
- 可恢复的选项卡、筛选、排序、分页等页面状态放在应用根 Jotai store，仅当前进程保留，不持久化或镜像 Query/core 数据。
- 行内动作绑定实际 Button、ActionIcon、Switch、Checkbox 或链接；Card、Table 行/单元格不得代理子控件动作。

## 设置与本地 release notes

- 固定支持页路由集合精确为 `/settings`，固定支持导航精确包含设置入口；不接受额外固定支持页面、入口或受管专属资源。LokiMetis 经批准的双码图片继续由产品自有 `/brand-support/sponsor/` 路径持有，不进入固定 GUI 支持资源允许集合，也不产生额外页面或入口。
- 设置页固定包含应用、版本和本地“更新日志”按钮，不包含联系人或在线检查更新。窗口标题固定为 `{applicationName} v{version}`。
- 所有 GUI 保留 `src-tauri/tauri.release.conf.json`，正式候选用它把根 `release-notes.json` 映射为 `BaseDirectory::Resource/release-notes.json`；调试构建不传该配置。
- 所有 GUI 固定注册使用 `tokio::fs` 的 `load_release_notes`；Rust 与 React 校验 schema v2、1 MiB、近五版、每类十条、最新在前及本地化，由设置页显示 loading/error/retry。

## 测试不变量

- 结构检查始终验证 os/window-state/dialog 的根/member 依赖、唯一注册、Rust-only 边界（dialog 例外为精确 guest 权限）和非空回归。
- 条件能力按 profile 验证完整实现或彻底缺席；深链接、快捷键、通知、自启和托盘各自保留生命周期、恢复与失败可见回归。
- 始终验证设置页本地 release-notes Rust/React 链路，并要求固定支持页、导航和受管专属资源精确匹配设置页允许集合。LokiMetis 另外验证 `sponsor_page = enabled` 的产品扩展只消费受保护的底部双码组件、i18n 与两张产品图片，且 `/sponsor`、赞助导航、共享 sponsor template assets 和受管二维码资产全部缺席。
- 负向检查拒绝任何应用内更新依赖/config/command/UI/resource 和任何遥测、分析、统计网络管线。
- 初始化真实 E2E 验证窗口状态、侧栏、设置页、所有实际页面和适用宿主能力，并在成功、失败、超时或取消时恢复由测试修改的宿主状态。
- macOS DMG 只在本次签名选择形成的最终字节上验证 Finder 布局；签名启用时验证已签名、公证并 stapled 的字节，默认禁用时验证明确的 unsigned 最终字节。
