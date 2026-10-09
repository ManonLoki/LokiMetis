// 自有补丁：纯 Rust 字体身份；任何 CoreFoundation 指针都不能进入缓存键。

/// 整数无损保存；有限实数统一负零和可精确表示的整数。
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) enum SemanticNumber {
    Integer(i64),
    Real(u64),
}

impl SemanticNumber {
    /// CFNumber 整数直接读取 i64，避免大于 2^53 的整数经过 f64 丢精度。
    pub(super) fn integer(value: i64) -> Self {
        Self::Integer(value)
    }

    /// 不能无损规范化的数字触发原生比较回退，不生成近似缓存键。
    pub(super) fn real(value: f64) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        if value.fract() == 0.0
            && (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&value)
        {
            return Some(Self::Integer(value as i64));
        }
        Some(Self::Real(value.to_bits()))
    }
}

/// 数组保留顺序；字典保存完整规范值，绝不以 CFHash 或普通哈希摘要代替身份。
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(super) enum DescriptorValue {
    Null,
    Boolean(bool),
    Number(SemanticNumber),
    Text(String),
    Data(Vec<u8>),
    Array(Vec<Self>),
    Dictionary(Vec<(Self, Self)>),
    Url(String),
    CharacterSet(Vec<u8>),
    FontDescriptor(Vec<(Self, Self)>),
}

impl DescriptorValue {
    /// 归一后的键重复意味着无法无损表达，必须返回 None。
    pub(super) fn dictionary(mut entries: Vec<(Self, Self)>, descriptor: bool) -> Option<Self> {
        entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        if entries.windows(2).any(|p| p[0].0 == p[1].0) {
            return None;
        }
        Some(if descriptor {
            Self::FontDescriptor(entries)
        } else {
            Self::Dictionary(entries)
        })
    }
}

/// 排除主字号；实际轴、特性、级联和有效矩阵共同定义本补丁的反查身份。
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct NativeFontIdentity {
    pub(super) postscript_name: String,
    pub(super) coordinates: Vec<(u32, u64)>,
    pub(super) features: Option<DescriptorValue>,
    pub(super) cascade: Option<DescriptorValue>,
    pub(super) matrix: [u64; 6],
}

/// 实际 CTFontGetMatrix 消除属性缺省与单位矩阵的表示差异。
pub(super) fn matrix_bits(values: [f64; 6]) -> Option<[u64; 6]> {
    if values.iter().any(|x| !x.is_finite()) {
        return None;
    }
    Some(values.map(|x| {
        if x == 0.0 {
            0.0f64.to_bits()
        } else {
            x.to_bits()
        }
    }))
}

/// 编译期证明，不包含任何 unsafe Send/Sync 实现。
fn assert_send_sync<T: Send + Sync>() {}
const _: fn() = assert_send_sync::<NativeFontIdentity>;

#[cfg(test)]
mod tests {
    use super::*;
    /// 负零、整数实数统一，但精确大整数不能发生 f64 舍入合并。
    #[test]
    fn exact_number_identity() {
        assert_eq!(SemanticNumber::real(-0.0), Some(SemanticNumber::Integer(0)));
        assert_eq!(
            SemanticNumber::real(650.0),
            Some(SemanticNumber::integer(650))
        );
        assert_ne!(
            SemanticNumber::integer(9_007_199_254_740_993),
            SemanticNumber::real(9_007_199_254_740_992.0).unwrap()
        );
        assert_eq!(SemanticNumber::real(f64::NAN), None);
        assert_eq!(SemanticNumber::real(f64::INFINITY), None);
        assert_eq!(
            SemanticNumber::real(9_223_372_036_854_775_808.0),
            Some(SemanticNumber::Real(
                9_223_372_036_854_775_808.0f64.to_bits()
            ))
        );
    }
    /// 字典顺序无关；feature/cascade 数组顺序仍影响完整值。
    #[test]
    fn dictionary_order_and_array_order() {
        let a = DescriptorValue::Text("a".into());
        let b = DescriptorValue::Text("b".into());
        assert_eq!(
            DescriptorValue::dictionary(
                vec![(a.clone(), b.clone()), (b.clone(), a.clone())],
                false
            ),
            DescriptorValue::dictionary(
                vec![(b.clone(), a.clone()), (a.clone(), b.clone())],
                false
            )
        );
        assert_eq!(
            DescriptorValue::dictionary(
                vec![(a.clone(), b.clone()), (a.clone(), a.clone())],
                false
            ),
            None
        );
        assert_ne!(
            DescriptorValue::Array(vec![a.clone(), b.clone()]),
            DescriptorValue::Array(vec![b, a])
        );
    }
    /// 矩阵有限值无损保存，负零归一，未知值交给保守路径。
    #[test]
    fn matrix_normalization() {
        assert_eq!(
            matrix_bits([1., 0., -0., 1., 0., 0.]),
            matrix_bits([1., 0., 0., 1., 0., 0.])
        );
        assert_ne!(
            matrix_bits([1., 0., 0., 1., 0., 0.]),
            matrix_bits([1., 0.2, 0., 1., 0., 0.])
        );
        assert_eq!(matrix_bits([f64::NAN, 0., 0., 1., 0., 0.]), None);
    }
}
