# 架构图交付记录

- diagram_type: architecture
- output: `desktop.html`
- source_revision: `950d9eb0cf39b9f57f08a98cdd45c81dd8effac1`
- specification_sha256: `15e3d34aca6ffd1e97ba5a4a6bece5c526a808a85e1692783c594f0a5d01593c`
- artifact_sha256: `4663994d0a059b0a60d564a1199f9db4b774e51dea439651d8f6569cfcc41c84`
- validation: 9/9 showcase, 0 errors, 0 warnings
- browser_evidence: passed
- visual_review: passed
- correction_rounds: 1

当前有效完成回执为 [review-2/desktop.finalize.json](review-2/desktop.finalize.json)，浏览器回执为 [review-2/desktop.browser-check.json](review-2/desktop.browser-check.json)。同目录较早回执仅保存生成过程；验证当前 HTML 时使用上述最终记录。

## 源码与关系复核

已按固定提交核对节点、主要调用/读写关系和平台边界。sources 指向提交内真实文件及行号，当前文档与图不声明未经观察的服务。

## 截图与感知复核

已查看 `visual-check/` 中 1440×900、2048×1320 的浅色和深色四张截图。2048×1320 可见所有图节点与图例；1440×900 通过页面纵向滚动继续阅读。未见节点互相覆盖、文字越框或标注覆盖节点。
默认比例下细节字号较小，深色主题的灰色关系说明偏淡，可使用查看器放大。截图不覆盖首屏之外的全部说明卡片，因此感知通过范围限于已捕获的图面；浏览器自动检查另行通过。

自动路由记录 1 处已桥接交叉，作为路线复杂度提示保留，不等同于来源或自动检查失败。

一次位置调整将桥接交叉从 2 处降至 1 处；节点、关系、文字与来源未改变。
