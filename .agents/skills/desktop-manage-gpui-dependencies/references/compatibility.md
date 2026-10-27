# GPUI 兼容补丁的维护边界

只在真实症状需要时采用。默认先使用声明依赖的正常 API；本文件不要求所有下游复制 vendor 或固定某个 Zed revision。

已知方案是把官方原样归档、许可证、文件集合和 SHA-256 保存在独立 `upstream/`，由标准库生成器在 Cargo `OUT_DIR` 应用唯一锚点替换并验证输出摘要。人工维护 shim/生成器/补丁模板照常检查，不直接改原样内容或生成物；新版本锚点不匹配就停止，不能扩大正则使其“通过”。

需要联合核对的边界：

- 公共图元结构与圆角/纹理参数改变时，同时核对 GPUI 公共接口、Apple/Metal、Windows/HLSL 与 WGPU/WGSL 的布局、对齐、常量及 shader。
- 字体补丁按当前包结构定位。可变字体、粗体选择与字形 advance 分别用真实字体/平台路径验证；包重组后旧路径不是事实。
- 可访问性角色、焦点代数、命中排除、窗口 reopen/缩放等分别覆盖实际触发和迟到回调；声明通过不能替代目标运行。
- Linux 隐藏创建须检查后端的首次 map 路径，不能仅凭 `show=false` 判断没有窗口闪现。已批准的隐藏启动若确需补丁，只对匹配源码/锚点的版本生效，回归首次映射次数、后续显窗和关闭取消；不把 X11 结论扩张为 Wayland。启动来源、等待恢复入口及回退显示由宿主层拥有，详见 [Linux 登录项与隐藏启动](../../desktop-add-gpui-adapter/references/linux-native.md#登录项与隐藏启动)，不把产品行为塞进 GPUI 通用补丁。
- GPU atlas 的逻辑删除与物理区域重用分开，等编码完成/取消后回收；不可用固定“过两帧”替代真实生命周期。
- HRESULT 按 API 的成功语义判定；不能把所有非零值当失败，也不能由一个托盘补丁扩张到全部 COM API。

同一个产品曾组合 `gpui-pre`、`gpui-pre-apple`、`gpui-pre-macos`、`gpui-pre-windows`、`gpui-pre-wgpu` 五包 `0.3.9`，以及 `accesskit_macos 0.26.3`、`tray-icon 0.26.1`。这是兼容案例，不是中性 renderer 默认依赖或跨平台验收声明；具体 patch 必须匹配目标依赖链与原样摘要。产品专用 opt-in（如 Apple `parent-rounded-clip`）只在对应 target 开启。

回归分开报告来源/生成摘要、ABI/编译、真实 API 行为、GPU 像素及最终候选运行。macOS 的 Metal 结果不能替代 Windows DirectX 或 Linux WGPU。未执行部分明确标为未验证。
