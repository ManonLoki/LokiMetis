# 本地资源来源

应用 Logo 来自调用方明确选择的 PNG；正式身份核对由初始化器负责。

本次用户要求历史项目1:1复刻，并明确授权引入达成目标所需资源；设置页内联赞助区因此只迁入参考项目已展示的两张本地付款图。来源为 `/Users/manonloki/Documents/my-work/loki_metis/loki_metis_gui/public/brand-support/sponsor/`，原始字节未处理：

- `sponsor/wechat-pay.png` SHA256 `ad07658afe3daf83d31c447c9f16aef057e83e09c0e4c3b1d493d3f5d9627442`。
- `sponsor/alipay.jpg` SHA256 `421bd0a127dfbdc987f3b47c9de88dd00758ab3fd919bc03722607ebc68044b1`。

两张来源文件原样编译嵌入，未进行解码重编码或重绘二维码；运行时只为屏幕展示解码，不发起支付、交易或远程请求。

侧栏和设置图标为 Phosphor Icons 2.1.10 的 SquaresFour、ChartLineUp、Palette、GearSix、ClockCounterClockwise、GithubLogo、WarningCircle、ArrowsClockwise regular，路径来自历史项目已声明的 `node_modules/@phosphor-icons/react/dist/defs/*.es.js`；只提取原path为独立SVG，保留完整MIT许可于 `icons/Phosphor-LICENSE.txt`，不引入React运行时。

更新日志关闭符号与复选钩来自历史项目 Mantine 9.6.0 的 `node_modules/@mantine/core/esm/components/CloseButton/CloseIcon.mjs` 和 `Checkbox/CheckIcon.mjs`；分别保留15×15和10×7 viewBox与evenodd填充规则，完整MIT许可于 `icons/Mantine-LICENSE.txt`。
