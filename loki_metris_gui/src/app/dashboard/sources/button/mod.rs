//! Sources 保留既有名称，唯一实现由可见历史按钮共用，避免复制反馈逻辑。

pub(super) use crate::app::surface::{
    SurfaceButton as SourceButton, SurfaceButtonVariant as SourceButtonVariant,
};

#[cfg(test)]
mod tests;
