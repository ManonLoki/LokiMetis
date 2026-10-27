# 设备卡片示意图

四张手机/平板PNG由内置ImageGen于2026-10-21生成，Watch图于2026-10-22生成，2026-10-22按显示槽统一等比缩至短边512px（平板768×512）后嵌入，保留透明通道，避免为96×112及40×48小图槽解码原始1254px位图；不是厂商官网照片，不表示实际设备型号。三张笔记本PNG于2026-10-27使用同一内置ImageGen生成，并按用户要求编辑屏幕内的经典壁纸及系统徽标；等比缩至长边768px后嵌入。完整初始与最终编辑提示词见[imagegen.json](imagegen.json)。

- `iphone.png`：经典深色iPhone，蓝灰抽象屏幕。
- `android-phone.png`：经典深色Android手机，浅绿抽象屏幕。
- `ipad.png`：竖向iPad，蓝灰抽象屏幕。
- `android-tablet.png`：横向Android平板，浅绿抽象屏幕。
- `watch.png`：深色圆角矩形手表及黑色表带，蓝灰抽象屏幕。
- `windows-laptop.png`：深灰笔记本，Windows经典蓝色Bloom风格壁纸及白色四格徽标。
- `mac-laptop.png`：银色笔记本，macOS Big Sur风格壁纸及白色Apple徽标。
- `linux-laptop.png`：深灰笔记本，Ubuntu风格紫橙壁纸及Tux企鹅徽标。

图片保留透明通道，图像槽固定96×112px并位于卡片左侧，等比contain，不裁切。设备配图是装饰：平台来自认证DeviceInfo，手机/平板形态来自同一认证连接在deviceFormFactorV1协商后上报的phone/tablet。PC按同一认证DeviceInfo的windows/macos/linux平台选择各自笔记本图，不要求phone/tablet形态上报，也不改变协议的形态值域。笔记本只作统一示意，即使真实硬件是台式机也不据此改写形态。图片不读取名称；未取得认证平台时显示中性图标，Android/iOS缺少形态时仍显示中性图标。任何图片选择都不改变名称、平台文字、IP、信任或连接状态。兼容与系统分类边界见[终端联动协议](../../../docs/DEVICE_LINK_PROTOCOL.md)。

Watch经iPhone转发且不独立配对，仅在对应iPhone卡片内的附属区域显示40×48px等比contain小图、真实名称与在线状态，不生成独立卡片或操作入口。现有协议没有型号字段，图片不用于推断型号。
