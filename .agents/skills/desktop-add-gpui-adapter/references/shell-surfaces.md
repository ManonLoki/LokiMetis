# GPUI 侧栏与条件支持页

只在初始化或修改对应机制时读取。当前产品事实优先于中性 renderer；此处保留装配合同，不触发重生成。

- compact 为 80px、6px 内容内边距、36px Logo、22px 图标、56px 全宽居中竖排名称，完全没有折叠按钮或折叠翻译键。detailed 为展开 248px、收起 76px、72px/44px Logo、22px 图标、44px 菜单行与 Tooltip；展开图标/名称在满宽内行左对齐，收起图标居中。折叠按钮为 24px 悬浮控件，中心落在侧栏右边界并与顶部 Logo 垂直居中，展开/收起均跨边界各 12px；最后绘制且整个按钮可点击，不占独立菜单行；折叠为独立设备偏好。侧栏与内容偏移消费同一宽度，布局不额外补固定 margin。功能菜单向下增长；底部按已选赞助 → 设置 → 已选关于生成。原生列表页使用 GPUI 的类型化实体/组件设计，不要求 Mantine、TanStack 或 Jotai；跨页面会话视图由进程内实体持有，不能持久化业务数据副本。

- `about_page: enabled` 才生成 `app/about.rs`、枚举/导航/展示状态与词典键；页面包含当前应用、负责人、品牌联系方式、免责声明、双语更新日志入口和真实不可用更新状态。中性基线未实现 GPUI updater，按钮明确反馈 unavailable/零出站，不能伪装已安装 Tauri updater 或检查成功。`build.rs` 在候选工具提供 `HARNESS_GPUI_RELEASE_NOTES_RS` 时，把经过根 schema v2 校验并安全转义的最近十版 `(date, version, features zh/en, bugs zh/en)` 静态条目及原 JSON 字节复制到 Cargo `OUT_DIR`；env 与路径内容变化均触发重编译。普通开发/初始化未提供该变量时明确空状态；窗口线程不读取磁盘、JSON 或网络。禁用关于页不生成不可达页面或翻译，候选仍嵌入并打包原更新日志字节供独立 GPUI 构建 Skill 核对。

- `sponsor_page: enabled` 才生成 [固定原生赞助模板](../assets/gui/src/app/sponsor.rs)、三档权益卡片、独立支付区、完整媒体和词典。赞助页拥有完整背景/留白，父内容槽不生成额外“赞助”标题、顶部底线或内边距；页面 `flex_grow(1)`、`flex_shrink_0` 与父列滚动约束共同防止整页位移。完整居中介绍含维护边界、能力图标与 PC 说明；按扣除当前侧栏后的内容宽度 ≥1000px 三列，否则一列，支付区 ≥924px 左右排，否则上下排。源码布局参考优化过的 gpui-demo，内容来自自含受管品牌源 [assets/brand-support](../assets/brand-support)。固定品牌、价格、联系方式、支付二维码与免责声明为已有批准的产品家族例外；媒体逐字节校验 `media-manifest.json` 大小/SHA-256，不复制参考应用业务、凭据、Logo 或远端地址。二维码保留 120×136、白底与原比例，只静态展示，不授权支付自动化。禁用时 sponsor 源码/回归、词典、媒体和运行时引用全部缺席。来源和许可责任见 [SOURCE.md](../assets/brand-support/SOURCE.md)。
