//! 应用图标在编译期嵌入；其他图标继续使用 GPUI Kit 自带的资源。

use gpui_kit::{AssetSource, Result, SharedString};
use std::borrow::Cow;

/// 窗口内共享的已选择应用图标路径。
pub const LOGO: &str = "app-identity/logo.@@LOGO_EXT@@";

/// 固定本地资源源不接受任意文件路径或远程 URL。
pub struct Assets;

impl AssetSource for Assets {
    /// 精确匹配应用图标，其他资源交由组件库自有资源处理。
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path == LOGO {
            return Ok(Some(Cow::Borrowed(include_bytes!("../assets/logo.@@LOGO_EXT@@"))));
        }
@@SPONSOR_ASSETS_LOAD@@
        gpui_kit::assets::Assets.load(path)
    }

    /// 只枚举当前编译包实际含有的资源。
    fn list(&self, prefix: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(prefix)?;
        if LOGO.starts_with(prefix) {
            paths.push(LOGO.into());
        }
@@SPONSOR_ASSETS_LIST@@
        Ok(paths)
    }
}
