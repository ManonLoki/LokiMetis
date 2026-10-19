---
name: desktop-add-gpui-autostart
description: 为明确启用自启的 GPUI 下游接入原生 OS 状态开关、串行读写和失败恢复；支持初始化及已有 GUI 的受限迁移，不自动注册登录项。
---

# 接入 GPUI 开机自启

仅根 Cargo 为 GPUI GUI 且 profile/本次批准选择启用自启时适用；Tauri 使用原自启 Skill。完整读取 [原生能力合同](../desktop-add-gpui-adapter/references/native-capabilities.md) 的自启章节与 [依赖基线](../desktop-add-gpui-adapter/references/dependency-baseline.md)，复用 adapter 拥有的真实实现与模板，不复制第二套驱动。

## 执行

1. 初始化复用已确认选择与上下文；已有 GUI 新增能力在任何写入前进入 `$desktop-implement-change` 和适用版本门禁。确认项目身份、目标平台、profile、现有设置/原生生命周期；未选择时不接入。批准提供能力不等于批准立即注册登录项。
2. 初始化由原 GPUI renderer 按 `autostart: enabled` 条件生成；已有 GUI 不能调用拒绝覆盖 GUI 的 `addGpuiAdapter`。使用 [纯 renderer](../desktop-add-gpui-adapter/scripts/gpui_adapter_files.mjs) 和 [能力模板](../desktop-add-gpui-adapter/scripts/gpui_native_capabilities.mjs) 计算相同身份/其他选择下 disabled→enabled 的差集，只把该差集合并进当前实现，不重新生成整个 GUI、core、Logo、业务页面或打包配置。
3. 差集包含 GUI Cargo、设置接线、双语键、native 自启模块；仅原先没有 native 模块时补主入口和模块装配，否则在已有生命周期加入自启。根 workspace 只添加确需依赖，成员复用 workspace；其他能力/依赖保留。按 `$desktop-manage-dependencies` 复核当前兼容 API 与最小 features，不直接盲用模板旧版本。
4. 这是受限的逐处实施流程，没有自动覆盖已有 GUI 的迁移命令。受影响文件存在产品定制时先读其真实执行路径，按职责合并并运行对应回归；无法安全合并则停止该写入，报告具体冲突。profile 只更新已批准的自启选择与机制事实，保留其余字段、Logo 确认、快捷键动作和产品说明，不覆盖整份文档。
5. OS 当前用户登录项是权威，后台串行查询/写入并复读；UI 支持 unknown/pending/error/retry，初始化只读，不在渲染或事件线程做阻塞 I/O。Windows 限 HKCU、保护路径编码；主业务和 core 不持有自启状态。沿用真实生命周期 owner 与退出回收，不把成功请求当实际注册状态。
6. 在隔离夹具覆盖查询失败、操作失败、重试与连续切换；受影响的真实 Rust/设置回归证明其他能力、业务、身份和资源保留。当前宿主切换前保存登录项完整可恢复事实（原存在性、启动目标/参数、平台开关，必要时原 plist/当前用户注册表值），不只是布尔状态；无法取得快照则不执行切换。结束或失败先回收 owned 进程/worker，再恢复原字节/值并复读，只触及本应用登录项，不把安装包路径改成本次 debug 路径；未运行宿主验证明确 `Unverified`，不自动构建最终候选。

## 输出

报告新增或迁移路径、依赖与 profile 差异、实际 OS 状态/恢复、测试及未验证平台。源码接入不证明登录项有效；无法恢复原状态不得宣称宿主验证完成。
