# 变更记录

> 记忆日期：2026-09-13

## Changed

### 兼容皮肤应用确认

- `change_id = BUG-LEGACY-SKIN-REDUNDANT-APPLY-CONSENT-20260913`
- `required_version = 0.3.6`
- 按用户确认，已在资源库中的兼容皮肤点击“应用”后直接进入安装流程，不再重复显示第三方脚本信任弹窗；导入时的来源信任确认、后端执行标志及宿主重启风险确认保持独立。

## Fixed

### 换肤时 Tokio 工作线程栈溢出

- `change_id = BUG-SKIN-TOKIO-STACK-OVERFLOW-20260913`
- `required_version = 0.3.7`
- 换肤的多层异步 CDP 验证链在调试运行时曾使默认 2 MiB 工作线程栈溢出；GUI 在首次使用 Tauri 异步运行时前安装 16 MiB 工作线程栈的运行时，以避免该路径直接中止进程。完整 GUI 注入仍须在真实候选上核验。

### ChatGPT 换肤端口归属误判

- `change_id = BUG-CODEX-CDP-MULTI-LISTENER-20260913`
- `required_version = 0.3.5`
- macOS ChatGPT 主进程与其 Computer Use 子进程共同监听同一回环 CDP 端口时，换肤不再因监听 PID 数量大于一而误判端口归属失败。每个监听者仍须属于同一已验证官方宿主进程树，混入其它进程或无法确认父子关系时拒绝注入。
- 移除换肤启动和重启路径中不需要的 `--remote-allow-origins=*` 参数；当前 CDP 客户端使用不带 `Origin` 的 WebSocket 握手。

## Verification

- 换肤模块 156 项 Rust 单元与回归测试通过。
- 在当前运行的 ChatGPT 上对修正后的归属函数进行了只读实测，共用调试端口的两个监听者均通过同一进程树校验。
- 未运行 GUI 应用、实际皮肤注入或安装候选验收。
- 兼容皮肤交互的完整前端回归与发布候选验收以本次发布实际运行结果为准，未在本条记录中预先声明通过。
