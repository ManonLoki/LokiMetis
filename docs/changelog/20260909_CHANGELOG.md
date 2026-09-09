# 变更记录

> 记忆日期：2026-09-09
>
> 本次变化分类为 `maintenance`；产品版本和所需版本均保持 `0.2.19`，不重置发布周期。

## Changed

### 同步上游 Harness 工程层

- `change_id = HARNESS-UPGRADE-20260909`
- `required_version = 0.2.19`
- 上游基线为版本 `202609090208`、提交 `4fb45f1309f2a7d8886853c4f5980f3d0e9e4ff4`；隔离渲染候选包含 139 个适用工程文件，并以 `.harness/upstream-lock.json` 建立首份可追溯基线。
- 引入 base-100 Minor/Patch 进位、受管开发环境的缺失安装与低版本升级、逐发布语义审查、`gui-release-v2`、macOS 签名意图优先、macOS 通知权限恢复，以及候选证据整组 staging、复算和原子提交合同。
- 更新发布 Git helper，使具名分支、HEAD、隔离 index、冻结 patch 与 hook 结果保持逐字节一致；更新 DMG、本地发布说明、性能和候选验收门禁。
- LokiMetis 的八字段 GUI profile、设置页底部微信支付/支付宝双码、固定 GitHub opener、无版本窗口标题、业务路由、桌宠/换肤/托盘扩展，以及已批准的本地文件、官方进程和回环 CDP 能力均作为产品差异保留。

## Removed

- 删除旧 Harness `assets/brand-support/` 模板树 38 个文件、旧 `settings-and-sponsor-pages.md` 和被替代的 `inspect_dmg_layout.py`；新增身份中立的 `assets/gui-support/` 工程资产，不恢复独立 `/sponsor` 页面、赞助导航或共享二维码模板。
- 产品源码中的两张收款码与项目专属 `codex-skin-generator` Skill 不属于上述删除范围，内容摘要保持不变。

## Security

- 所有权清单在通用 Skill 规则前增加 `.agents/skills/codex-skin-generator/** = protected`，防止未来上游同名路径覆盖项目专属能力。
- macOS 通知拒绝、受限或请求后仍未授权时，只允许 Rust 通过固定系统设置前缀和当前应用 identifier 打开通知设置；WebView 不能注入 URL 或 bundle identifier。
- macOS 候选先锁定签名意图再探测环境；通知启用时必须完成签名、公证、stapling、Gatekeeper 与最终验证，失败不得退回 unsigned。

## Verification

- 上游 `python3 -B scripts/validate_harness.py` 通过：62 个必需文件、30 个 Skills 及当前 Harness 契约。
- 下游升级器与三个工程检查器共 68 项测试全部通过；版本、发布 Git、发布说明、DMG 与性能共 71 项测试全部通过。
- 环境恢复门禁共声明 77 项：57 项通过，1 项 Windows Git Bash 用例和 19 项 Windows PowerShell 用例因当前 macOS 宿主跳过；macOS xwin 18 项全部通过。
- 3 个变更 Shell helper 的 `bash -n`、所有变更/新增 JSON 解析，以及除本文件外 35 个变更 Markdown 文件的 26 个本地链接检查通过。
- 升级后只读计划加载 139 个基线条目，确认零待应用操作、零冲突、零路径问题。
- 未运行 Tauri 构建、安装包、GUI/Computer Use 或最终产物 E2E；Windows 原生行为与 Linux 交付保持 `Unverified`。
