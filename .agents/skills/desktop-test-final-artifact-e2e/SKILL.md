---
name: desktop-test-final-artifact-e2e
description: 通过 Computer Use 对完整真实 GUI 安装候选执行端到端验收；仅在当前构建明确启用或产品/渠道要求时调用。
---

# 最终 GUI 候选 E2E

1. 要求当前候选、manifest、版本、source commit、平台、架构与构建选择完整匹配，且当前构建 E2E 为 enabled 或渠道明确 required。复核 `reviewSelection`、`performanceSelection` 和 macOS 签名选择只来自本次候选 manifest，E2E 不得补问、翻转或替代这些结论。
2. 使用候选自身的 DMG/NSIS 安装路径；拒绝源码片段、开发服务器、调试 no-bundle、旧安装或模拟宿主。
3. 安装前快照测试创建的应用文件、登录项、通知偏好、窗口状态、快捷键和深链接注册；只修改已授权、可恢复的测试状态。
4. 验证安装/首次启动、Logo/标题/单 `v` 版本、侧栏、设置/i18n/主题、主窗口恢复、关闭行为和全部实际页面。
5. 只对 profile 启用能力验证真实托盘、通知、自启、单实例、深链接和快捷键；禁用能力验证运行时缺席。macOS 通知启用时必须先核对候选已签名、公证并 stapled，再在真实安装候选中验证授权状态：仅 `NotDetermined` 请求权限，`Denied`/`Restricted`/请求后仍未授权时必须打开当前 app identifier 的系统通知设置，打开失败可观察；调试结构检查不能替代此场景。所有候选都必须从设置页打开本地 release notes 并与根事实核对，同时确认固定支持页面、导航和专属资源精确匹配仅含设置页的允许集合；联系人不得出现在标题、应用元数据、侧栏或设置页。
6. 运行产品批准的核心成功路径与最高风险失败路径；外部不可逆副作用、生产数据、凭据或支付仍需单独授权。
7. Windows xwin 候选必须在真实 Windows 安装运行后才能改变 `runtimeVerification: Unverified`。未实际执行的平台/显示环境/包格式保持 `Unverified`。
8. 无论结果如何，卸载测试候选并恢复本 Skill 修改的宿主状态，回收所有进程；恢复失败阻断接受并给出人工步骤。E2E 结果不能把 `performanceStatus: failed|Not run|Unverified` 改成通过，也不能把 unsigned 通知候选变成可接受。

记录精确候选、场景、预期/观测、截图、清理、跳过项和剩余风险。此 Skill 不代签人工批准、不发布制品。
