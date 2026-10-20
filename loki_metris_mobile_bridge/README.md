# 共享移动协议桥

Android JNI和Swift C ABI调用同一Rust配对、窗口、恢复认证与图片校验实现。此crate复用已有workspace依赖，不依赖GUI、SQLite、Tokio或平台网络库；unsafe只在缓冲映射入口。头文件为[公开C ABI](include/loki_metris_mobile.h)。

这是已可运行单元回归的协议桥，不表示手机应用已经连通或完成平台验收。移动项目中的JNI、Keystore、Keychain及构建入口正逐项接线，最终以真实运行证据为准。

## 调用合同

- `create`接收UTF-8 JSON `{deviceId,name}`，宿主先真实保存应用UUID，句柄跨socket保留。`destroy`幂等；旧句柄永不复用，最多64个句柄。
- `call`输入最多128KiB，调用方提供192KiB固定输出缓冲。容量不足时不消费操作；缓冲必须有效且不重叠。返回0成功、1稳定业务错误、-1 ABI/内部失败，输出为`{ok,result}`或`{ok:false,error}`。只有status>=0且输出长度合法时才能读结果。
- 所有输入/输出都是完整UTF-8字节，不用JNI modified-UTF8 String。结果可能含私有材料；不得打印输入/结果、把它们写入UI快照或崩溃日志。

首次配对顺序：

1. 只有用户主动开窗才调用`{op:"openPairing",clientId}`，取得四位码与`offer`。发送offer、显示码；重连使用同offer和同句柄，绝不自动重新open。共享窗口固定60秒、总共5次、单在途，TCP/IP变化不重置。
2. 收到桌面Hello调用`{op:"pairHello",hello}`，发送返回的reply。收到A确认调用`{op:"pairConfirmation",confirmation}`，此时只返回`material`和一次性`commitId`，没有ACK。
3. 保存前调用`{op:"checkCommit",commitId}`，同一原生owner真实写入Android Keystore保护的私有记录或iOS本机Keychain。把真实结果用`{op:"pairingStored",commitId,stored}`交回核心；原窗口仍有效、材料绑定相同且保存成功时才返回ACK。旧OS回调不能确认或消费新pending。
4. 写完ACK后关闭首次配对socket，使用保存的秘密重新建立业务连接。网络失败调用`disconnect`只结束当前尝试、不返还次数；用户取消/后台调用`cancelPairing`关闭整个窗口。存储期间过期可能留下单边记录，不能声称跨OS和网络原子提交；UI须支持忘记后重新配对。

恢复连接：`{op:"resume",clientId,secret}`中的secret是32原始字节的规范Base64；发送返回introduction。收到桌面回复调用`resumeReply`，先写完返回confirmation，再以`seal {message}`发送元信息，`open {packet}`解密桌面业务消息。收到首个可信快照才展示连接成功。认证/序号/解码失败后销毁socket，不在半写连接重试。

缓存：先用`asset_valid`核对持久文件的摘要/MIME/长度，命中不请求网络；未命中调用`assetBegin {descriptor}`。逐块调用`assetChunk {chunk}`，验证成功后才写平台临时文件。最后返回complete=true表示共享核心完整校验通过，宿主随后同步并原子提交文件；失败/取消丢弃部分文件。`disconnect`也会丢弃未完成接收器。单图32MiB、固定16KiB块和正文上限与桌面共用实现。

线协议统一见[终端联动协议](../docs/DEVICE_LINK_PROTOCOL.md)。不提供APNs/FCM、后台保活或独立Watch网络。
