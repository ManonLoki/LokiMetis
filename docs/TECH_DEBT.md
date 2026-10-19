# 已知边界与技术债

- 产品边界以最新 `docs/product_spec/` 为准；完整候选仍需按当次构建与渠道要求独立验收。
- Linux已有真实验证范围：v0.3.11 ARM64 GNOME XWayland的桌宠、Dock、托盘恢复、Hook通知及其它16项必需场景通过；[能力证据索引](LINUX_GPUI_TOOLCHAIN.md#已验证能力与证据索引)保留精确候选绑定。剩余缺口为当前v0.3.13及x64运行、登录自启、混合DPI/热插拔、第三方Agent自行消费Hooks与未测宿主换肤。纯Wayland桌宠全局定位/置顶和Portal/X11后台热键服务仍未接通，空action合同不注册键位，旧global-hotkey 0.8断连回执不能作为注册成功。新版本及未覆盖环境仍须真实候选验证。
- 本机发布历史差异（2026-10-19只读发现）：Git common-dir `agent-first-harness/git-lifecycle.json`的`lastRelease`为v0.3.11/`9eb59c14cfdf93ec8abcc7734b898f0051033ceb`，受保护`.harness/version-state.json`的`last_release`为v0.3.13/`1518ca4c81aae9dcef294cb382a2020827a6dfcb`，maintenance plan返回`Git lifecycle release and version state disagree`。Cargo和版本状态同为0.3.13；本次知识维护不重写任一发布事实。解除条件是单独核实两次发布、当前common-dir登记及tag来源后，通过生命周期/版本专用流程恢复一致，不能以升版或手改状态掩盖差异。
- Hooks上游信息边界：没有轮次/序号的旧终态若在新提示后迟到，无法可靠判断所属；并行同名工具权限请求或同服务器MCP表单缺少调用ID时，保守保留询问至明确新轮次/终态。WorkBuddy FinalStop依据本机5.7.7第一方引擎；更早版本未提供该事件时不能保证完成提示，需对应版本另行核验。详细协议来源见[Hooks协议依据](HOOKS_PROTOCOLS.md)。
- Codex/Grok Stop会在其它Hook可能阻止续跑之前触发，公开协议没有所有门禁通过后的成功信号；本次保守禁用它们的Stop“已完成”提示，仍显示空闲且允许同ID续跑。Grok task_complete是后台作业结束，不用于替代整个Agent成功。后续以第一方新增权威终态契约解除此降级。
- Windows 剩余宿主验收：托盘指针交互、通知横幅、自启登录、真实 Agent／WSL Hooks、Codex／WorkBuddy 换肤、混合 DPI 热插拔及 NSIS 安装仍需对应真实场景，单元测试和开发预览不能替代。
- macOS正式候选当前被既有签名门禁阻断：系统通知enabled要求candidate启用签名，而构建工具尚未实现Developer ID签名/公证，不能用disabled绕过。解除条件为按批准范围实现正式签名并通过相应候选验证；现有local ad-hoc试包不替代该路径。具体门禁见[发布规则](RELEASE.md)。
- GPUI 固定基线当前不提供 updater 服务、通用原生 dialog 与深链接；未来确有需求时独立批准并接入真实实现。当前更新检查零出站。
- 上游传递依赖 block 0.1.6 曾被 Cargo 报告未来 Rust 不兼容；既有 Rust 1.99.0 构建/测试记录不等于未来工具链兼容保证，升级时仍须核对上游修复。空热键接口的未使用项已用带原因的 `allow(dead_code)` 标注（初始化无动作/注册是已确认行为），既有 macOS 与 Windows 生产构建记录无编译警告，各次候选仍独立核对。
- 本次 Computer Use 无法读取系统菜单栏；托盘图标、恢复、退出由用户真实本机检查确认，其余原生窗口和系统状态由工具观察。未来发布候选仍按当次 E2E/渠道要求独立验收。
- Windows 测试构建用测试宿主替换原生桌宠/窗口宿主，`cargo check --tests --target x86_64-pc-windows-gnu` 因此对原生 Windows 宿主模块报告约 40 条 dead_code 警告（生产 Windows 构建已无警告）。若要消除，需要让原生宿主模块在测试配置下不参与编译，并保持真实 HWND 回归可用。
- 2026-10-19 全仓重构检查发现的范围外结构问题（本次未改，均需可执行回归后再动）：
  - `backend/local_index/claude` 与 `grok` 的 discovery/scan/index/signature 为复制后分别演化的实现（取消检查、链接跳过计数等已不一致），宜提取共享的有界目录遍历与签名骨架，以工具差异作参数；合并前须先用回归锁定两侧现行行为。
  - Windows `ShellExecuteW("open")` 在 `native/links.rs` 先初始化 COM，`skins/file_manager.rs` 未初始化；统一为单一 Shell 打开入口会改变后者行为，须在 Windows 真机验证后处理。Linux 下 `open_repository` 固定返回 false，而文件管理器已有可信 `xdg-open`/`gio` 入口，属于产品能力缺口而非重构项。
  - 项目 Rust 源未纳入 rustfmt：按默认配置格式化后总行数约增加 14%，看板已按职责拆出 `overview`、`metric_display` 与 `calls/column_widths`，格式化后均不超过 800 行；`app/mod_test.rs` 格式化后仍约 854 行。引入格式化门禁前须先拆分该测试文件，并核对按源码文本断言的测试。
