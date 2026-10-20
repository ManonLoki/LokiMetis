# 两端共用的 SPAKE2 核心

此库提供发送端A、接收端B及严格JSON消息类型。协议合同见[设备配对协议](../docs/DEVICE_PAIRING_PROTOCOL.md)。精确使用RustCrypto `spake2 =0.4.0` / Ed25519变体；不声称能与任意RFC9382库直接互通。

```text
SenderSession::start → PairingHello
ReceiverSession::start → PairingReply
SenderSession::accept → PairingConfirmation + AwaitingAck
ReceiverSession::confirm_and_commit → PairingAck
AwaitingAck::verify → PairingMaterial
```

接收端的保存回调只在A确认认证成功后调用一次，返回成功才生成B回执。回调必须原子复核当前配对窗口、取消状态并完成持久化。宿主负责四位码和UUID的系统随机生成、60秒期限、跨连接最多5次预算和单在途限制。网络驱动负责帧上限、超时与取消。不要记录配对材料或把确认消息的固定nonce规则用于未来业务数据。

配对后恢复会话位于`session`：终端B主动连接后发送`SessionHello`，桌面A返回`SessionReply`，终端验证后发送`SessionConfirmation`，双方才使用各自独立方向的`SecureChannel`。每次恢复以保存秘密进行全新随机交换，严格序号与AEAD拒绝反射、重放和错序；收到无效包永久关闭双向通道。线协议及上游未经审计/未保证防时序攻击的边界见[终端联动协议](../docs/DEVICE_LINK_PROTOCOL.md)。该库API不等于移动应用已接通。

本库没有GUI、SQLite、网络或异步运行时依赖。工作区内独立测试：

```sh
cargo +1.98.1 test -p loki_metris_pairing --lib
```

已选定Android通过JNI、iOS通过C ABI薄桥调用相同Rust状态机；Apple Watch已选iPhone协作模式，真实目标与运行仍待验证。平台接入及备选方案取舍见[技术选型](../docs/DEVICE_PAIRING_TECHNOLOGY.md)。已加入独立[移动协议桥](../loki_metris_mobile_bridge/README.md)，并共享窗口和资源校验；JNI/Swift及系统安全存储驱动正在移动项目接线，尚未形成手机实物互通证据。当前crate继承根workspace清单，接入其它项目时应保持整套依赖版本、suite和协议合同一致。
