# GUI 支持界面与出站入口

当前产品遵循已批准的 `GPUI-PARITY-001`（required_version `0.2.0`）：保留 LokiMetris 名称与独立身份，参考 LokiMetis 的公共设置、交互与本地媒体。用户明确授权为实现该目标引入必要项目依赖及资源，无需重复确认日常实施。

应用信息、更新日志和赞助区统一位于公共设置；没有独立关于或赞助导航。更新日志只显示当前构建嵌入的实际发布数据，普通构建显示空状态，不复制参考应用的发布记录。应用 updater 未接入。

GitHub 按钮固定打开 `https://github.com/ManonLoki/LokiMetis`，提示为历史项目源码。此入口的历史源码提示不随本地 Git remote 自动改变；仓库配置不代表 LokiMetris 已完成渠道发布。按钮只发送系统打开 URL 请求，不上传文件、日志或会话；系统拒绝时显示错误，重试清除旧错误。

页内赞助区只显示历史项目原有微信与支付宝收款图，精确来源、字节摘要见 `loki_metris_gui/assets/SOURCE.md`。本次1:1目标选择这两张现有图，原样嵌入，不重绘、不解码重建、不自动支付，也不表示获得任何支付渠道认证。

通知偏好默认关闭；启动只恢复本地设置并查询真实系统状态，不主动请求权限或发送测试通知。当前已接入当天首次严格超过1B Token、Hooks询问/结束/错误及Codex/WorkBuddy换肤终态通知；触发与去重规则以[最新产品规格](product_spec/20261028_product_spec.md#托盘自启与系统通知)为准，已确认开关与权限共同控制发送。原生通知点击可恢复主窗，API接受、横幅呈现与点击分别验证。

macOS使用本产品bundle的UserNotifications权限与协作delegate，显式开启时必要申请Alert/Sound，随后保存用户意图并披露权限与保存结果；关闭不撤销OS权限。Windows使用本产品AUMID、Toast设置查询及Activated/Failed回调，只有用户明确开启或重试才初始化授权身份，启动查询不偷偷写HKCU。Linux使用会话D-Bus `org.freedesktop.Notifications`，服务可用不表示每应用授权或横幅已显示；owner、点击及未知投递结果按[Linux工程知识库](LINUX_GPUI_TOOLCHAIN.md#系统通知宿主)处理。v0.3.11 ARM64 GNOME XWayland的Hook询问横幅、点击恢复和关闭门禁已有真实候选验证，见[能力证据索引](LINUX_GPUI_TOOLCHAIN.md#已验证能力与证据索引)；其余事件、版本及平台按实际候选另取证。

macOS 通知要求完整 bundle 的签名身份与 `Info.plist` 中本产品标识一致。开发预览可使用仅本地的 ad-hoc 签名并严格校验；它不代表 Developer ID 签名、公证或分发候选。裸 debug 二进制或仅有链接器签名的复制 bundle 不能作为通知权限验收依据，正式候选仍遵守独立签名门禁。系统拒绝、权限查询失败和超时必须保留真实失败，不能以本地偏好模拟授权。


局域网联动（`LMDESK-TERMINAL-LINK-001`，required_version `0.6.0`）由应用级服务拥有：应用启动即加载私有配对账本并启动连接服务，不依赖进入设备页或已有配对设备。“终端设备”页仅展示已配对卡片；移动端发起的连接请求可在任意页面触发串行配对模态，切页或隐藏不撤销应用级请求，用户取消与票据过期仍撤销对应尝试。当前桌面已接入认证设备信息、IP、心跳、Token/额度/Hooks 快照、按需图片分块及 iPhone 附属 Watch 状态。共享桥与外部移动驱动的接入、缓存、恢复及实物验证范围见[终端联动协议](DEVICE_LINK_PROTOCOL.md)；源码接入不表示各端验收通过。配对码/密钥不进入日志或页面快照，真实私有保存后才授信。首次配对合同见[设备协议](DEVICE_PAIRING_PROTOCOL.md)。macOS预览与正式包共用两类Bonjour声明，不自动授予本地网络权限。


Cursor账户Token入口由用户于2026-10-22批准，change_id：`LMDESK-CURSOR-TOKEN-USAGE-001`，required_version：`0.8.0`。只在启用Cursor且选择该入口时读取当前本机登录态，向固定`https://cursor.com/api/dashboard/get-filtered-usage-events`发出只读POST；请求仅含冻结时间和分页，不发送项目、会话正文或本机路径。令牌只进入curl标准输入，不读写浏览器；加密登录态按下节已批准范围读取钥匙串，不改写Cursor数据库，不接受重定向。读取任务归应用后台owner，退出/取消发送信号后等待自有进程回收。结果仅在内存中按账户/时区/窗口节流，不写原始事件、用户标识、会话ID或Token历史；完整性与缺失规则见最新产品规格。


## Cursor联名套餐与加密登录态

用户于2026-10-27明确批准兼容 Grok + Cursor，并允许读取对应 macOS 钥匙串项及验证官方额度。change_id：`LMDESK-CURSOR-GROK-SHARED-QUOTA-001`；分类：feature；required_version：`0.10.0`；决定见[ADR-20261027-001](adr/20261028_ADR.md#adr-20261027-001cursor-联名套餐采用官方周期与受限系统解密)。

- 新额度入口固定为只读 POST `https://api2.cursor.sh/aiserver.v1.DashboardService/GetPlanInfo` 和 `https://api2.cursor.sh/aiserver.v1.DashboardService/GetCurrentPeriodUsage`，正文固定 `{}`，认证只进入 curl 标准输入的 Bearer 头。单次仍为15秒/256KiB，不接受重定向。仅套餐入口返回404/405/501时回退固定 `https://cursor.com/api/usage-summary`。
- Cursor safeStorage v1/v10 仅在 macOS 查询已有 `Cursor Safe Storage` / `Cursor Key` 项；二元组已在本机 Cursor 3.27.14 对照，不把上游 Electron 的默认账户名推断为 Cursor 定制发行事实。只调用固定 `/usr/bin/security find-generic-password -s "Cursor Safe Storage" -a "Cursor Key" -w`；参数不含秘密，标准输出上限1KiB，预算最多8秒，取消/超时回收owned进程，不通过Shell、不创建或修改系统秘密。
- Windows 仅读取同一 `%APPDATA%/Cursor` 用户数据根下有界的 `Local State`，解析 `os_crypt.encrypted_key`，以禁止交互的本用户 DPAPI 解包 AES-256-GCM 密钥；复制完成后清零并释放系统缓冲，不创建 detached 阻塞任务。OS 内部调用不能强制中断，调用前后均检查取消和预算，取消或过期结果不会发往上游。Linux 仅对真实 v11 包装经 PATH 中的 `secret-tool lookup application cursor` 查询既有 Secret Service 条目，以一轮 PBKDF2-SHA1/AES-CBC 解密；v10 固定密码降级及 KWallet 原生后端仍保持锁定。两平台均不写入登录态或系统密钥环（change_id：`LMDESK-CURSOR-KEYRING-BACKENDS-001`；required_version：`0.13.0`）。
- 密文格式、系统 AES 解密和严格 PKCS7/UTF-8/会话校验全部成功后才能请求上游。明文密钥和令牌只用于当前读取，不记录进程标准输出/错误、不写文件。系统拒绝、未知格式或未实现的平台后端保持不可用；不读其它钥匙串、不修改ACL、不刷新登录。
- 窗口类型和比例继续由 core 解释。桌面与终端共用同一非敏感额度投影；Cursor 账户 Token 查询复用同一登录读取，但仍只访问自身固定分页入口。共享池的两个提供商卡不汇总成额外额度。

## 手动检查更新

用户于2026-11-05批准在公共设置加入"检查更新"按钮（change_id：`LMDESK-UPDATE-CHECK-001`；required_version：`0.13.0`）。仅在用户点击时向固定 `https://api.github.com/repos/ManonLoki/LokiMetis/releases/latest` 发出一次只读 GET：从 PATH 发现平台对应 curl，禁用用户 curl 默认配置，8 秒预算、64KiB 响应上限、不接受重定向，请求只含 User-Agent 与 Accept 头，无凭据、无正文、无遥测。没有自动轮询、定时器或启动检查；应用其余路径保持零出站。仅接受 HTTP 200；响应只解析 `tag_name` 的三段产品标签（含正式 `v版本-YYYYMMDD` 日期后缀）并与当前构建比较：已是最新或发现新版本在按钮下方展示，发现新版本附 `https://github.com/ManonLoki/LokiMetis/releases/latest` 页面地址，由用户自行前往；比较失败、超时或畸形响应显示检查失败，不安装、不下载、不修改任何文件。
