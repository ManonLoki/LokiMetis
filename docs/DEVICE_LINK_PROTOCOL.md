# 终端局域网联动协议 v1

change_id：`LMDESK-TERMINAL-LINK-001`；required_version：`0.6.0`。日期：2026-10-21。桌面与移动接线已实现；协议、宿主职责与运行证据分别核对，本地开发证据保存在忽略的`release/`。

## 服务与平台职责

桌面提供长期数据服务，Android/iPhone主动发现并连接。桌面沿同一双向连接推送Token/Hooks及提供商额度状态；终端请求缺失图片，首次缓存后按内容哈希复用。TCP发起方与密码学角色无关：桌面仍为SPAKE2 A，终端为B。首次四位码和保存规则继续以[配对协议](DEVICE_PAIRING_PROTOCOL.md)为准。

桌面侧已接入独立的`_loki-host._tcp.local.`广播及TCP监听。应用启动即加载私有账本并启用服务，首次运行通过现有原子存储建立稳定桌面UUID；空账本也能接收首个客户端请求。页面切换或隐藏不停止服务，设备页不再浏览旧接收端或提供发现/刷新入口。首版服务使用私网/链路本地IPv4，广播禁用IPv6与环回，不能把领域层的IPv6规则支持写成当前监听支持。

客户端报到后由应用级弹窗串行显示名称、UUID和实际连接来源IP，这些初始信息明确标为未认证；系统、形态和版本仍仅在认证后取得。其他模态优先，新请求等待或按期限过期。弹窗的确认与拒绝都绑定本机单调ticket，不能作用到同ID的新连接；取消、客户端断开、提前发帧或超时立即释放请求。确认后模态保留到真实握手/落盘终态，失败在原模态显示；四位码明文显示并保留前导零，不自动提交或重试。change_id：`LMDESK-INCOMING-PAIRING-DIALOG-001`；required_version：`0.6.2`。

除下述固定HTTP摘要入口外，每条新连接的第一帧是严格`ConnectionIntroduction { kind, value }`：`kind=pairing`时value为`PairingOffer { version:1, clientId, deviceId, name }`；`kind=resume`时value为下述`SessionHello`。临时报到仅在终端主动配对窗口内发送：移动端TCP就绪后、报到前建立至多60秒窗口；桌面收到介绍后最多等待60秒，用户输入四位码后执行原四帧配对，实际剩余期限仍由接收端窗口约束，真实本地保存后才授信。首次介绍、恢复认证及开始密码交换后的阶段各有15秒预算；最多8条未认证连接，全部已保存及未认证连接合计至多72条，由同一JoinSet拥有。未知身份、取消、任一协议错误均关闭该socket。

用户最终选择纯局域网，不做息屏推送：后台清空Token与Hooks展示，仅保留配对及图片缓存；恢复时重新认证并等待最新快照。iOS挂起、Android Doze都不能由长连接或轮询绕过。Watch选iPhone配套传输，不复用手机IP或在线状态。官方依据：[iOS后台](https://developer.apple.com/documentation/uikit/extending-your-app-s-background-execution-time)、[Android Doze](https://developer.android.com/training/monitoring-device-state/doze-standby)、[watchOS低层网络限制](https://developer.apple.com/documentation/technotes/tn3135-low-level-networking-on-watchos)、[WatchConnectivity](https://developer.apple.com/documentation/watchconnectivity/wcsession)；核对日期2026-10-21。

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

业务恢复完成后的第一条终端加密明文是严格`DeviceInfo { deviceId, name, platform, appVersion }`。platform为`android`、`ios`、`macos`、`windows`或`linux`，名称沿用160字节/无控制符限制；版本非空白、无控制符、最多64字节。元信息不得改变本轮已认证身份，也不接受自报`ip`字段。IP取实际认证socket的`peer_addr()`，保留IPv6链路本地scope；IP不写入配对账本。

### 终端自行上报设备形态

change_id：`LMDESK-DEVICE-REPORTED-FORM-001`；required_version：`0.6.0`。首条严格DeviceInfo仍保持四字段，避免旧桌面拒绝新终端。新桌面在当前认证snapshot外壳增加`deviceFormFactorV1`能力；新终端仅在完整校验本连接真实snapshot后发送一次`{type:"deviceFormFactor",formFactor:"phone"|"tablet"}`。缺省能力、缓存快照和未知系统类型均不发送；连接切换/失败/关闭撤销已报告状态，新连接重新协商。未知类型、额外字段或无效消息保持失败关闭。旧终端仍可正常认证和心跳。

Android在应用级owner创建时读取系统`Configuration.smallestScreenWidthDp`：正数且≥600dp报告tablet，其余正数报告phone，未定义不报告；它是系统尺寸类别，不承诺所有折叠/桌面窗口设备的物理形态识别，也不读取Compose页面宽度或型号。iOS在主线程读取`UIDevice.userInterfaceIdiom`，phone/pad分别报告phone/tablet，其它类别不报告。Watch仍按已有iPhone子设备合同展示，不创建独立直连类型。系统API依据：[Android最小宽度](https://developer.android.com/reference/android/content/res/Configuration#smallestScreenWidthDp)、[Apple界面类型](https://developer.apple.com/documentation/uikit/uidevice/userinterfaceidiom)（2026-10-21核对）。

core只允许当前、未过期且仍受账本信任的租约更新形态。形态只保存在进程内presence，断线保留，重连清空并等待重新报告；不迁移配对账本、不影响信任或IP。GUI仅按认证平台与形态选择四套插图；无报告时显示中性图标，设备名称不参与判断。

## 已实现的在线领域规则

`DeviceBook::begin_link_session`不把持久秘密交给GUI；验证后获得不可伪造的会话。在线租约只允许同次认证绑定一次，坏包关闭的通道不能上线；当前账本移除或同ID换密钥会撤销原会话资格。

15秒心跳、45秒无认证入站消息判暂不可达；本地发送成功、mDNS广告或保存配对不算入站通信。使用本机单调时间，期限边界即过期，迟到包不能复活。重连创建新代次，旧socket的心跳和关闭不能影响新IP/状态。应用后台或断开不代表设备关机；投影保留最近认证地址与距上次通信时间。Watch不能复用iPhone投影；移动配套owner和桌面子设备投影见下节，各宿主运行证据不互相替代。

## 已实现的宿主接线与剩余验证边界

桌面网络适配器已接入握手总期限、连接并发上限、应用退出回收，以及在当前账本下原子提交presence。加密控制消息为`{ type:"ping", nonce }`、`{ type:"pong", nonce }`或`{ type:"pause" }`；Pong必须匹配在途Ping。JSON分帧读进度由连接owner持有，select取消不丢失半帧；半写取消或任何解码/认证错误均关闭socket。启动只读快照不能覆盖用户移除结果，页面切换与隐藏不关闭应用服务。设备页区分认证当前IP与不可达时的上次IP。

桌面已实现业务快照和资源传输：首条桌面业务消息为`{type:"snapshot", snapshot, capabilities:["watchCompanionV1","deviceFormFactorV1"]}`，之后每次更新沿同一通道推送完整快照。`snapshot.version=1`；`tokens`含`day {date,zone,startMs,endMs}`、同轮`observedMs`、`all`及`agents [{agent,amount}]`。每份amount含十进制字符串或null的`total`、`availability`及完整度/置信度；未扫描与读取失败不冒充零。Token每30秒最多重读一次，连接归零停止周期读取；初连重新读取，超过90秒或跨日的旧值不作为当前值发送。

Hooks投影包含`hooksAvailable`和`hooks [{agent,slot,behavior,content,contentTruncated,image}]`。当前实现沿用本机十二槽位覆盖/释放规则，发送其中公开且启用Agent的活跃项，尚未按桌宠当前页裁剪；该范围须按下述已确认约束对齐，不能据现有实现声明四格已一致。slot为1..12；文案最多512 UTF-8字节，截断显式标记；image为内容摘要或null。发布器每500毫秒检查内存状态，只在变化时推送。Token错误与目录错误分别用`tokenError`/`catalogError`明确表达。已连接终端共享一次读取结果，慢设备只保留最新快照；心跳、状态发送优先于持续入站请求。

`assets [{hash,mime,length}]`仅含公开档案实际引用的可用图片，最多64个摘要；同内容去重。SHA-256为64位小写十六进制，MIME限PNG/JPEG/GIF/BMP/WebP，由 pairing 的共享魔数表识别，单资源最多32MiB。终端只发送`{type:"assetRequest",hash,offset}`请求当前清单资源；offset从0开始，按16384原始字节对齐。桌面返回`{type:"assetChunk",chunk:{hash,offset,data}}`，data为规范Base64；旧清单摘要返回`{type:"assetUnavailable",hash}`。每次请求只响应一块，没有无界发送队列。接收器严格检查顺序、长度、最后完整摘要与魔数，校验完成才能交平台原子保存；缺失/损坏缓存须重新下载。清单变化不会把不同正文拼成同一资源。

桌面完成项单独登记为`LMDESK-LAN-DESKTOP-SERVICE-001`，required_version `0.6.0`。独立共享C ABI桥已有回归，窗口及资产校验在桌面与桥之间只保留同一实现。移动原生驱动、系统私有存储与Watch协作均已接线；真实缓存、重连和互通按对应宿主与构建留存运行证据，纯状态机测试不替代产品运行。

iOS显示项目已写入应用级`LanLinkEngine`与`LanSocket`接线：主动浏览桌面服务、显式配对、Keychain真实保存后ACK、新socket恢复、裸DeviceInfo、加密心跳/完整快照与逐块图片请求。前后台由App拥有，切页或保存名称不重启连接；显式停止在当前进程内优先于自动恢复。每轮最多8次传输重试，真实前台恢复或目标新广告可开启新轮，不能修改共享密码窗口预算。读进度、8帧写队列、半写超时和主线程最新投影各自有owner与容量边界。

手机完整展示快照仅保留在当前认证连接内存，不保存或恢复历史Token与Hooks；断开、后台、换目标和重启清空投影，旧`DisplayCache/display-lan-v1.json`只作精确迁移删除。App Group仅共享schema 2的当前摘要，认证心跳维持45秒租约，连接失效请求删除，过期/旧格式为空态，主App不从组件恢复。WidgetKit刷新仍由系统安排。正文/图片与配对秘密不进入组件文件。iOS图片只下载当前Hook实际引用的摘要，Rust逐块及完整校验后同步磁盘、重读校验并原子提交，成功缓存直接复用。后台缩略图解码限制为512像素，当前展示静态缩略图；图片缓存保留当前引用并按复用时间收敛到512MiB，精确回收本产品崩溃遗留半成品。源码接线不等于任意构建或宿主已验收，具体运行范围由相应开发证据限定。

Android显示项目已写入应用级`AndroidLanEngine`、非阻塞`AndroidLanSocket`与DNS-SD浏览接线，移除旧的仅广播owner。`DisplayApplication`统一前后台与屏幕旋转，界面不拥有socket；主动选择、共享配对、Keystore真实保存后ACK、新TCP认证及裸DeviceInfo顺序同iOS。API34以上持续跟踪服务信息，旧版串行解析并刷新临时地址；发现停止或后台释放旧系统前台多播锁。地址不作为持久信任。

Android已提交信任映射只在系统同步提交成功后更新，避免SharedPreferences失败写入的内存变化被读成已保存；每次持久化完整映射，最多64个桌面。Token与Hooks不再持久化，启动精确清理旧`lan-display-v1.json`及`.new/.bak`；失联/后台/换目标时清空内存投影。图片仍在应用私有目录，真实原子提交后读回校验，部分文件不展示。严格JSON语法及64层上限在系统递归解析前执行。Android JNI与iOS静态库由移动构建脚本按目标编译并嵌入应用。原生加载、系统存储和完整数据链路需运行完整App验证，不能以源码或扩展独立编译代替E2E。

本次已运行的测试事实以提交说明和实际输出为准。隔离环回TCP、模拟时钟及固定测试密钥不替代真实局域网、Android/iOS/Watch或桌面跨平台运行证据。

## 桌宠当前页同步约束

change_id：`LMDESK-TERMINAL-PET-PANEL-RULE-001`；required_version：`0.7.1`。用户已确认的发送范围与一致性标准唯一维护于[产品规格：移动端四格与桌宠当前页一致](product_spec/20261021_product_spec.md#移动端四格与桌宠当前页一致)。本次仅固化规则，不改线格式或声明接线已完成。

当前`project_hooks`遍历十二槽位，发布器没有消费桌宠的布局与当前页；后续需让桌宠与发送端使用同一当前页投影，并将翻页纳入快照变化条件。槽位仍使用1..12的绝对编号，不能裁剪后重编号而改变归属。TCP完整快照、iPhone向Watch转发及HTTP摘要的Hooks选择应追溯同一投影；HTTP仍只保留既有摘要字段，不增加正文或图片权限。既有大小限制、显式截断、认证与当前连接失效规则继续生效。

## Watch配套前台协议

移动项目已增加单target watchOS SwiftUI App与应用级`PhoneWatchBridge`/`WatchLinkEngine`。Watch默认展示，设备管理负责配套同步，名称与稳定UUID放设置。系统已配对的iPhone作为中转，Watch不建立桌面TCP或复用iPhone的配对秘密；桌面到iPhone走LAN，iPhone到Watch由系统配套通道决定蓝牙或Wi-Fi路径，不声称API可强制仅LAN。

首版只使用带回执的`sendMessageData`：`version=1`，请求为`probe/requestSync/pause/snapshotBegin/snapshotChunk/assetBegin/assetChunk`；编码后的单RPC最多32KiB，正文块16KiB。完整快照包装最多96KiB，含来源桌面UUID、iPhone UUID、手机接收时刻及已通过完整解析的展示投影，不包含秘密。包装SHA-256绑定原始传输字节，Watch完整重组、解析并接纳到当前前台内存后回复complete；不持久化Token与Hooks。cached只表示当前前台会话中相同完整快照重新核验，不把命中时刻写成新同步。

两端各持一个物理请求槽和一份最新待发送投影。已开始的有界快照固定其字节与清单，提交后优先补齐已提交和最新Hooks共同引用的缺失图片，再发送合并后的最新快照；更新不能反复清零大图进度。资源撤销或代次变化取消在途作业，图片失败立即让位于新快照。引用移除时收敛旧图片ACK，重新引用须再核验缓存。8秒无回执只撤销逻辑资格，系统回调终态前不另排无限请求；旧请求结果受手机LAN代次和Watch前台UUID约束。Watch每次真正进入active建立新UUID，inactive/background立即撤销。iPhone只在当前LAN已认证且未退后台时转发；Watch唤醒手机的请求不会启动后台LAN。WC激活、已配对或isReachable都只作发送前提，必须有Watch前台回复才产生可通信投影；逐块有效ACK续45秒通信租约，完整快照校验并接纳到当前前台内存的ACK才更新最近同步。

图片仍只包含当前Hook实际引用摘要。Watch检查元信息、严格偏移、完整长度、MIME魔数及SHA-256后原子提交；相同摘要命中不传正文，损坏后可重取。Watch私有不备份`WatchLink`目录只保存安装身份与资源，启动精确清理旧`watch-display-v1.json`，完整快照仅在当前前台内存中，不把iPhone App Group视为跨设备目录。缩略图限制256像素，当前为静态展示；当前引用优先，其余缓存限制512MiB/512文件，取消和启动仅清理精确半成品。

没有接入application context或后台文件队列，不承诺息屏接收。官方依据：[WC即时数据与回执](https://developer.apple.com/documentation/watchconnectivity/wcsession/sendmessagedata(_:replyhandler:errorhandler:))、[可达性边界](https://developer.apple.com/documentation/watchconnectivity/wcsession/isreachable)、[单target Watch](https://developer.apple.com/documentation/watchos-apps/migrating-to-a-single-target-watchos-app)；核对日期2026-10-21。

桌面已在iPhone行下增加独立Watch名称、ID、应用版本、通信与最近确认同步投影，没有Watch IP或独立配对按钮。直连`DeviceInfo`接受Android/iOS及三种桌面展示终端平台，Watch仍不作为独立TCP类型。当前认证snapshot外壳才广告`capabilities:["watchCompanionV1","deviceFormFactorV1"]`；旧移动端忽略扩展外壳，旧桌面缺少广告时新iPhone绝不发送新消息，能力和心跳nonce随LAN代次清空，缓存快照不参与协商。

手机在pong后或撤销Watch可达时发送`{type:"companionStatus",nonce,report:{info:{deviceId,name,appVersion},reachable,contact:{sequence,ageMs}|null,sync:{sequence,ageMs}|null}}`。只允许当前iOS父租约且仍保存原信任；一只Watch不创建第二份账本或凭据。nonce必须是该连接已发送且尚未过期的ping，网络owner有界保留最近四个，容许上一轮状态与下一轮ping交叉在途。未知nonce、旧父租约或Android报告关闭通道。

实际Watch回执的序号在手机owner内递增，父连接/前台/换表不回退计数。contact的ageMs在发送前由手机单调时钟向上取整；完整快照通过校验并被当前前台内存接纳的ACK才更新sync，缓存命中与图片ACK只更新contact。序号范围1..9007199254740991，历史年龄最多七天，更旧事实省略。同步序号不得大于通信序号，同步年龄不得小于通信年龄。

桌面用`收到状态时刻 − ping发送时刻 + ageMs`作为年龄上界，把排队和传输延迟也算入；上界不足45秒、父连接可达且新回执有效才显示可通信。相同序号不更新时间，倒退序号拒绝，false撤销后同序号true不能复活。父连接换代清除子状态，暂停/断线/超时/撤销立即使子投影不可达；父手机心跳不能续Watch租约。最近确认同步显示保守年龄，不使用两端墙钟计算在线。

桌面、手机与Watch的源码接线、隔离回归和完整应用运行各自独立。真实图文传输、缓存复用和缺图恢复的证据绑定相应构建，不能将模拟器结果扩张为物理设备、常亮硬件或其它桌面宿主的结论。

## 当前连接展示与失效

change_id：`LMDESK-TERMINAL-LIVE-DISPLAY-001`；required_version：`0.6.2`。手机/Watch只显示当前连接收到的Token和Hooks，不恢复历史快照；配对身份与内容寻址图片文件保留。iPhone父连接失效时，单个待pause意图等待旧物理RPC终态后优先发送，Watch清空展示并更换前台接收代次；RPC超时、45秒通信租约过期和系统不可达也清空，迟到旧块不能恢复。新连接必须收到新完整快照才重新显示。Widget是当前连接摘要的有限共享，不是离线历史：45秒租约与预排空Timeline限制崩溃/挂起后的残留，系统调度可能延迟清屏，不能承诺断开瞬间刷新。


## 小组件独立HTTP摘要读取

change_id：`FEAT-WIDGET-HTTP-V1`；required_version：`0.7.0`。用户于2026-10-21要求小组件独立获取桌面数据、记录当前服务地址并在下次连通时更新。

桌面在同一IPv4监听端口识别固定`POST /api/widget/v1/snapshot HTTP/1.1`，不增加HTTP广告。原有帧连接保持兼容；认证快照增加能力`widgetHttpV1`及可选外壳`widgetEndpoint {version:1,port,key}`。key是每对桌面/设备独立的32字节只读密钥，标准带padding Base64，只通过原有加密通道下发。手机使用系统当前TCP实际对端地址，不能采用任意远端URL；每次合法认证快照更新共享读取配置，地址/端口/密钥变化使在途旧请求失效。

密钥使用HKDF-SHA256，IKM为32字节配对秘密，salt为UTF8 `clientId + "/" + deviceId`，info为UTF8 `LokiMetris/widget-http/1/read-only`，输出32字节。Widget只取得此读取密钥，无法用它恢复TCP认证。请求JSON为`{clientId,deviceId,nonce,ciphertext}`；nonce为每次新生成12字节随机数的Base64，ciphertext为ChaCha20-Poly1305的密文拼接16字节tag后Base64。请求明文及AAD均为UTF8 `loki-widget-request-v1`。身份绑定由每对身份的派生密钥和当前账本共同保证。

响应外壳为`{nonce,ciphertext}`，使用新的随机12字节nonce；AAD为UTF8 `loki-widget-response-v1`拼接原请求12字节nonce。解密后为`{type:"snapshot",sourceId,snapshot}`，snapshot仅包含version、tokens、tokenError、hooksAvailable、hooks及可选enabledAgents/providerQuota；名单语义见[终端Agent启用名单](#终端agent启用名单)。每条Hook仅保留agent/slot/behavior，不传正文、图片、资源表或宿主路径。Widget验证来源、完整性、原始u64字符串及现有展示边界后才写入共享摘要。

HTTP头部最多4096字节，请求正文1..1024字节，必须声明唯一Content-Length及application/json，拒绝Transfer-Encoding/Expect和其他方法/路径；桌面请求处理5秒预算，客户端6秒总期限，响应外壳最多96KiB、解密摘要最多64KiB，禁止重定向、缓存、Cookie和HTTP认证。沿用最多8条待处理连接许可；每设备保留最近32个nonce，重复请求不触发再次扫描。认证请求才临时登记读取需求，唤醒休眠发布器并等待完成读取；所有TCP设备断开时也能执行，完成/超时/取消撤销需求，不将HTTP读取标记为设备在线。响应前复核当前配对，解除配对后拒绝。

App与Widget使用固定文件锁协调摘要与读取配置。切换来源、忘记配对、服务换代及较新摘要到达后，迟到HTTP不能覆盖。Timeline由`.never`改为`.after`，申请约5分钟后再次执行，实际时间由WidgetKit调度，离线保留真实上次同步时间；不承诺5分钟准点更新，不增加云推送。主App仍不恢复历史Token/Hooks。独立读取配置排除备份并使用首次解锁后可读保护。Apple机制依据：[WidgetKit刷新](https://developer.apple.com/documentation/widgetkit/keeping-a-widget-up-to-date)、[扩展网络请求](https://developer.apple.com/documentation/widgetkit/making-network-requests-in-a-widget-extension)、[本地HTTP配置](https://developer.apple.com/documentation/bundleresources/information-property-list/nsapptransportsecurity/nsallowslocalnetworking)，核对日期2026-10-21。


## 终端Agent启用名单

change_id：`LMDESK-TERMINAL-AGENT-VISIBILITY-001`；required_version：`0.9.0`。用户要求用量与额度面板只显示桌面设置启用的Agent，完整TCP快照与加密HTTP摘要统一携带可选`snapshot.enabledAgents`。

该字段是独立的camelCase字符串数组，值来自`AiTool`目录：codex/claudeCode/cursor/openCode/workBuddy/hermes/openClaw/codeBuddy/qwenCode/kimiCode/qoder/geminiCli/gitHubCopilot/grok。缺失或null表示旧桌面或尚无成功设置读取，`[]`明确表示全关闭；不能从tokens.agents、Hooks、额度成功与否或非空条目推断。Token读取失败、尚无数据及Cursor不进入终端本机Token汇总均不影响真实名单。Cursor账户Token仍仅由桌面独立视图展示，终端可同步其额度及Hooks。设置读取失败保留最近成功名单，首次未知保持未提供且不发送未知配置下的额度。

发布器每轮读取已确认设置，并通过原有500ms轮询比较完整名单变化；Cursor启停不依赖统计来源epoch。额度先由core使用同份名单筛选，再与上一份已发布投影比较；保留真实窗口、观测时间和原始Token总量/质量。全关闭明确撤下旧额度，不伪造新的额度读取。HTTP透传同一名单，手机、Watch和小组件保持其缺失/null/空数组语义；旧桌面未提供时保留移动端原行为。该扩展不改变snapshot.version、配对或认证权限。

## 提供商额度同步与主动查询

change_id：`LMDESK-TERMINAL-PROVIDER-QUOTA-001`；required_version：`0.7.1`。用户于2026-10-21要求额度发给移动终端，终端也能主动查询。

`DisplaySnapshot`增加可选`providerQuota {observedAtUnix, providers}`。observedAtUnix为桌面最近一次额度读取完成的Unix秒；不把查询时刻冒充重新请求上游的时刻。providers最多四项、唯一且按codex/claude/cursor/grok排列，每项为`{provider,plan?,fiveHour?,weekly?,monthly?}`；每个窗口为`{kind:"fiveHour"|"weekly"|"monthly",usedTenths:0..1000,resetAtUnix?}`，monthly当前只由cursor提供。claude与monthly为不兼容扩展（ADR-20261021-017），旧终端须同版升级。usedTenths为已用整数千分比，终端只做1000减法得到剩余。窗口类型必须与字段相符，方案只接受24字节以内的已消毒ASCII短文本。缺失窗口行隐藏，只有周额度或只有月额度也显示；0%有效。没有完成过读取时省略providerQuota，完成后providers空列表明确撤下旧额度。

发布器在有认证TCP会话或合法HTTP读者时独立触发同一个应用级额度板，与桌面页面是否可见无关；额度变化沿现有加密连接推送完整快照。额度仍约五分钟读取一次，在途读取合并，本地优先规则不变。手机与Watch不读取或保存提供商登录凭据，额度投影不含路径、账号标识或失败正文。

当前认证snapshot外壳广告`snapshotQueryV1`，终端完整校验本次快照后才可发送严格`{type:"snapshotRequest"}`。旧桌面无能力时不发送；断开或换连接清除能力。桌面先复核当前租约，再返回当前快照并唤醒发布器更新；每连接最多每秒处理一次查询，额度读取仍遵守五分钟节流，不允许终端强制连续请求上游。快照查询不创建新配对或在线证明，既有45秒认证入站租约规则继续生效。

现有加密HTTP摘要同时保留providerQuota，不增加URL、认证密钥或允许主机。Android/iOS的实时展示随当前认证连接清空，既有组件摘要可保留最近同步额度及其原始观测时间；组件缓存不表示实时在线。Android私有组件格式写入新版标记并兼容读取旧格式；Apple Codable缺省新字段为nil。Watch经iPhone完整快照和已授权独立HTTP两条路径均保留同一额度。旧终端忽略可选字段，不升级snapshot.version或密码套件。

开发验证包含额度投影值域/排序、仅周窗口与零余额、真实加密TCP推送和主动查询、HTTP裁剪、移动旧格式与缓存往返；构建与实物运行证据按本次开发交付分别报告，不等同于新移动包的真机端到端验收。
