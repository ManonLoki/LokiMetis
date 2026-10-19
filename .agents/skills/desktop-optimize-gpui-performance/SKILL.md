---
name: desktop-optimize-gpui-performance
description: 对 GPUI 原生 GUI 做有测量依据的性能审计或优化，定位运行时、重绘、图片与缓存成本；用于明确性能请求或已复现的成本问题，不随普通 GUI 开发自动跑基准。
---

# 优化 GPUI 性能

保留可观察行为，先固定场景和成本证据，再改最小机制。读取目标工程规则、Rust 基线和已有性能资料；存在 `docs/PERFORMANCE_OPTIMIZATION.md` / `docs/performance/README.md` 时先复用，不重写历史通过/失败结果。Harness 只保留通用方法，产品阈值、数据和历史测量留在下游。

- 定义源码、构建 profile、宿主/GPU、场景、数据量、窗口与可见性、预热、采样窗和进程边界。测量细节按 [measurement](references/measurement.md)，不自动安装 profiler 或启动生产工作负载。
- 线程、队列、定时器、重绘或退出问题读 [runtime-and-demand](references/runtime-and-demand.md)。图片、动画、GPU、列表或缓存问题读 [resources-and-caches](references/resources-and-caches.md)，不预读其他方向。
- 定位“每次成本 × 频率 × 同时存活量”，区分消除工作、合并工作和推迟工作。产品参数只按真实预算调整，不把某产品的 worker 数、队列长度、帧数或内存预算当通用默认。
- 在同一实施流程完成必要行为回归与同条件成本复测。取消、旧响应、隐藏/关闭、资源最后 owner、错误和退出属于高风险边界；只跑本次相关非空测试。
- 报告实测变化、回归结果、样本/平台局限及未测量项。若只有更少分配/通知的单元证据，就只作该层结论；不声称整应用更快或候选可用。

依赖或后端兼容变更交 `$desktop-manage-gpui-dependencies`；真实构建请求交 `$desktop-build-gpui-release`。性能任务本身不触发候选、E2E、发布或持久验收记录。
