# 设备配对技术选型

状态：已批准；确认日期：2026-10-20。change_id：`LMDESK-PAIRING-TECH-SELECTION-001`；required_version：`0.6.0`；分类：`maintenance`，仅记录选型，不提升版本。

采用**共享 Rust 配对核心＋移动端薄封装**。Windows、macOS、Linux 直接使用项目内 `loki_metris_pairing`；Android通过JNI、iOS通过C ABI调用。共享桥已有隔离Rust回归，手机应用实际接线仍在进行；2026-10-21已选Apple Watch由iPhone协作，独立网络不在本轮范围。

确认来源：用户在比较共享 Rust 与 BoringSSL 各端 SDK 路线后，明确要求“按照你的建议 先落盘技术选型文档”。本次只形成文档决定，不实施移动桥接、接收端或业务数据传输。关联决定见 [ADR-20261020-003](adr/20261020_ADR.md#adr-20261020-003确认共享-rust-配对核心与移动端薄封装路线)。

## 选择理由与职责

桌面和接收端共用同一套密码状态机、身份绑定、消息编码、密钥派生和双向确认，减少不同语言实现的协议偏差，也让协议修改与回归集中在一处。移动应用保留原生 UI、mDNS、TCP、生命周期和存储；不把整个客户端改写为 Rust。

| 层次 | 职责 | 当前状态 |
| --- | --- | --- |
| `loki_metris_pairing` | A/B 双方密码状态机、严格 JSON 消息、身份与 transcript 绑定、派生和认证确认 | 已有 Rust API；无 GUI、SQLite、网络或 Tokio 依赖 |
| 桌面 core / GUI | core 决定保存资格与稳定错误；GUI 驱动发现、网络、取消和原子文件提交 | 已实现发送端；真实跨设备配对尚未验证 |
| 移动薄封装 | 转换参数、管理会话句柄与缓冲区、映射稳定错误、调用同一 Rust 状态机 | 独立C ABI桥及单元回归已实现，原生驱动接线中；不另写SPAKE2或第二套确认流程 |
| 接收端宿主 | 随机码、配对窗口、全局尝试预算、单在途、网络取消、系统安全存储和保存后回执 | 显示终端当前只有 mDNS 广播，配对仍未开放 |

跨语言层后续采用小范围接口，明确会话与内存所有权、取消释放及错误返回；不让 Rust panic 穿越语言边界。具体 ABI、构建脚本和分发封装在接收端实施时确定，本次不新增绑定生成框架或第三方依赖。

## 各平台接入路线

| 平台 | 已选方向 | 后续需要验证 |
| --- | --- | --- |
| Windows / macOS / Linux | 桌面 Rust 直接调用共享 crate | 各目标架构的实际构建、系统随机源、宿主网络与持久化；现有本机测试不代表三个平台全通过 |
| Android | Kotlin/Java 经薄 JNI 层调用按目标 ABI 构建的 Rust 库 | NDK 链接与打包、真实设备加载、取消与释放、系统安全存储和接收端配对流程 |
| iOS | Swift 经 C ABI 调用 Rust 静态库，按设备/模拟器目标封装 | Xcode 集成、目标架构与部署版本、真实设备生命周期和安全存储 |
| Apple Watch / watchOS | 本轮由iPhone通过WatchConnectivity协作，不独立TCP配对 | 配套target、资源交付与真实运行仍待验证；不把iPhone心跳/IP当Watch信息 |

移动与 Watch 是共享库未来的消费端，不改变本仓库根 Cargo 所声明的 Windows、macOS、Linux 桌面产品目标。

截至 2026-10-20 核对的 [Rust watchOS 官方说明](https://doc.rust-lang.org/rustc/platform-support/apple-watchos.html)：`aarch64-apple-watchos` 与 `aarch64-apple-watchos-sim` 属于 Tier 2，可通过 rustup 获取标准库；`arm64_32-apple-watchos`、`armv7k-apple-watchos` 与 `x86_64-apple-watchos-sim` 属于 Tier 3。Tier 3 涉及自行构建标准库/工具链的维护成本，不能一概宣称 watchOS 已能直接接入。官方目标等级也不等于本库及其依赖已在项目 Rust `1.98.1` 与对应 SDK 组合上通过验证。

## 算法与兼容边界

当前保留已声明的 RustCrypto `spake2 =0.4.0` / `Ed25519Group`，套件名为 `spake2-0.4-ed25519-hkdfsha256-chacha20poly1305-v1`，沿用 HKDF-SHA256 与 ChaCha20Poly1305 的双向确认。四位 ASCII 配对码、TCP 四帧顺序及所有字段的唯一合同见[设备配对协议](DEVICE_PAIRING_PROTOCOL.md)，本选型不修改线协议。

**该 Ed25519 变体不是最终 RFC 9382 互通套件。**不能据“SPAKE2”“Ed25519”或“遵循 RFC”的名称，认定其它语言库可直接交换消息；相关差异见 [RustCrypto 官方讨论](https://github.com/RustCrypto/PAKEs/issues/186)。本路线通过共用同一实现与精确套件达成兼容。两端接入时仍须保持协议和密码依赖一致，升级后共同验证；不静默切换算法或降级。

共享 Rust 只是减少实现分叉，并非安全审计结论。[spake2 0.4.0 官方文档](https://docs.rs/spake2/0.4.0/spake2/)明确披露未经独立安全审计，并提示恒定时间限制。四位码的保护还依赖接收端实际实施至多 60 秒窗口、跨连接共享最多 5 次尝试、单在途及保存后才确认成功；这些约束不能由语言桥接代替。

## 备选方案与取舍

| 方案 | 比较结果 | 本次结论 |
| --- | --- | --- |
| 共享 Rust 核心＋移动薄封装 | 桌面继续沿用现有实现；移动侧有原生构建接入成本，密码与确认流程维护一份 | 采用 |
| Rust `boring-sys`＋Swift Crypto 内部 BoringSSL C 模块＋Android 原生封装 | macOS上Rust/Swift原语互通探针通过，Android及移动目标未验证；桌面增加C/C++、CMake与bindgen/libclang构建链，Android仍需JNI，Apple侧需跟踪内部模块兼容性 | 本次不采用，不替换项目依赖 |
| 各语言分别实现同名 SPAKE2 / RFC 套件 | 需先落实各端可用实现，并统一参数、编码、密码映射、transcript 与确认流程；算法名称本身不能解决兼容 | 本次不采用，不自行移植密码算法 |

BoringSSL 方案的工程判断依据为 [boring-sys 官方构建源码](https://github.com/cloudflare/boring/blob/master/boring-sys/build/main.rs)和 [Swift Crypto 5.0.3 清单](https://github.com/apple/swift-crypto/blob/5.0.3/Package.swift)（核对日期：2026-10-20）。`CCryptoBoringSSL` 是 Swift Crypto 的内部 C target，清单公开产品为 `Crypto`、`CryptoExtras` 和兼容产品 `_CryptoExtras`；本次探针可导入该模块，不等于获得稳定的公开 Swift SPAKE2 API。

HTTPS 首次核验指纹的路线此前已被用户选择的 SPAKE2 方向替代，本次不恢复该配对体验。

## 实现与验证边界

- 已有共享 Rust 核心的 6 项单元回归，以及桌面设备领域 5 项、GUI 网络与存储 14 项相关回归；此前在本机 Rust `1.98.1` 下通过。它们覆盖实际测试宿主和隔离夹具，不代表移动端或真实局域网配对完成。
- 选型比较期间，临时隔离探针使用 Rust `boring-sys 5.3.0` 与 Swift Crypto `5.0.3`，在 macOS 主机上验证了 A/B 两个方向得到相同秘密、错码得到不同秘密。该探针仅证明备选密码原语互通；临时目录已删除，未集成到产品，也未验证 iOS、Android、watchOS 或完整四帧与存储流程。
- 2026-10-21新增共享C ABI桥、两阶段保存及全局窗口回归；原生JNI/Swift、打包和私有存储驱动在移动项目接线中。桌面已接入业务发送；手机应用、真实设备配对/缓存和Watch仍未完成，不创建发布候选。

后续接收端实施首先验证共享核心在目标移动架构上的构建与调用，再验证四位码配对、失败与取消、限流和实际保存。Watch 独立配对是否纳入范围，须在明确产品模式与架构覆盖后决定。未来业务数据的连接认证、nonce、重放保护、撤销与传输另行协商，不能直接套用配对确认消息的固定 nonce 规则。

若实际移动/Watch 构建出现无法合理解决的障碍、必须对接已有第三方标准套件，或发现密码库的安全问题，再基于具体证据重评实现；涉及套件变化时共同修订协议和兼容策略，不在原套件名下静默替换。
