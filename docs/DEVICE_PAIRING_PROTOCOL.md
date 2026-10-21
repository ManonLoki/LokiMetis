# 局域网设备发现与配对协议 v1

change_id：`LMDESK-LAN-DEVICES-001`；required_version：`0.6.0`。本文件定义桌面A与显示终端B的首次配对合同。Android/iPhone通过共享原生桥接入；当前主流程为手机发现桌面并主动连接，介绍帧与配对后业务见[联动协议](DEVICE_LINK_PROTOCOL.md)。

## 发现与信任

- 兼容发现路径由接收端发布 `_loki-metris._tcp.local.`，SRV指定配对TCP端口；TXT必须包含`v=1`、`id=<规范小写UUID>`、`name=<名称>`。名称非空、无控制符、最多160 UTF-8字节。`pair=1`才表示接收端已实现本文件约定的SPAKE2套件；`pair=off`、缺省或其它值只展示设备并禁用配对。发现schema仍为`v=1`。
- 只采用解析后的IPv4私网/链路本地、IPv6 ULA/带接口scope的链路本地地址，不解析TXT中的URL或远端主机。每设备最多8个端点，发现列表最多128项，保存列表最多64项。
- 旧接收端浏览仅保留内部兼容实现与回归，不再由当前GUI触发；其一轮12秒、广告至多30秒的限制不变。当前入口为终端主动发现桌面并发送配对请求，桌面启动/弹窗/取消生命周期以联动协议为准，离页或隐藏不关闭应用服务。
- mDNS广告未经认证，名称/IP/同一UUID的新广告都不能改写已保存凭据。“近期已发现”不等于已验证在线。每次配对固定用户选中的身份和本轮端点。

## 共用实现与移动端边界

双方复用项目内轻量Rust库[`loki_metris_pairing`](../loki_metris_pairing/README.md)，精确固定`spake2 =0.4.0`的Ed25519变体。发送端`SenderSession → AwaitingAck → PairingMaterial`和接收端`ReceiverSession`执行同一身份编码、transcript、派生及双向确认，避免不同语言库在同名算法下采用不同线协议。

该库无GPUI、SQLite、网络或Tokio依赖，可通过`cargo test -p loki_metris_pairing --lib`单独验证依赖闭包。Android JNI、iOS C ABI和系统私有存储已接入。接收端先验证得到`PendingReceiverCommit`，宿主真实保存后才完成回执；同步宿主可使用`confirm_and_commit`包装。Watch通过iPhone系统配套通信，不独立运行首次配对，具体取舍见[技术选型](DEVICE_PAIRING_TECHNOLOGY.md)。接收端宿主负责系统随机UUID、全局尝试预算、有效期、单在途和真实持久化；必须在保存回调中原子复核窗口/取消状态。库不会把内存回调成功冒充磁盘存储保证。

## 四位码与接收端前置条件

用户已明确选择4位ASCII十进制码，保留前导零，例如`0037`。配对码仅在接收端当前配对窗口显示，不进入网络消息、日志或持久文件。

接收端必须维持以下条件：

1. 用户主动打开配对窗口，使用系统密码学随机源均匀生成`0000..9999`，有效期至多60秒。
2. 同一轮码跨所有连接/IP共享原子的最多5次尝试预算，同时至多一个握手；收到有效Hello即预约一次预算，失败不退还。重连、换IP或重发广告不能重置预算；达到上限关闭该轮，重新打开必须由接收端用户主动操作并换码。
3. 验证发送端确认后，先原子保存双方身份与持久密钥，再生成最终认证回执；成功立即使该轮码失效。未验证的交换不产生可信设备。
4. 本发送端不自动重试密码交换。连接建立失败时可尝试同一候选的其它固定端点；发送Hello后不再重试。总网络预算15秒，每个TCP连接预算2秒。

共享窗口实现有效期、跨连接预算与单在途，移动宿主在系统保存成功后提交回执；各平台实际持久化和生命周期仍须以相应运行证据核对。4位码的5次均匀猜测概率上界为0.05%，不具备长密钥的抗在线猜测强度。

## 帧与算法

TCP每帧为`4字节大端无符号长度 || UTF-8 JSON`，载荷长度`1..4096`。拒绝未知字段、无效版本、错误身份、错误消息方向及超限帧。没有HTTP、代理或重定向。

套件精确名称：`spake2-0.4-ed25519-hkdfsha256-chacha20poly1305-v1`。采用RustCrypto `spake2 0.4.0`的`Ed25519Group`；这是该库的消息/哈希变体，并非最终RFC9382互通套件（[官方RFC差异讨论](https://github.com/RustCrypto/PAKEs/issues/186)），不能仅凭“SPAKE2”或“RFC9382”名称推断其它实现互通。消息为33字节（含角色前缀`A`/`B`），以标准带padding的Base64编码。

A固定为本发送端，B固定为接收端。双方传入相同身份字节：

```text
idA = UTF8("LokiMetris/pair/1/" + suite + "/sender/" + clientId)
idB = UTF8("LokiMetris/pair/1/" + suite + "/receiver/" + deviceId)
password = UTF8(四位ASCII码)
```

1. A发送`PairingHello`：`version=1`、`suite`、`clientId`、`deviceId`、随机UUID `session`、`exchange=pA`。
2. B发送`PairingReply`：`version=1`、回显`session`、`deviceId`、`name`、全新随机UUID `nonce`、`exchange=pB`。B必须检查Hello目标为自身、套件精确匹配和配对窗口仍有效。
3. A发送`PairingConfirmation`：仅含`confirmation`，以A确认密钥认证加密固定字节`LokiMetris/sender-confirm/1`。
4. B验证A确认、完成持久保存后发送`PairingAck`：仅含`confirmation`，以B确认密钥认证加密固定字节`LokiMetris/receiver-saved/1`。A验证该回执后才取得领域`VerifiedPairing`，原子落盘成功后页面才显示已保存。

`Spake2::finish()`返回密钥不代表码正确。错码、修改元数据、方向反射和旧会话回执都必须在确认阶段失败。

## Transcript和密钥派生

按下列固定顺序对每个UTF-8字段加入`u32大端字节长度 || 字段字节`，然后计算SHA-256；不依赖JSON对象键顺序：

```text
"LokiMetris/pair/1", suite, clientId, deviceId, session,
Hello.exchange, Reply.nonce, Reply.exchange, Reply.name
```

HKDF-SHA256的salt为该32字节摘要，IKM为`Spake2::finish()`返回的共享字节。三个独立info分别导出32字节：

```text
LokiMetris/pair/1/sender-confirm
LokiMetris/pair/1/receiver-confirm
LokiMetris/pair/1/persistent
```

确认消息使用ChaCha20Poly1305，AAD为transcript摘要，nonce为12字节全零，输出密文含认证tag后用标准Base64编码。每次交换使用全新SPAKE2状态，每个独立方向key只加密一次，因此固定nonce不在同一key下重复。持久key单独派生，**不能直接复用本节nonce规则发送业务数据**；业务连接使用独立随机恢复握手、方向密钥和消息序号规则，详见[终端联动协议](DEVICE_LINK_PROTOCOL.md)，不能复用本配对确认的固定nonce发业务数据。

## 本地保存与失败语义

账本位于应用数据根`devices/paired.json`，schemaVersion为1。首次只读缺文件零写入；开始配对前先持久化稳定本机UUID。保存内容仅有本机ID、已认证设备ID/名称及独立共享key，不保存IP或配对码。

Unix目录0700、文件0600；Windows沿用当前用户产品数据目录的继承访问控制，尚未经Windows宿主验证。拒绝专属目录/目标文件符号链接、不安全Unix权限、超过64KiB、未知schema、重复ID、无效字段和损坏文件。写入先同步同目录临时文件，再原子替换；成功后才提交内存快照。错误不包含原始网络响应、路径或秘密。

移除需要用户确认，仅删除本机信任记录，不调用接收端、不删除接收端数据。握手取消或本地提交失败时，接收端可能已保存其一侧记录；重配和孤立记录由设备管理中的重新配对/忘记入口处理，不能宣称分布式原子提交。

mDNS后台错误独立于存储错误。浏览结束请求Shutdown并等待确认；未确认句柄由应用保留，下次发现先清理，退出时再次有界清理，异常销毁仍发送关闭请求。无法确认时记录稳定类别，不宣称已回收或继续新建daemon。

## 依据与验证边界

截至2026-10-20核对的官方说明：[mdns-sd](https://docs.rs/mdns-sd/0.21.6/mdns_sd/)、[SPAKE2](https://docs.rs/spake2/0.4.0/spake2/)、[HKDF](https://docs.rs/hkdf/0.13.0/hkdf/)、[ChaCha20Poly1305](https://docs.rs/chacha20poly1305/0.11.0/chacha20poly1305/)、[Apple本地网络隐私](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy)。SPAKE2库明确声明未经独立安全审计并提示恒定时间限制；使用该库不等于协议获得第三方安全认证。

macOS预览/正式包从同一配置生成`NSLocalNetworkUsageDescription`及`NSBonjourServices=["_loki-metris._tcp","_loki-host._tcp"]`。不自动批准系统权限，不新增证书签名；真实授权与互通按各平台运行证据核对，模拟器不替代实物。单元测试使用隔离临时目录和真实环回TCP接收夹具，不能冒充局域网设备或产品接收端已完成。
