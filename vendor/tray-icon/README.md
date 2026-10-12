# Windows 托盘 HRESULT 兼容层

来源为 `provenance.json` 登记的 tray-icon 0.26.1 官方原包，26 个文件逐字节保存在 `upstream/`，包含全部许可证和 SPDX。完整来源清单、唯一替换模板及生成结果均经固定 SHA-256 校验；构建仅写入 Cargo OUT_DIR。复用现有 `gpui-patch-support` 生成器，不新增运行时依赖。

Windows 实测 `Shell_NotifyIconGetRect` 返回 `1`，原库只接受 `S_OK`，导致已登记托盘被判断不可恢复，关闭主窗口退出，鼠标事件也被丢弃。补丁采用 [SUCCEEDED 的非负 HRESULT 合同](https://learn.microsoft.com/en-us/windows/win32/api/winerror/nf-winerror-succeeded)，与 [微软通知图标示例](https://github.com/microsoft/Windows-classic-samples/blob/main/Samples/Win7Samples/winui/shell/appshellintegration/NotificationIcon/NotificationIcon.cpp) 一致；保留原 HWND/uID 和可选 GUID，不增加身份迁移。上游正确接受全部成功码后可移除此补丁。

人工维护范围为 shim、补丁和查询回归测试；上游文档、crate 根属性和构建环境提示保持原有行为。shim 省略上游示例与可选序列化测试的 dev-dependencies；当前产品未启用 tray-icon 的 serde 特性，回归直接复用工作区已解析的运行时依赖，原始 Cargo 清单仍完整保存。维护验证限定 `--lib`；将来启用 serde 测试或上游 winit 文档示例前，需另行恢复相应开发依赖。

相关验证：

```text
pnpm exec node vendor/gpui-patch-support/verify-source.mjs vendor/tray-icon
pnpm exec cargo test -p tray-icon --lib
```

注入测试覆盖完整标识传递、零及正成功码保留矩形、负错误码拒绝结果；不能替代实际 Windows 隐藏与托盘恢复验证。
