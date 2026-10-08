//! 接口与运行时无关的中性核心，等待独立产品定义后增加真实用例。

/// 中性脚手架没有已批准的产品功能。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductDefinition {
    Undefined,
}

/// 各适配器共享同一个中性产品定义事实。
pub const fn product_definition() -> ProductDefinition {
    ProductDefinition::Undefined
}

#[cfg(test)]
mod tests {
    use super::{ProductDefinition, product_definition};

    /// 初始化不得凭空宣称产品已经定义。
    #[test]
    fn neutral_scaffold_has_no_product_definition() {
        assert_eq!(product_definition(), ProductDefinition::Undefined);
    }
}
