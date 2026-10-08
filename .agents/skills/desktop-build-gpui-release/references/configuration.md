# GPUI 打包配置合同

根 `package.json` 的 `scripts.gpui:package` 固定调用 `node .agents/skills/desktop-build-gpui-release/scripts/build_gpui_release.mjs`，`engines.node` 与 `engines.pnpm` 分别声明 `>=24.21.0`、`>=12.4.1`。通过 `pnpm run gpui:package preview|setup|icons|build ...` 原样传入子命令和参数，不额外插入 `--`；`pnpm run gpui:package --help` 只显示底层 helper 帮助。纯 GPUI 模板没有第三方 npm 依赖，运行这些工程命令不需要先执行 `pnpm install`，也不要求前端单元测试或 `pnpm-lock.yaml`。

初始化器调用 `$desktop-add-gpui-adapter` 的 `scripts/gpui_node_tooling.mjs` 中 `renderGpuiPackageJson(existingBytes = null)` 生成或受限合并根清单。根 `package.json` 仍是升级器默认保护的产品文件，不得放入受管候选整体替换。既有项目在工程层升级后，可明确执行以下专用合并；只补齐缺失的受管脚本和运行时声明，已有非等值受管字段冲突时停止，保留其它产品字段与依赖。

```sh
node .agents/skills/desktop-add-gpui-adapter/scripts/merge_gpui_node_tooling.mjs merge --root .
pnpm run gpui:package --help
```

根 `packaging/gpui.json` 只持有打包身份/资产；版本来自选定 Cargo workspace package。生成器调用 `scripts/gpui_config.mjs` 的 `renderDefaultConfig({projectId,nameZh,nameEn,owner,logoPath})`，不得手抄示例产品身份。

```json
{
  "schemaVersion": 1,
  "package": "example_gui",
  "binary": "example_gui",
  "productName": "示例",
  "productNameEn": "Example",
  "identifier": "com.example.app",
  "publisher": "Owner",
  "icons": {
    "source": "example_gui/assets/logo.png",
    "macos": "packaging/icons/app.icns",
    "windows": "packaging/icons/app.ico"
  },
  "macos": {
    "minimumSystemVersion": "12.0",
    "signingIdentity": null,
    "background": "packaging/macos/background.png"
  },
  "windows": { "signing": null, "extractor": null },
  "nsis": {
    "installMode": "currentUser",
    "languages": ["SimpChinese", "English"],
    "displayLanguageSelector": true
  }
}
```

- 所有字段必须存在，未知字段、重复 JSON 键、控制字符、`${`、不规范/越界/符号链接路径均拒绝。原生图标、DMG 背景与两份许可证必须是项目内普通文件。`minimumSystemVersion` 至少 11.0。
- `windows.extractor` 仅可填写项目内已批准/声明的 `7z.exe` 或 `7zr.exe` 相对路径；调用协议为 `x -y -o<本次隔离目录> <最终NSIS>`。Skill 不下载安装它。支持哪种 NSIS 压缩由实际提取结果决定，解包失败或缺资源阻断候选。
- `windows.signing` 启用时形如 `{ "certificateThumbprint": "40位十六进制公开指纹", "timestampUrl": "https://批准的时间戳服务" }`。证书只通过宿主已授权 store 使用，不保存私钥、密码或秘密路径。macOS 签名当前不可用，填 `signingIdentity` 不会自动启用。
- Cargo metadata 的 `rust-test-manifests` 默认 `["Cargo.toml"]`；每个独立 workspace 都须列出。`gui-root` 缺省从明确的 GUI package manifest 得出，根 package 可用 `"."`。`dependency-lock-policy` 缺省 `ignored`；`tracked` 走既有锁文件门禁，不生成或暂存锁文件。
- 初始化器可调用 `scripts/gpui_icons.mjs` 的 `renderPlatformIcons(pngBytes)` 得到 `{macos,windows}` buffers，调用 `renderDmgBackground()` 得到中性背景 PNG；均为纯 Node 标准库，生成时组合到文件 Map，不自行覆盖现有工程。

构建临时 Rust 日志模块声明固定为：

```rust
pub const RELEASE_NOTES_JSON: &[u8] = &[];
pub const RELEASE_NOTES: &[(&str, &str, &[(&str, &str)], &[(&str, &str)])] = &[];
```

每个元组依次是 ISO 日期、带一个小写 `v` 的版本、功能优化的中文/英文条目对、问题修复的条目对。候选且 `about_page: enabled` 时 helper 填入真实日志，并要求二进制实际保留原始字节，例如 `std::hint::black_box(RELEASE_NOTES_JSON)`，避免编译器移除证据；文本展示用静态条目，未知 locale 回退英文。local 或关于页禁用时不设置 `HARNESS_GPUI_RELEASE_NOTES_RS`，`build.rs` 写空模块；关于页禁用仍核对包内独立 JSON 资源。此标记本身不证明 UI 行为，真实候选 E2E 仍按当前选择执行。

配置/产物字段已在 2026-10-08 核对 [cargo-packager 0.11.8 API](https://docs.rs/cargo-packager/0.11.8/cargo_packager/config/struct.Config.html)、[资源映射](https://docs.crabnebula.dev/packager/configuration/#resources) 与 [DMG 配置](https://docs.rs/cargo-packager/0.11.8/cargo_packager/config/struct.DmgConfig.html)。固定版本沿用已验证示例的 `0.11.8`，不随全局工具版本变化。示例的 Docker/Windows 着色器路线未移入当前管线。

0.11.8 的 Applications 坐标 JSON 键实际是 `appFolderPosition`，不是自动推导的 `applicationFolderPosition`；构建 helper 已按实际工具解析核对。上游 `Context::new` 把 create-dmg/NSIS 支持工具放在 OS 用户缓存 `.cargo-packager`，并未开放 tools-dir 配置；只承诺 cargo-packager CLI 项目本地安装，不把其内部缓存冒充项目隔离。
