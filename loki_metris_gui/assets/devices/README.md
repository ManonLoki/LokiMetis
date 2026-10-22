# 设备卡片示意图

四张手机/平板PNG由内置ImageGen于2026-10-21生成，Watch图于2026-10-22生成，均原样嵌入应用；不是厂商官网照片，不表示实际设备型号。完整提示词见[imagegen.json](imagegen.json)。

- `iphone.png`：经典深色iPhone，蓝灰抽象屏幕。
- `android-phone.png`：经典深色Android手机，浅绿抽象屏幕。
- `ipad.png`：竖向iPad，蓝灰抽象屏幕。
- `android-tablet.png`：横向Android平板，浅绿抽象屏幕。
- `watch.png`：深色圆角矩形手表及黑色表带，蓝灰抽象屏幕。

图片保留透明通道，图像槽固定96×112px并位于卡片左侧，等比contain，不裁切。设备配图是装饰：平台来自认证DeviceInfo，手机/平板形态来自同一认证连接在deviceFormFactorV1协商后上报的phone/tablet。图片不读取名称；缺少平台或形态时显示中性图标。任何图片选择都不改变名称、平台文字、IP、信任或连接状态。兼容与系统分类边界见[终端联动协议](../../../docs/DEVICE_LINK_PROTOCOL.md)。

Watch经iPhone转发且不独立配对，仅在对应iPhone卡片内的附属区域显示40×48px等比contain小图、真实名称与在线状态，不生成独立卡片或操作入口。现有协议没有型号字段，图片不用于推断型号。
