# 既有 GPUI GUI 的原生能力增量接线

仅在终端下游已有 GPUI GUI，且当前请求批准新增、调整或禁用原生能力时读取。沿用 `$desktop-implement-change` 的分支、版本、相关回归与提交上下文；初始化继续使用原 renderer，自启交 `$desktop-add-gpui-autostart`。

## 事实与差异

先核对根 Cargo 的框架、GUI member、目标平台、当前 profile，以及实际入口、设置、设备偏好、服务 owner、关闭和退出回收路径。产品实现优先于中性模板。通知事件、额外托盘菜单、快捷键动作/键位必须来自已批准需求，能力开关本身不提供这些产品事实。

当前模板托盘和后台热键仅支持 macOS/Windows；含 Linux 的目标组合不能靠改 profile 或忽略 renderer 错误启用。独立 Linux 扩展按 [Linux 原生宿主方案](linux-native.md) 接线和验证。深链接仍 unavailable/disabled，updater 与通用原生 dialog 不由本说明启用。实际新增依赖按 `$desktop-manage-dependencies` 与项目当前 API/版本处理，不按 [模板依赖快照](dependency-baseline.md) 降级产品。

[add-only helper](../scripts/add_gpui_adapter.mjs) 明确拒绝已有 GUI，不得为增加单项能力重跑它，也不恢复全新工作区 wrapper。可用 [纯 renderer](../scripts/gpui_adapter_files.mjs) 与 [能力模板](../scripts/gpui_native_capabilities.mjs)，显式传入当前已确认身份、Logo、目标平台和全部选择，只改变本次能力值，比较两份内存 Map 识别触点；原生根/member 依赖另对照 `nativeDependencies`。Map 差异是人工合并参考，没有自动迁移或覆盖能力。

不得整份写回 renderer 中的 `package.json`、GUI profile、打包配置、Logo、支持页、产品页面或偏好。只合并本次批准的字段/机制，保留其他九字段、身份证据与产品合同。共享 native owner、Windows 窗口桥、Global/Task、依赖和测试仍有消费者时保留；禁用模板缺文件不代表可以整文件删除产品定制。无法安全合并的重叠内容停止写入并报告。

## 系统通知

对照 [通知入口](../assets/gui/src/notifications.rs)、[通知回归](../assets/gui/src/notifications_test.rs)、[主入口](../assets/gui/src/main.rs)、[偏好](../assets/gui/src/preferences.rs)、[设置](../assets/gui/src/app/settings.rs) 与 [设置回归](../assets/gui/src/app/settings_test.rs)，并检查 `app/mod.rs` 的条件测试模块和双语词典接线。

只建立默认关闭的应用偏好和有界提交入口，不在启动或切换设置时请求权限、发送测试通知。沿用与打包身份一致的 `set_app_identity`；偏好保存复用已有 writer，设置动作归 Switch 自身。产品事件规则留在 core，由批准路径提交通知。

模板结果只有 Disabled、InvalidPayload、Submitted；Submitted 不证明授权或送达。当前 actions 为空，没有点击回执/恢复窗口的现成流程；需要时先按 [接口快照与官方 API 边界](native-capabilities.md) 核实并实际实施。macOS 横幅依赖打包身份与系统许可，Windows 需要稳定身份，Linux 需要会话通知服务。

禁用时移除对应提交调用、模块接线、设置控件、专属词典、偏好运行时字段与专属测试；保留其他设置、产品测试和设备数据，不删除整份用户偏好目录。

## 系统托盘

对照 [托盘](../assets/gui/src/native/tray.rs)、[原生 owner](../assets/gui/src/native/mod.rs)、[窗口壳层](../assets/gui/src/app/mod.rs) 与能力模板。增量涉及条件依赖、模块、由当前 Logo 派生的 `assets/tray.rgba`、双语标签、服务创建、轮询、语言刷新及关闭回调。

原生对象只在当前 UI loop 创建；复用唯一 Global/Task 和退出回收路径。固定宿主动作是显示窗口/退出，只有托盘实际创建成功才关闭隐藏；失败保留正常关闭。额外菜单按批准需求映射宿主动作或单个 core 用例。Windows 恢复遵守现有句柄所有权和前台限制。

禁用时移除托盘专属依赖、图标、菜单、语言刷新、事件和关闭隐藏接线，证明最后窗口关闭仍退出。共享服务/窗口桥保留；已有产品关闭合同先核对，不能用模板替换整个回调。

## 全局快捷键

对照 [快捷键服务](../assets/gui/src/native/shortcuts.rs) 及能力模板的模块、service、轮询与退出接线。模板仅提供稳定 ID、编译期函数 dispatcher、Pressed-only 分派、部分失败回滚和 owned 注销。中性能力为空动作、零默认键位、零 OS 注册和零占位界面。

新增产品动作确认用途、初始键位或未绑定、固定/可编辑、界面位置与安全验证方式。业务动作调用 core，不能把字符串解释成脚本或未批准操作。配置值和实际注册状态分别建模；注册/回滚失败可见，只清理 owned 绑定。

模板尚无完整可编辑绑定界面、权威持久化和逐项编辑状态流程；需要时实际实现与测试，`Service::register` 不代表完整成品。[初始化 profile 解析器](../scripts/gpui_profile.mjs) 只接受九字段和空动作合同，renderer 也只生成空合同；不得用它们清空或否定已有产品非空合同，后者按真实 schema、dispatcher、持久化和回归验证。

禁用前释放 owned 绑定，再移除专属注册入口、事件、service/依赖、界面和合同块；保留其他原生服务、共享 owner 与产品定制。

## 回归与返回

只运行本次实际变化需要的非空回归：

- 通知：关闭/非法载荷零投递、有界提交、控件自身点击、父级不代理、切换不自动发送，以及受影响的偏好保存。
- 托盘：双语回退、创建失败正常关闭、创建成功关闭隐藏与恢复/退出、禁用后最后窗口关闭退出。标签单元测试不证明托盘可点击。
- 快捷键：空合同零 manager/注册、重复/非法输入、Pressed-only、部分失败与回滚失败保留所有权、退出清理；产品动作再验证实际 dispatcher 和已实现的编辑/持久化。
- 共同证明其他能力、业务路径、身份、词典、资源和生命周期保留。修改 renderer 才运行相关保留 Node 回归，日常下游修改不恢复已删除初始化 E2E 或自动创建候选。

宿主操作按当前授权和适用检查执行，保存并恢复测试状态，回收 owned 进程/任务。通知横幅、托盘交互与非空安全按键分别取证；Map、源码、单元测试不冒充真实宿主结果。

返回同一实施流程，报告批准范围、实际 profile/依赖/代码差异、产品定制保留、本次回归、宿主恢复及未验证行为。
