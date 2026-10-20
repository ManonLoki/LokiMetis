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

15秒心跳、45秒无认证入站消息判暂不可达；本地发送成功、mDNS广告或保存配对不算入站通信。使用本机单调时间，期限边界即过期，迟到包不能复活。重连创建新代次，旧socket的心跳和关闭不能影响新IP/状态。应用后台或断开不代表设备关机；投影保留最近认证地址与距上次通信时间。Watch不能复用iPhone投影；移动配套owner和桌面子设备投影见下节，完整设备间运行仍待验证。

## 已实现的宿主接线与剩余验证边界

桌面网络适配器已接入握手总期限、连接并发上限、应用退出回收，以及在当前账本下原子提交presence。加密控制消息为`{ type:"ping", nonce }`、`{ type:"pong", nonce }`或`{ type:"pause" }`；Pong必须匹配在途Ping。JSON分帧读进度由连接owner持有，select取消不丢失半帧；半写取消或任何解码/认证错误均关闭socket。启动只读快照不能覆盖用户移除结果，空账本的旧重启令牌不能跨越离页取消。设备页区分认证当前IP与不可达时的上次IP。

桌面已实现业务快照和资源传输：首条桌面业务消息为`{type:"snapshot", snapshot, capabilities:["watchCompanionV1"]}`，之后每次更新沿同一通道推送完整快照。`snapshot.version=1`；`tokens`含`day {date,zone,startMs,endMs}`、同轮`observedMs`、`all`及`agents [{agent,amount}]`。每份amount含十进制字符串或null的`total`、`availability`及完整度/置信度；未扫描与读取失败不冒充零。Token每30秒最多重读一次，连接归零停止周期读取；初连重新读取，超过90秒或跨日的旧值不作为当前值发送。

Hooks投影包含`hooksAvailable`和`hooks [{agent,slot,behavior,content,contentTruncated,image}]`，沿用本机十二槽位覆盖/释放规则，只发送公开且启用Agent的最终状态。slot为1..12；文案最多512 UTF-8字节，截断显式标记；image为内容摘要或null。发布器每500毫秒检查内存状态，只在变化时推送。Token错误与目录错误分别用`tokenError`/`catalogError`明确表达。已连接终端共享一次读取结果，慢设备只保留最新快照；心跳、状态发送优先于持续入站请求。

`assets [{hash,mime,length}]`仅含公开档案实际引用的可用图片，最多64个摘要；同内容去重。SHA-256为64位小写十六进制，MIME限PNG/JPEG/GIF，单资源最多32MiB。终端只发送`{type:"assetRequest",hash,offset}`请求当前清单资源；offset从0开始，按16384原始字节对齐。桌面返回`{type:"assetChunk",chunk:{hash,offset,data}}`，data为规范Base64；旧清单摘要返回`{type:"assetUnavailable",hash}`。每次请求只响应一块，没有无界发送队列。接收器严格检查顺序、长度、最后完整摘要与魔数，校验完成才能交平台原子保存；缺失/损坏缓存须重新下载。清单变化不会把不同正文拼成同一资源。

桌面完成项单独登记为`LMDESK-LAN-DESKTOP-SERVICE-001`，required_version `0.6.0`。独立共享C ABI桥已有回归，窗口及资产校验在桌面与桥之间只保留同一实现。移动原生驱动/私有存储与Watch协作已写入源码，终端真实缓存、重连和跨设备互通仍须运行验证；不能把桌面能力或纯接收状态机测试报告为移动产品已完成。

iOS显示项目已写入应用级`LanLinkEngine`与`LanSocket`接线：主动浏览桌面服务、显式配对、Keychain真实保存后ACK、新socket恢复、裸DeviceInfo、加密心跳/完整快照与逐块图片请求。前后台由App拥有，切页或保存名称不重启连接；显式停止在当前进程内优先于自动恢复。每轮最多8次传输重试，真实前台恢复或目标新广告可开启新轮，不能修改共享密码窗口预算。读进度、8帧写队列、半写超时和主线程最新投影各自有owner与容量边界。

手机完整展示快照写入App私有`DisplayCache/display-lan-v1.json`，供进程重启后的历史展示；摘要另经App Group供Widget使用，正文/图片与配对秘密不进入组件文件。iOS图片只下载当前Hook实际引用的摘要，Rust逐块及完整校验后同步磁盘、重读校验并原子提交，成功缓存直接复用。后台缩略图解码限制为512像素，当前展示静态缩略图；图片缓存保留当前引用并按复用时间收敛到512MiB，精确回收本产品崩溃遗留半成品。源码接线不等于两端实际运行已通过，Android完整构建、Watch协作及完整移动互通仍须完成。

Android显示项目已写入应用级`AndroidLanEngine`、非阻塞`AndroidLanSocket`与DNS-SD浏览接线，移除旧的仅广播owner。`DisplayApplication`统一前后台与屏幕旋转，界面不拥有socket；主动选择、共享配对、Keystore真实保存后ACK、新TCP认证及裸DeviceInfo顺序同iOS。API34以上持续跟踪服务信息，旧版串行解析并刷新临时地址；发现停止或后台释放旧系统前台多播锁。地址不作为持久信任。

Android已提交信任映射只在系统同步提交成功后更新，避免SharedPreferences失败写入的内存变化被读成已保存；每次持久化完整映射，最多64个桌面。完整快照与图片均在应用私有目录，真实原子提交后读回校验；完成前的部分文件不可被展示。严格JSON语法及64层上限在系统递归解析前执行。Kotlin相关行为回归与应用Kotlin编译通过，完整APK构建因缺Rust Android目标停止，尚无真实配对、断线重连或缓存互通运行结论。iOS模拟器也仍待缺失目标安装授权；不能将源码接线或扩展独立编译写成App E2E通过。

本次已运行的测试事实以提交说明和实际输出为准。隔离环回TCP、模拟时钟及固定测试密钥不替代真实局域网、Android/iOS/Watch或桌面跨平台运行证据。

## Watch配套前台协议

移动项目已增加单target watchOS SwiftUI App与应用级`PhoneWatchBridge`/`WatchLinkEngine`。Watch默认展示，设备管理负责配套同步，名称与稳定UUID放设置。系统已配对的iPhone作为中转，Watch不建立桌面TCP或复用iPhone的配对秘密；桌面到iPhone走LAN，iPhone到Watch由系统配套通道决定蓝牙或Wi-Fi路径，不声称API可强制仅LAN。

首版只使用带回执的`sendMessageData`：`version=1`，请求为`probe/requestSync/pause/snapshotBegin/snapshotChunk/assetBegin/assetChunk`；编码后的单RPC最多32KiB，正文块16KiB。完整快照包装最多96KiB，含来源桌面UUID、iPhone UUID、手机接收时刻及已通过完整解析的展示投影，不包含秘密。包装SHA-256绑定原始传输字节，Watch完整重组、解析、私有原子保存后才回复complete；cached只表示旧缓存重新核验，不把命中时刻写成新同步。

两端各持一个物理请求槽和最新待发送投影。8秒无回执只撤销逻辑资格，系统回调终态前不另排无限请求；旧请求结果受手机LAN代次和Watch前台UUID约束。Watch每次真正进入active建立新UUID，inactive/background立即撤销。iPhone只在当前LAN已认证且未退后台时转发；Watch唤醒手机的请求不会启动后台LAN。WC激活、已配对或isReachable都只作发送前提，必须有Watch前台回复才产生可通信投影；逐块有效ACK续45秒通信租约，完整快照落盘ACK才更新最近同步。

图片仍只包含当前Hook实际引用摘要。Watch检查元信息、严格偏移、完整长度、MIME魔数及SHA-256后原子提交；相同摘要命中不传正文，损坏后可重取。Watch私有不备份`WatchLink`目录分别保存安装身份、完整原始快照和资源，不把iPhone App Group视为跨设备目录。缩略图限制256像素，当前为静态展示；当前引用优先，其余缓存限制512MiB/512文件，取消和启动仅清理精确半成品。

没有接入application context或后台文件队列，不承诺息屏接收。官方依据：[WC即时数据与回执](https://developer.apple.com/documentation/watchconnectivity/wcsession/sendmessagedata(_:replyhandler:errorhandler:))、[可达性边界](https://developer.apple.com/documentation/watchconnectivity/wcsession/isreachable)、[单target Watch](https://developer.apple.com/documentation/watchos-apps/migrating-to-a-single-target-watchos-app)；核对日期2026-10-21。

桌面已在iPhone行下增加独立Watch名称、ID、应用版本、通信与最近确认同步投影，没有Watch IP或独立配对按钮。直连`DeviceInfo`仍只接受android/ios。当前认证snapshot外壳才广告`capabilities:["watchCompanionV1"]`；旧移动端忽略扩展外壳，旧桌面缺少广告时新iPhone绝不发送新消息，能力和心跳nonce随LAN代次清空，缓存快照不参与协商。

手机在pong后或撤销Watch可达时发送`{type:"companionStatus",nonce,report:{info:{deviceId,name,appVersion},reachable,contact:{sequence,ageMs}|null,sync:{sequence,ageMs}|null}}`。只允许当前iOS父租约且仍保存原信任；一只Watch不创建第二份账本或凭据。nonce必须是该连接已发送且尚未过期的ping，网络owner有界保留最近四个，容许上一轮状态与下一轮ping交叉在途。未知nonce、旧父租约或Android报告关闭通道。

实际Watch回执的序号在手机owner内递增，父连接/前台/换表不回退计数。contact的ageMs在发送前由手机单调时钟向上取整；完整快照持久化ACK才更新sync，缓存命中与图片ACK只更新contact。序号范围1..9007199254740991，历史年龄最多七天，更旧事实省略。同步序号不得大于通信序号，同步年龄不得小于通信年龄。

桌面用`收到状态时刻 − ping发送时刻 + ageMs`作为年龄上界，把排队和传输延迟也算入；上界不足45秒、父连接可达且新回执有效才显示可通信。相同序号不更新时间，倒退序号拒绝，false撤销后同序号true不能复活。父连接换代清除子状态，暂停/断线/超时/撤销立即使子投影不可达；父手机心跳不能续Watch租约。最近确认同步显示保守年龄，不使用两端墙钟计算在线。

桌面与手机源码接线、隔离回归以及Watch模拟器基础交互各自独立。桌面→iPhone→Watch真实图文传输、缓存复用和完整手机运行仍待完成；不能把源码、typecheck、独立Watch构建或空态交互报告为完整E2E通过。
