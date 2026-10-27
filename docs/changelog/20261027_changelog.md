# 2026-10-27 开发变更

## LMDESK-TODAY-UNREADY-AGENT-TOTAL-001 · 空 Agent 不再隐藏今日总 Token

分类：bug-fix；required_version：`0.10.7`。

ClaudeCode 等已启用来源尚未扫描或待重扫时，今日摘要从同轮可解释的独立 Agent 分量保留部分总量，并明确保持 Partial/Unknown 与最低置信度。缺失分量不补零，全部缺失仍无记录，真实读取失败仍不可用；部分总量不能触发 Token 通知。桌宠与局域网终端共用修复，严格联合调用查询和分页合同保持。

开发验证：10 项今日领域回归、6 项服务缓存回归通过，覆盖首次读取与缓存命中、有效零、缺失成员、失败优先、同轮身份、重复与溢出。Windows 安装包及 Android 平板复测单独取证，不将开发测试表述为最终候选验收。

## LMDESK-CURSOR-GROK-SHARED-QUOTA-001 · Grok 与 Cursor 联名额度兼容

分类：feature；required_version：`0.10.0`。复用当前发布周期的功能版本，不重复提升Minor。

支持官方 DashboardService 的周/月周期、毫秒重置时间与真实百分数，新联名套餐显示周额度，旧套餐保持月额度。macOS 读取已有 Cursor safeStorage 加密登录态，密钥与令牌只驻留当前读取内存，保留旧明文/会话和旧服务器回退。已识别新套餐后的失败不混入旧月额度，缺失不冒充零。

相关 core、Cursor登录/额度/Token 驱动回归以及用户批准的 macOS 真实账户读取已通过。Windows/Linux的系统加密后端仍未接入，未执行安装包、GUI候选或渠道发布；范围见[产品规格](../product_spec/20261027_product_spec.md#grok-与-cursor-联名套餐额度)。

## LMDESK-PC-DEVICE-ARTWORK-001 · PC 设备按系统显示笔记本

分类：feature；required_version：`0.10.0`，沿用当前功能周期，不重复提升Minor。

已配对Windows、macOS和Linux按认证系统标识显示各自笔记本图，无需手机/平板形态。屏幕包含对应经典壁纸风格与四格Windows、Apple、Tux徽标；保留原卡片、平台标签、手机/平板/Watch和中性回退。配对与认证协议已支持PC，本次不新增协议字段或放宽信任校验。

开发验证：设备页17项回归及core认证6项回归通过，覆盖PC无形态配图、八项嵌入图的解码/透明背景、七类设备卡片的宽窄/明暗/双语布局与五平台身份校验。未执行安装包、真实PC客户端配对或跨平台最终候选验收。
