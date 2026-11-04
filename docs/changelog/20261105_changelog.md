# 2026-11-05 开发变更

## LMDESK-TERMINAL-PET-PANEL-RULE-001 · 移动端Hooks只跟随桌宠当前页

分类：bug-fix；required_version：`0.12.1`。

已配对的手机、平板、Apple Watch与小组件此前收到全部十二槽位内的活跃项，与桌宠面板当前页不一致。发布器现在与桌宠共用同一套布局+焦点位置的当前页推导（core `project_hooks_page`），只发送当前页可见槽位：翻页后新快照撤下旧页，空页发送空集合不挪用其它页，槽位保持1..12绝对编号，文案截断与缺图语义不变。TCP完整快照、Watch配套转发与HTTP摘要消费同一发布投影。

开发验证：core 8项终端数据回归（新增当前页投影逐槽一致、翻页撤旧页、越界焦点夹紧、其它布局同一原则），23项桌宠偏好回归（翻页与布局切换推导改用共享helper），以及2项设备发布回归（新增真实中继事件+翻页往返的发布循环回归）通过。未执行真机联动、安装包或跨平台候选验收。

## LMDESK-CURSOR-KEYRING-BACKENDS-001 · Cursor 加密登录覆盖 Windows 与 Linux Secret Service

分类：feature；required_version：`0.13.0`。

Windows 现在读取 Local State 中 DPAPI 包装的密钥并用 AES-256-GCM 解密 safeStorage v1/v10 包装；Linux 经 Secret Service（secret-tool）取得既有密码后走共享的 PBKDF2/AES-128-CBC 解密（macOS 原有 CommonCrypto 路径合并为同一实现，向量不变）。KWallet 原生路径与未支持系统仍返回锁定，明文登录路径不变。

开发验证：7 项 safeStorage 回归（macOS 一千零三轮与 Linux 一轮固定向量、Windows GCM 合成向量、Linux secret-tool 进程夹具、失败映射）通过；service 全量 463 项与 pairing 23 项通过；x86_64-pc-windows-gnu 与 x86_64-unknown-linux-gnu 交叉检查通过。Windows DPAPI/Local State 与 Linux 真实密钥环、真实账户读取待对应平台候选验证。新增 RustCrypto 依赖 aes-gcm、aes、pbkdf2、sha1。
