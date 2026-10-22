# GUI 支持界面与出站入口

当前产品遵循已批准的 `GPUI-PARITY-001`（required_version `0.2.0`）：保留 LokiMetris 名称与独立身份，参考 LokiMetis 的公共设置、交互与本地媒体。用户明确授权为实现该目标引入必要项目依赖及资源，无需重复确认日常实施。

应用信息、更新日志和赞助区统一位于公共设置；没有独立关于或赞助导航。更新日志只显示当前构建嵌入的实际发布数据，普通构建显示空状态，不复制参考应用的发布记录。应用 updater 未接入。

GitHub 按钮固定打开 `https://github.com/ManonLoki/LokiMetis`，提示为历史项目源码。此入口的历史源码提示不随本地 Git remote 自动改变；仓库配置不代表 LokiMetris 已完成渠道发布。按钮只发送系统打开 URL 请求，不上传文件、日志或会话；系统拒绝时显示错误，重试清除旧错误。

页内赞助区只显示历史项目原有微信与支付宝收款图，精确来源、字节摘要见 `loki_metris_gui/assets/SOURCE.md`。本次1:1目标选择这两张现有图，原样嵌入，不重绘、不解码重建、不自动支付，也不表示获得任何支付渠道认证。

通知偏好默认关闭；启动只恢复本地设置并查询真实系统状态，不主动请求权限或发送测试通知。当前已接入当天首次严格超过1B Token、Hooks询问/结束/错误及Codex/WorkBuddy换肤终态通知；触发与去重规则以[最新产品规格](product_spec/20261022_product_spec.md#托盘自启与系统通知)为准，已确认开关与权限共同控制发送。原生通知点击可恢复主窗，API接受、横幅呈现与点击分别验证。

macOS使用本产品bundle的UserNotifications权限与协作delegate，显式开启时必要申请Alert/Sound，随后保存用户意图并披露权限与保存结果；关闭不撤销OS权限。Windows使用本产品AUMID、Toast设置查询及Activated/Failed回调，只有用户明确开启或重试才初始化授权身份，启动查询不偷偷写HKCU。Linux使用会话D-Bus `org.freedesktop.Notifications`，服务可用不表示每应用授权或横幅已显示；owner、点击及未知投递结果按[Linux工程知识库](LINUX_GPUI_TOOLCHAIN.md#系统通知宿主)处理。v0.3.11 ARM64 GNOME XWayland的Hook询问横幅、点击恢复和关闭门禁已有真实候选验证，见[能力证据索引](LINUX_GPUI_TOOLCHAIN.md#已验证能力与证据索引)；其余事件、版本及平台按实际候选另取证。

macOS 通知要求完整 bundle 的签名身份与 `Info.plist` 中本产品标识一致。开发预览可使用仅本地的 ad-hoc 签名并严格校验；它不代表 Developer ID 签名、公证或分发候选。裸 debug 二进制或仅有链接器签名的复制 bundle 不能作为通知权限验收依据，正式候选仍遵守独立签名门禁。系统拒绝、权限查询失败和超时必须保留真实失败，不能以本地偏好模拟授权。


局域网联动（`LMDESK-TERMINAL-LINK-001`，required_version `0.6.0`）由应用级服务拥有：应用启动即加载私有配对账本并启动连接服务，不依赖进入设备页或已有配对设备。“终端设备”页仅展示已配对卡片；移动端发起的连接请求可在任意页面触发串行配对模态，切页或隐藏不撤销应用级请求，用户取消与票据过期仍撤销对应尝试。当前桌面已接入认证设备信息、IP、心跳、Token/额度/Hooks 快照、按需图片分块及 iPhone 附属 Watch 状态。共享桥与外部移动驱动的接入、缓存、恢复及实物验证范围见[终端联动协议](DEVICE_LINK_PROTOCOL.md)；源码接入不表示各端验收通过。配对码/密钥不进入日志或页面快照，真实私有保存后才授信。首次配对合同见[设备协议](DEVICE_PAIRING_PROTOCOL.md)。macOS预览与正式包共用两类Bonjour声明，不自动授予本地网络权限。


Cursor账户Token入口由用户于2026-10-22批准，change_id：`LMDESK-CURSOR-TOKEN-USAGE-001`，required_version：`0.8.0`。只在启用Cursor且选择该入口时读取当前本机登录态，向固定`https://cursor.com/api/dashboard/get-filtered-usage-events`发出只读POST；请求仅含冻结时间和分页，不发送项目、会话正文或本机路径。令牌只进入curl标准输入，不读写浏览器或钥匙串，不改写Cursor数据库，不接受重定向。读取任务归应用后台owner，退出/取消发送信号后等待自有进程回收。结果仅在内存中按账户/时区/窗口节流，不写原始事件、用户标识、会话ID或Token历史；完整性与缺失规则见最新产品规格。
