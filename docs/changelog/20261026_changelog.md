# 2026-10-26 开发变更

## FEAT-DISPLAY-TERMINAL-PLATFORMS · 跨平台展示终端认证

分类：feature；required_version：`0.10.0`。本周期首个新功能，将开发版本从`0.9.3`提升至`0.10.0`。

已认证终端元信息接受macos、windows和linux，并在设备卡片显示正确的平台名称。保留原有Android/iOS身份、未知平台拒绝、设备身份绑定与iPhone专属Watch授权。SPAKE2、保存ACK、会话加密及配对账本均不改变；协议见[终端联动协议](../DEVICE_LINK_PROTOCOL.md)。

开发验证：`cargo test -p loki_metris_core --lib devices::link::tests`六项通过，含五平台序列化/认证身份校验和未知值拒绝；`cargo check -p loki_metris_gui --bin loki_metris_gui`通过。此结果不代表新的客户端已安装运行、Windows/Linux展示端验收或正式发布。
