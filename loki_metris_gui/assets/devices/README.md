# 设备卡片示意图

四张PNG由内置ImageGen于2026-10-21生成，原样嵌入应用；不是厂商官网照片，不表示实际设备型号。完整提示词见[imagegen.json](imagegen.json)。

- `iphone.png`：经典深色iPhone，蓝灰抽象屏幕。
- `android-phone.png`：经典深色Android手机，浅绿抽象屏幕。
- `ipad.png`：竖向iPad，蓝灰抽象屏幕。
- `android-tablet.png`：横向Android平板，浅绿抽象屏幕。

图片保留透明通道，图像槽固定184px高，等比contain，不裁切。设备配图是装饰：平台来自认证DeviceInfo，手机/平板形态来自同一认证连接在deviceFormFactorV1协商后上报的phone/tablet。图片不读取名称；缺少平台或形态时显示中性图标。任何图片选择都不改变名称、平台文字、IP、信任或连接状态。兼容与系统分类边界见[终端联动协议](../../../docs/DEVICE_LINK_PROTOCOL.md)。

Watch经iPhone转发且不独立配对，只显示对应iPhone卡片内的附属状态，不生成独立图片。
