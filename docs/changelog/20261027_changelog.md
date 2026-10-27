# 2026-10-27 开发变更

## LMDESK-CURSOR-GROK-SHARED-QUOTA-001 · Grok 与 Cursor 联名额度兼容

分类：feature；required_version：`0.10.0`。复用当前发布周期的功能版本，不重复提升Minor。

支持官方 DashboardService 的周/月周期、毫秒重置时间与真实百分数，新联名套餐显示周额度，旧套餐保持月额度。macOS 读取已有 Cursor safeStorage 加密登录态，密钥与令牌只驻留当前读取内存，保留旧明文/会话和旧服务器回退。已识别新套餐后的失败不混入旧月额度，缺失不冒充零。

相关 core、Cursor登录/额度/Token 驱动回归以及用户批准的 macOS 真实账户读取已通过。Windows/Linux的系统加密后端仍未接入，未执行安装包、GUI候选或渠道发布；范围见[产品规格](../product_spec/20261027_product_spec.md#grok-与-cursor-联名套餐额度)。
