//! 可复用的中性初始化核心；产品定义获批前不得加入或推测业务行为。

#![deny(missing_docs)]

/// 表示工程骨架是否就绪，以及是否仍需补充产品定义。
#[derive(Debug, PartialEq, Eq)]
pub struct ScaffoldStatus {
    /// 标识项目的中性共享核心已经完成结构初始化。
    pub initialized: bool,
    /// 标识产品目的、核心输入输出和成功标准尚待明确。
    pub product_definition_required: bool,
}

/// 查询不带任何业务假设或外部副作用的中性脚手架状态。
///
/// 该异步 API 供 GUI 薄适配器复用，但不绑定 Tokio 类型、序列化格式或界面状态。
pub async fn scaffold_status() -> ScaffoldStatus {
    ScaffoldStatus {
        initialized: true,
        product_definition_required: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证中性 core 只报告结构已经初始化，同时明确要求后续产品定义。
    #[tokio::test]
    async fn reports_that_product_definition_is_required() {
        assert_eq!(
            scaffold_status().await,
            ScaffoldStatus {
                initialized: true,
                product_definition_required: true,
            }
        );
    }
}
