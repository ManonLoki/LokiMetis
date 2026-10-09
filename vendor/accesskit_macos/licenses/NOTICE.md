# LokiMetris 修改声明

此文件由 LokiMetris 维护，不是原样上游许可证。

上游：AccessKit `accesskit_macos 0.26.3`，源提交
`c88605b96d04431f9c3c792464a0f2f253480e94`。
官方仓库：https://github.com/AccessKit/accesskit

LokiMetris 仅在构建生成的节点模块中应用标题兼容补丁：

- Heading 原生角色使用 `AXHeading`。
- 正数 Heading 层级通过 `AXHeadingLevel` 声明并返回。
- 没有显式值的正层级 Heading 通过整数 `AXValue` 返回层级；显式值保持优先。
- 保留原有盲文、父类属性及其它角色；不修改 Label setter。

原始第三方源码位于相邻 `upstream/`，保持所有字节和版权注释不变。
本目录的 MIT、Apache-2.0 和 Chromium 许可文本均来自该精确官方提交；
具体 URL 与摘要见相邻 `provenance.json`。
本说明不替换或修改任何上游许可条款。
