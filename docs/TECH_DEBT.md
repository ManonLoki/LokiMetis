# 已知边界与技术债

- 产品边界以最新 `docs/product_spec/` 为准；完整候选仍需按当次构建与渠道要求独立验收。
- 桌宠首图测试revision混合计数：2026-10-23 [Linux AMD64首次执行](https://github.com/ManonLoki/LokiMetis/actions/runs/39911519002/job/120275382463)中，`first_image_retry_waits_for_changed_service_catalog`在首图和页面断言通过后观察到pet_settings_revision=2、预期1；同源码完整重跑通过。该全局revision同时统计首图偏好与首次窗口位置写，`settle_catalog`没有排空position_task，推进GPUI假时钟时可能让两笔独立写入先后完成；此为源码支持的时序解释，未独立复现第二笔事务。解除条件为隔离几何基线或显式排空位置防抖及回执，精确核对首图与位置落盘、重复刷新无额外提交，再通过相关模块及Linux全量回归；本轮重试不改写首次失败。
- Windows异步界面测试等待预算：2026-10-22 [Actions首次执行](https://github.com/ManonLoki/LokiMetis/actions/runs/39765773399/job/119780731065)中，`hook_directory_controls_preserve_tool_drafts_and_disable_unsaved_write`回执等待超时；同源码完整重跑通过。现有[`settle`](../loki_metris_gui/src/app/monitor/hooks/tests.rs)固定500轮、每轮sleep 2ms，推进GPUI假时钟不能代表真实Tokio文件线程和全局设置写锁已完成；负载下的等待预算脆弱性尚未修正。解除条件为改用有界真实时间deadline、保留真实落盘和跨标签归属断言，并通过Hooks模块及Windows全量回归；本次重试不将首次失败改写为通过。
- Ubuntu 26应用中心本地deb集成：本机snap-store 1448（`d7d3fbe8`）已安装即禁用本地deb按钮，不比较版本；安装详情页不接入产品图标，已安装页的stock图标也受Snap主题目录可见性限制。包已补标准AppStream YAML目录，实际升级后在应用中心的发现/卸载仍待对应候选验证。上游限制及来源见[Linux打包合同](../.agents/skills/desktop-build-gpui-release/references/linux-packaging.md)；解除条件为上游支持本地deb版本比较、可读取本地图标，或另行批准软件源/商店渠道。不改包名伪装新应用、不写Snap私有目录、不自动配置外部软件源。
- Linux已有真实验证范围：v0.3.11 ARM64 GNOME XWayland的桌宠、Dock、托盘恢复、Hook通知及其它16项必需场景通过；[能力证据索引](LINUX_GPUI_TOOLCHAIN.md#已验证能力与证据索引)保留精确候选绑定。剩余缺口为v0.3.13及后续版本、x64运行、登录自启、混合DPI/热插拔、第三方Agent自行消费Hooks与未测宿主换肤。纯Wayland桌宠全局定位/置顶和Portal/X11后台热键服务仍未接通，空action合同不注册键位，旧global-hotkey 0.8断连回执不能作为注册成功。新版本及未覆盖环境仍须真实候选验证。
- Hooks上游信息边界：没有轮次/序号的旧终态若在新提示后迟到，无法可靠判断所属；并行同名工具权限请求或同服务器MCP表单缺少调用ID时，保守保留询问至明确新轮次/终态。WorkBuddy FinalStop依据本机5.7.7第一方引擎；更早版本未提供该事件时不能保证完成提示，需对应版本另行核验。详细协议来源见[Hooks协议依据](HOOKS_PROTOCOLS.md)。
- Codex/Grok Stop会在其它Hook可能阻止续跑之前触发，公开协议没有所有门禁通过后的成功信号；本次保守禁用它们的Stop“已完成”提示，仍显示空闲且允许同ID续跑。Grok task_complete是后台作业结束，不用于替代整个Agent成功。后续以第一方新增权威终态契约解除此降级。
- 通用GPUI开发watcher的Windows后代回收缺口：`.agents/skills/desktop-add-gpui-adapter/scripts/gpui_dev.mjs`在leader关闭后把Windows句柄直接标为gone，子孙进程仍可能存活；对应真实后代回归跳过Windows。解除条件为接入真实进程所有权机制并在Windows验证leader先退出的回收路径。当前产品`pnpm dev`使用独立`scripts/dev.mjs`，本次Linux候选不消费该通用watcher；不得据此声称Windows后代回收已验证。
- Windows 剩余宿主验收：托盘指针交互、通知横幅、自启登录、真实 Agent／WSL Hooks、Codex／WorkBuddy 换肤、混合 DPI 热插拔及 NSIS 安装仍需对应真实场景，单元测试和开发预览不能替代。
- macOS Developer ID 签名、公证与 stapling 已接入，原“构建工具尚未实现”的阻断已解除，[2026-10-22 渠道记录](verification/20261022_verification.md)保留当次 App/DMG 的 Accepted 与 Gatekeeper 证据。后续 candidate 仍受[正式签名门禁](RELEASE.md)约束，local ad-hoc 包不代表系统通知或渠道信任通过。
- GPUI 固定基线当前不提供 updater 服务、通用原生 dialog 与深链接；未来确有需求时独立批准并接入真实实现。当前更新检查零出站。
- 上游传递依赖 block 0.1.6 曾被 Cargo 报告未来 Rust 不兼容；既有 Rust 1.99.0 构建/测试记录不等于未来工具链兼容保证，升级时仍须核对上游修复。空热键接口的未使用项已用带原因的 `allow(dead_code)` 标注（初始化无动作/注册是已确认行为），既有 macOS 与 Windows 生产构建记录无编译警告，各次候选仍独立核对。
- 早期 Computer Use 未能读取系统菜单栏，相关托盘图标、恢复、退出结果当时由用户真实本机检查确认；这段历史不表示当前工具始终无法读取菜单。工具可读取应用菜单也不能替代真实托盘点击或各平台候选验收，未来发布仍按当次 E2E/渠道要求独立核对。
- Windows 测试构建用测试宿主替换原生桌宠/窗口宿主，`cargo check --tests --target x86_64-pc-windows-gnu` 因此对原生 Windows 宿主模块报告约 40 条 dead_code 警告（生产 Windows 构建已无警告）。若要消除，需要让原生宿主模块在测试配置下不参与编译，并保持真实 HWND 回归可用。
- 2026-10-19 全仓重构检查发现的范围外结构问题（本次未改，均需可执行回归后再动）：
  - `loki_metris_host/src/local_index/claude` 与 `grok` 的 discovery/scan/index/signature 为复制后分别演化的实现（取消检查、链接跳过计数等已不一致），宜提取共享的有界目录遍历与签名骨架，以工具差异作参数；合并前须先用回归锁定两侧现行行为。
  - Windows `ShellExecuteW("open")` 在 `native/links.rs` 先初始化 COM，`skins/file_manager.rs` 未初始化；统一为单一 Shell 打开入口会改变后者行为，须在 Windows 真机验证后处理。Linux 下 `open_repository` 固定返回 false，而文件管理器已有可信 `xdg-open`/`gio` 入口，属于产品能力缺口而非重构项。
  - 项目 Rust 源未纳入 rustfmt：按默认配置格式化后总行数约增加 14%，看板已按职责拆出 `overview`、`metric_display` 与 `calls/column_widths`，格式化后均不超过 800 行；`app/mod_test.rs` 格式化后仍约 854 行。引入格式化门禁前须先拆分该测试文件，并核对按源码文本断言的测试。
- 设备恢复导出/导入（`*_recovery`、`DeviceRecoveryPreview`、`random_bytes`）在生产构建中没有调用方，仍会产生未使用警告。[当前规格](product_spec/20261027_product_spec.md)明确保留core、应用服务与存储层恢复API且不提供GUI入口，因此不能仅为消除警告删除。后续产品若调整保留范围或新增调用入口，再同步清理或补齐实际调用与回归。

- Cursor系统加密登录的跨平台后端：2026-10-27只接入并核对macOS Cursor safeStorage v1/v10（系统Keychain与CommonCrypto）。Windows的Local State/DPAPI/AES-GCM和Linux的Secret Service/KWallet尚未接入，加密包装返回CredentialLocked；旧明文登录继续使用。解除条件为核对对应发行的实际格式，在已批准的OS秘密范围内实现受限驱动，并分别完成目标平台的合成错误路径和真实账户读取验证，不能直接发送密文、猜测密钥或以macOS结果代替。
