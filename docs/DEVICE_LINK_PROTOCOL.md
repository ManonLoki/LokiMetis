# 终端局域网联动协议 v1

change_id：`LMDESK-TERMINAL-LINK-001`；required_version：`0.6.0`（plan）。日期：2026-10-21。状态：实施中。以下明确区分共享协议、宿主接线和真实设备验证，不能据库测试宣称产品已互通。

## 服务与平台职责

桌面提供长期数据服务，Android/iPhone主动发现并连接。桌面沿同一双向连接推送Token/Hooks状态；终端请求缺失图片，首次缓存后按内容哈希复用。TCP发起方与密码学角色无关：桌面仍为SPAKE2 A，终端为B。首次四位码和保存规则继续以[配对协议](DEVICE_PAIRING_PROTOCOL.md)为准。

桌面侧已接入独立的`_loki-host._tcp.local.`广播及TCP监听，旧`_loki-metris._tcp.local.`接收端发现继续保留，两者不混淆。用户进入设备页启动服务；空安装启动不联网，已有配对时在下次启动恢复服务。离页关闭未保存设备的临时窗口，已配对连接由应用继续拥有。首版服务使用私网/链路本地IPv4，广播禁用IPv6与环回，不能把领域层的IPv6规则支持写成当前监听支持。

每条新连接的第一帧是严格`ConnectionIntroduction { kind, value }`：`kind=pairing`时value为`PairingOffer { version:1, clientId, deviceId, name }`；`kind=resume`时value为下述`SessionHello`。临时报到仅在终端主动配对窗口内发送，桌面最多等待60秒，用户选择并输入四位码后仍执行原四帧配对，真实本地保存后才授信。首次介绍与恢复握手均有15秒初始认证总预算；最多8条未认证连接，全部已保存及未认证连接合计至多72条，由同一JoinSet拥有。未知身份、取消、任一协议错误均关闭该socket。

用户最终选择纯局域网，不做息屏推送：后台保留已接收缓存，恢复时重新认证并补齐最新快照。iOS挂起、Android Doze都不能由长连接或轮询绕过。Watch选iPhone配套传输，不复用手机IP或在线状态。官方依据：[iOS后台](https://developer.apple.com/documentation/uikit/extending-your-app-s-background-execution-time)、[Android Doze](https://developer.android.com/training/monitoring-device-state/doze-standby)、[watchOS低层网络限制](https://developer.apple.com/documentation/technotes/tn3135-low-level-networking-on-watchos)、[WatchConnectivity](https://developer.apple.com/documentation/watchconnectivity/wcsession)；核对日期2026-10-21。

## 已实现的配对后认证会话

共享实现位于`loki_metris_pairing::session`；无网络、GUI、数据库或移动框架依赖。长期配对秘密仅作为本轮PAKE输入，不直接派生历史业务包密钥；每轮双方都使用新的随机SPAKE2交换。没有新增算法依赖。

套件名精确为`spake2-0.4-ed25519-hkdfsha256-chacha20poly1305-link-v1`，沿用`spake2 =0.4.0` Ed25519变体。它不是任意RFC9382实现互通的承诺。上游明确披露未经独立安全审计，且未保证恒定时间或抵抗时序攻击；本协议也没有第三方安全认证。不得将每次新临时交换描述为已正式验证的前向保密保证。[上游安全说明](https://docs.rs/spake2/0.4.0/spake2/#security)

终端B先使用配对得到的32字节原始秘密作为`Password`，不得在另一端改用Base64字符串。身份域为：

```text
idA = UTF8("LokiMetris/link/1/" + suite + "/desktop/" + clientId)
idB = UTF8("LokiMetris/link/1/" + suite + "/device/" + deviceId)
```

1. 终端发送`SessionHello`：`version=1`、`suite`、`clientId`、`deviceId`、本机新随机UUID `session`、B的规范Base64交换`exchange`。此时所有字段仍未授信。
2. 桌面按当前账本中的双端身份精确查找秘密，建立全新A交换。返回`SessionReply`：`version=1`、回显`session`、桌面新随机UUID `challenge`、A交换`exchange`及桌面`confirmation`。
3. 终端验证桌面确认后返回`SessionConfirmation { confirmation }`；桌面验证此确认后才取得业务通道。终端必须先写完该确认，随后才能写业务帧。

宿主以系统安全随机源生成新UUID，不从对端字段复制；即使宿主误复用两个UUID，新PAKE交换仍须保证不能接受旧确认或旧业务包。四位码不参与恢复握手，也不再次展示或持久化。

按下列顺序对每个UTF-8字段加入`u32大端字节长度 || 字段字节`，计算SHA-256 transcript：

```text
"LokiMetris/link/1", suite, clientId, deviceId,
session, challenge, Hello.exchange(B), Reply.exchange(A)
```

HKDF-SHA256使用本轮PAKE共享输出作IKM、transcript作salt。四个独立info为`LokiMetris/link/1/desktop-proof`、`device-proof`、`desktop-data`、`device-data`（后三项也带相同前缀），各导出32字节。两种proof分别AEAD加密固定文案`LokiMetris/link/1/desktop-confirm`和`LokiMetris/link/1/device-confirm`，AAD为transcript，nonce为12字节零；每个独立proof key仅用一次。

## 已实现的数据帧与元信息

TCP帧仍为`u32大端长度 || UTF-8 JSON`。握手载荷上限4096字节；认证密文JSON上限96KiB，必须在反序列化和分配载荷前检查。单包应用明文为1..65536字节。

密文消息为`EncryptedPacket { sequence, ciphertext }`，ciphertext是含16字节tag的规范Base64。每个方向从序号0严格递增；nonce为`4字节零 || u64大端sequence`，AAD为`transcript的32字节 || u64大端sequence`。两个方向使用不同data key。单方向最多2^32包，之后必须重新认证；不能回绕。重放、跳号、非法编码、长度或认证失败永久关闭双向通道。通道不可复制或序列化。本地尚未发送的空/超限明文拒绝且不消耗nonce。

业务恢复完成后的第一条终端加密明文是严格`DeviceInfo { deviceId, name, platform, appVersion }`。platform为`android`或`ios`，名称沿用160字节/无控制符限制；版本非空白、无控制符、最多64字节。元信息不得改变本轮已认证身份，也不接受自报`ip`字段。IP取实际认证socket的`peer_addr()`，保留IPv6链路本地scope；IP不写入配对账本。

## 已实现的在线领域规则

`DeviceBook::begin_link_session`不把持久秘密交给GUI；验证后获得不可伪造的会话。在线租约只允许同次认证绑定一次，坏包关闭的通道不能上线；当前账本移除或同ID换密钥会撤销原会话资格。

15秒心跳、45秒无认证入站消息判暂不可达；本地发送成功、mDNS广告或保存配对不算入站通信。使用本机单调时间，期限边界即过期，迟到包不能复活。重连创建新代次，旧socket的心跳和关闭不能影响新IP/状态。应用后台或断开不代表设备关机；投影保留最近认证地址与距上次通信时间。Watch需要独立的iPhone协作投影，尚未接线。

## 已实现的宿主接线与剩余验证边界

桌面网络适配器已接入握手总期限、连接并发上限、应用退出回收，以及在当前账本下原子提交presence。加密控制消息为`{ type:"ping", nonce }`、`{ type:"pong", nonce }`或`{ type:"pause" }`；Pong必须匹配在途Ping。JSON分帧读进度由连接owner持有，select取消不丢失半帧；半写取消或任何解码/认证错误均关闭socket。启动只读快照不能覆盖用户移除结果，空账本的旧重启令牌不能跨越离页取消。设备页区分认证当前IP与不可达时的上次IP。

桌面已实现业务快照和资源传输：首条桌面业务消息为`{type:"snapshot", snapshot}`，之后每次更新沿同一通道推送完整快照。`snapshot.version=1`；`tokens`含`day {date,zone,startMs,endMs}`、同轮`observedMs`、`all`及`agents [{agent,amount}]`。每份amount含十进制字符串或null的`total`、`availability`及完整度/置信度；未扫描与读取失败不冒充零。Token每30秒最多重读一次，连接归零停止周期读取；初连重新读取，超过90秒或跨日的旧值不作为当前值发送。

Hooks投影包含`hooksAvailable`和`hooks [{agent,slot,behavior,content,contentTruncated,image}]`，沿用本机十二槽位覆盖/释放规则，只发送公开且启用Agent的最终状态。slot为1..12；文案最多512 UTF-8字节，截断显式标记；image为内容摘要或null。发布器每500毫秒检查内存状态，只在变化时推送。Token错误与目录错误分别用`tokenError`/`catalogError`明确表达。已连接终端共享一次读取结果，慢设备只保留最新快照；心跳、状态发送优先于持续入站请求。

`assets [{hash,mime,length}]`仅含公开档案实际引用的可用图片，最多64个摘要；同内容去重。SHA-256为64位小写十六进制，MIME限PNG/JPEG/GIF，单资源最多32MiB。终端只发送`{type:"assetRequest",hash,offset}`请求当前清单资源；offset从0开始，按16384原始字节对齐。桌面返回`{type:"assetChunk",chunk:{hash,offset,data}}`，data为规范Base64；旧清单摘要返回`{type:"assetUnavailable",hash}`。每次请求只响应一块，没有无界发送队列。接收器严格检查顺序、长度、最后完整摘要与魔数，校验完成才能交平台原子保存；缺失/损坏缓存须重新下载。清单变化不会把不同正文拼成同一资源。

桌面完成项单独登记为`LMDESK-LAN-DESKTOP-SERVICE-001`，required_version `0.6.0`。独立共享C ABI桥已有回归，窗口及资产校验在桌面与桥之间只保留同一实现。移动原生驱动/私有存储、终端真实缓存、重连和Watch协作仍在实施；不能把桌面能力或纯接收状态机测试报告为移动产品已完成。

iOS显示项目已写入应用级`LanLinkEngine`与`LanSocket`接线：主动浏览桌面服务、显式配对、Keychain真实保存后ACK、新socket恢复、裸DeviceInfo、加密心跳/完整快照与逐块图片请求。前后台由App拥有，切页或保存名称不重启连接；显式停止在当前进程内优先于自动恢复。每轮最多8次传输重试，真实前台恢复或目标新广告可开启新轮，不能修改共享密码窗口预算。读进度、8帧写队列、半写超时和主线程最新投影各自有owner与容量边界。

手机完整展示快照写入App私有`DisplayCache/display-lan-v1.json`，供进程重启后的历史展示；摘要另经App Group供Widget使用，正文/图片与配对秘密不进入组件文件。iOS图片只下载当前Hook实际引用的摘要，Rust逐块及完整校验后同步磁盘、重读校验并原子提交，成功缓存直接复用。后台缩略图解码限制为512像素，当前展示静态缩略图；图片缓存保留当前引用并按复用时间收敛到512MiB，精确回收本产品崩溃遗留半成品。源码接线不等于两端实际运行已通过，Android连接owner、Watch协作及完整移动互通仍须完成。

本次已运行的测试事实以提交说明和实际输出为准。隔离环回TCP、模拟时钟及固定测试密钥不替代真实局域网、Android/iOS/Watch或桌面跨平台运行证据。
