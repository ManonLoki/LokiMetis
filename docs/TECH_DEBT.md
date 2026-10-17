# 已知边界与技术债

- 产品边界以最新 `docs/product_spec/` 为准；完整候选仍需按当次构建与渠道要求独立验收。
- Linux原生能力仍有范围内未完成项：KSNI对象创建不等于桌面恢复入口始终存在；Portal/X11后台热键的新服务尚未接线，旧global-hotkey 0.8的断连回执不能作为注册成功；桌宠原生操作、通知权限/点击和窗口恢复尚需真实Linux适配与运行验证。构建/打包工具链不能替代这些能力；方案及解除条件见[Linux工程知识库](LINUX_GPUI_TOOLCHAIN.md)。
- Hooks上游信息边界：没有轮次/序号的旧终态若在新提示后迟到，无法可靠判断所属；并行同名工具权限请求或同服务器MCP表单缺少调用ID时，保守保留询问至明确新轮次/终态。WorkBuddy FinalStop依据本机5.7.7第一方引擎；更早版本未提供该事件时不能保证完成提示，需对应版本另行核验。详细协议来源见[Hooks协议依据](HOOKS_PROTOCOLS.md)。
- Codex/Grok Stop会在其它Hook可能阻止续跑之前触发，公开协议没有所有门禁通过后的成功信号；本次保守禁用它们的Stop“已完成”提示，仍显示空闲且允许同ID续跑。Grok task_complete是后台作业结束，不用于替代整个Agent成功。后续以第一方新增权威终态契约解除此降级。
- Windows 剩余宿主验收：托盘指针交互、通知横幅、自启登录、真实 Agent／WSL Hooks、Codex／WorkBuddy 换肤、混合 DPI 热插拔及 NSIS 安装仍需对应真实场景，单元测试和开发预览不能替代。
- GPUI 固定基线当前不提供 updater 服务、通用原生 dialog 与深链接；未来确有需求时独立批准并接入真实实现。当前更新检查零出站。
- 上游传递依赖 block 0.1.6 被 Cargo 报告未来 Rust 不兼容；当前 Rust 1.99.0 构建/测试通过，未来升级工具链时核对上游修复。空热键接口的未使用项已用带原因的 `allow(dead_code)` 标注（初始化无动作/注册是已确认行为），当前 macOS 与 Windows 生产构建均无编译警告。
- 尚无 upstream-lock；首次工程层升级先初始基线审计。版本门禁与完整升级工具已保留，不伪造来源锁。
- 文件行数验证器尚不能识别已封存的 Archify 生成物：`pnpm run validate` 对 `.archify/architecture-lokimetris-20261011-2240/lokimetris.html` 的 13,885 行报告超限。该文件带 `archify 3.0.1` 生成标记，实际 SHA-256 `bb76374b3cbbec041c220999955ba8345e1b88705c10015dc8969a8612da5f36` 与相邻 `lokimetris.finalize.json` 的工具产物摘要一致；它属于生成物，不应手工拆分或压缩。本次保留验证命令失败事实，中文注释与 core-first 检查通过。后续单独修复生成物的精确登记与摘要校验，不放宽人工维护文件上限，不对整个目录免检。
- 本次 Computer Use 无法读取系统菜单栏；托盘图标、恢复、退出由用户真实本机检查确认，其余原生窗口和系统状态由工具观察。未来发布候选仍按当次 E2E/渠道要求独立验收。
- Windows 测试构建用测试宿主替换原生桌宠/窗口宿主，`cargo check --tests --target x86_64-pc-windows-gnu` 因此对原生 Windows 宿主模块报告约 40 条 dead_code 警告（生产 Windows 构建已无警告）。若要消除，需要让原生宿主模块在测试配置下不参与编译，并保持真实 HWND 回归可用。
