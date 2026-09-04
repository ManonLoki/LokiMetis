---
name: desktop-add-gui-deep-link
description: 为已选择 GUI 深链接能力的下游安装官方 deep-link 插件并固定受限协议边界；未选择时不得接入。
---

# 增加 GUI 深链接

只在 `docs/GUI_APP_PROFILE.md` 的 `deep_link = enabled`，或初始化后产品范围明确批准深链接时使用。

## 固定契约

1. 根 `[workspace.dependencies]` 声明 `tauri-plugin-deep-link = "2.4.10"`；GUI member 只以 `workspace = true` 继承。
2. 作为 Harness 的失败关闭组合门禁，`deep_link = enabled` 强制要求 `single_instance = enabled`；这不是 Tauri 对所有深链应用的通用要求，而是避免本模板生成“能冷启动但热启动丢 URL”组合的固定选择。single-instance 根依赖必须启用官方 `features = ["deep-link"]`，仍作为首 plugin；`.plugin(tauri_plugin_deep_link::init())` 紧随其后。
3. Harness 中性初始化固定采用静态、无业务载荷的 restore 路线：从已确认 `project_id` 确定性派生小写 scheme `app-<project-id 把下划线改为连字符>`，并只接受精确 URL `app-<kebab-id>://restore`。该值随八项 GUI 汇总展示并由用户确认；不另问业务 host/path，也不接受 query、fragment、userinfo、port 或任意 payload。
4. 该静态路线把 scheme 写入 `tauri.conf.json` 的 `plugins.deep-link.desktop.schemes`。Rust 使用 `DeepLinkExt` 同时处理冷启动 current URL 与热启动 `on_open_url`；统一校验函数通过后只调用 `restore_main_window`，非法 URL 返回稳定拒绝且不产生副作用。
5. 该能力是 Rust-only：不得安装 `@tauri-apps/plugin-deep-link`，不得给 WebView `deep-link:*` ACL，也不得把 argv 直接发给 React。以后需要业务路由、app/universal links 或外部 OAuth 回调时必须先批准产品事实，并把语义验证下沉 core。
6. 运行 `deep_link_uses_identity_derived_restore_url`、`deep_link_rejects_unconfigured_or_payload_urls`、`deep_link_routes_before_window_restore`、`deep_link_warm_launch_is_not_lost` 四个非空回归。初始化期至少验证解析器和宿主事件；macOS 自定义 scheme 的最终 OS 注册只能由已打包应用验证，debug no-bundle 不得冒充。
7. 禁用时依赖、single-instance `deep-link` feature、配置、监听、ACL、文案和测试残留都必须缺席。
8. Tauri 在 Windows/Linux 还支持桌面运行时注册；本中性静态路线不预先启用该更宽能力。若产品需要可移动 AppImage 路径、运行时注册/注销或其他 scheme，先批准产品范围，再补权限、所有权、清理和真实宿主验证，不能把本 Skill 的静态配置称为官方唯一方案。

## 完成输出

报告依赖下界、身份派生 restore URL、配置/冷热启动接线、与单实例的 feature/顺序、解析器与可执行宿主证据；未打包平台场景明确标为 `Not verified`。
