// 自有真实 CoreFoundation 回归，不将 CFType 跨线程存储。
use super::*;
use core_foundation::{array::CFArray, date::CFDate, dictionary::CFDictionary};

/// 大整数保持精确身份；整数/实数相同值统一，NaN/无穷拒绝键转换。
#[test]
fn real_cf_numbers_preserve_exact_value_semantics() {
    assert_ne!(
        semantic_value(&CFNumber::from(9_007_199_254_740_993i64).as_CFType()),
        semantic_value(&CFNumber::from(9_007_199_254_740_992f64).as_CFType())
    );
    assert_eq!(
        semantic_value(&CFNumber::from(650i64).as_CFType()),
        semantic_value(&CFNumber::from(650f64).as_CFType())
    );
    assert_eq!(
        semantic_value(&CFNumber::from(-0f64).as_CFType()),
        semantic_value(&CFNumber::from(0i64).as_CFType())
    );
    assert_eq!(semantic_value(&CFNumber::from(f64::NAN).as_CFType()), None);
    assert_eq!(
        semantic_value(&CFNumber::from(f64::INFINITY).as_CFType()),
        None
    );
}

/// 原生字典键顺序不影响身份，原生 feature/cascade 数组顺序必须保留。
#[test]
fn real_cf_dictionary_and_array_order() {
    let a = CFString::new("a");
    let b = CFString::new("b");
    let first =
        CFDictionary::from_CFType_pairs(&[(a.clone(), b.as_CFType()), (b.clone(), a.as_CFType())]);
    let second =
        CFDictionary::from_CFType_pairs(&[(b.clone(), a.as_CFType()), (a.clone(), b.as_CFType())]);
    assert_eq!(
        semantic_value(&first.as_CFType()),
        semantic_value(&second.as_CFType())
    );
    assert_ne!(
        semantic_value(&CFArray::from_CFTypes(&[a.clone(), b.clone()]).as_CFType()),
        semantic_value(&CFArray::from_CFTypes(&[b, a]).as_CFType())
    );
}

/// URL、原样数据、字符集和 CTFontDescriptor 均能完整转换为 Rust 所有权。
#[test]
fn real_cf_supported_descriptor_types_are_not_dropped() {
    let data = CFData::from_buffer(&[0, 1, 255]);
    assert_eq!(
        semantic_value(&data.as_CFType()),
        Some(DescriptorValue::Data(vec![0, 1, 255]))
    );
    let url = CFURL::from_path("/tmp/Loki Metris Font Probe.ttf", false).unwrap();
    assert!(matches!(
        semantic_value(&url.as_CFType()),
        Some(DescriptorValue::Url(_))
    ));
    let letters = CFString::new("ab中");
    let set = unsafe {
        CFCharacterSetCreateWithCharactersInString(
            kCFAllocatorDefault,
            letters.as_concrete_TypeRef(),
        )
    };
    assert!(!set.is_null());
    let set = unsafe { CFType::wrap_under_create_rule(set as _) };
    assert!(
        matches!(semantic_value(&set),Some(DescriptorValue::CharacterSet(bytes)) if !bytes.is_empty())
    );
    let attributes = CFDictionary::from_CFType_pairs(&[
        (
            CFString::new("ProbeString"),
            CFString::new("retained").as_CFType(),
        ),
        (CFString::new("ProbeData"), data.as_CFType()),
        (CFString::new("ProbeUrl"), url.as_CFType()),
    ]);
    let descriptor = core_text::font_descriptor::new_from_attributes(&attributes);
    assert!(
        matches!(semantic_value(&descriptor.as_CFType()),Some(DescriptorValue::FontDescriptor(entries)) if entries.len()==3)
    );
    let boolean = unsafe { CFType::wrap_under_get_rule(kCFBooleanTrue as _) };
    assert_eq!(
        semantic_value(&boolean),
        Some(DescriptorValue::Boolean(true))
    );
    let null = unsafe { CFType::wrap_under_get_rule(kCFNull) };
    assert_eq!(semantic_value(&null), Some(DescriptorValue::Null));
}

/// 未支持原生类型和异常深度都返回 None，不忽略该节点或制造通用 Unsupported 键。
#[test]
fn real_unknown_and_deep_values_request_native_fallback() {
    assert_eq!(semantic_value(&CFDate::new(123.).as_CFType()), None);
    let mut nested = CFString::new("leaf").as_CFType();
    for _ in 0..66 {
        nested = CFArray::from_CFTypes(&[nested]).as_CFType();
    }
    assert_eq!(semantic_value(&nested), None);
}
