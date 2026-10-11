// 自有生成补丁：只为原生可变字体补全 wght 实例和尺寸无关的反查身份。
use anyhow::{Result, bail};
use cocoa::appkit::CGFloat;
use core_foundation::{
    base::{CFEqual, CFType, TCFType},
    dictionary::{CFDictionary, CFDictionaryRef},
    number::CFNumber,
    string::{CFString, CFStringRef},
};
use core_graphics::geometry::CGAffineTransform;
use core_text::{
    font::{CTFont, CTFontRef},
    font_descriptor::{
        CTFontDescriptor, CTFontDescriptorCreateCopyWithVariation, CTFontDescriptorRef,
        kCTFontCascadeListAttribute, kCTFontFeatureSettingsAttribute,
    },
};
use font_kit::font::Font as FontKitFont;

mod semantic_identity {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/semantic_identity.rs"
    ));
}
mod descriptor_identity {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/descriptor_identity.rs"
    ));
}
use descriptor_identity::semantic_value;
pub(super) use semantic_identity::NativeFontIdentity;
use semantic_identity::matrix_bits;

/// OpenType 原始轴标签；CSS 字重直接设置设计坐标，不能误用归一化 CT trait。
const WEIGHT_AXIS: u32 = u32::from_be_bytes(*b"wght");
/// 所有非字重轴只参与实际身份，不由此补丁改变。
#[derive(Clone, Copy, Debug)]
struct Axis {
    identifier: u32,
    minimum: f64,
    maximum: f64,
    default: f64,
}

/// 缺失或非有限轴数值不允许被过滤后伪装成另一有效轴集合。
fn axis_number(dictionary: &CFDictionary<CFString, CFType>, key: CFStringRef) -> Option<f64> {
    let value = dictionary.find(key)?.downcast::<CFNumber>()?.to_f64()?;
    value.is_finite().then_some(value)
}

/// 对完整轴定义做校验，静态字体返回空集合。
fn axes(font: &CTFont) -> Option<Vec<Axis>> {
    let Some(definitions) = font.get_variation_axes() else {
        return Some(Vec::new());
    };
    let mut result = Vec::new();
    for dictionary in definitions.iter() {
        let axis = unsafe {
            let id = axis_number(&dictionary, kCTFontVariationAxisIdentifierKey)?;
            if id.fract() != 0.0 || !(0.0..=u32::MAX as f64).contains(&id) {
                return None;
            }
            Axis {
                identifier: id as u32,
                minimum: axis_number(&dictionary, kCTFontVariationAxisMinimumValueKey)?,
                maximum: axis_number(&dictionary, kCTFontVariationAxisMaximumValueKey)?,
                default: axis_number(&dictionary, kCTFontVariationAxisDefaultValueKey)?,
            }
        };
        if axis.minimum > axis.default
            || axis.default > axis.maximum
            || result
                .iter()
                .any(|a: &Axis| a.identifier == axis.identifier)
        {
            return None;
        }
        result.push(axis);
    }
    Some(result)
}

/// Copy Rule 包装只在当前栈存在，不将 CFDictionary 存入任何全局状态。
fn variation_dictionary(font: &CTFont) -> Option<CFDictionary<CFNumber, CFNumber>> {
    let pointer = unsafe { CTFontCopyVariation(font.as_concrete_TypeRef()) };
    (!pointer.is_null()).then(|| unsafe { CFDictionary::wrap_under_create_rule(pointer) })
}

/// 实际坐标完整验证；默认坐标和负零归一，未知轴仍保留标识和值。
fn coordinates(font: &CTFont, definitions: &[Axis]) -> Option<Vec<(u32, u64)>> {
    let Some(dictionary) = variation_dictionary(font) else {
        return Some(Vec::new());
    };
    let (keys, values) = dictionary.get_keys_and_values();
    let mut result = Vec::new();
    for (key, value) in keys.into_iter().zip(values) {
        let key = unsafe { CFType::wrap_under_get_rule(key) }.downcast::<CFNumber>()?;
        let value = unsafe { CFType::wrap_under_get_rule(value) }.downcast::<CFNumber>()?;
        let id = u32::try_from(key.to_i64()?).ok()?;
        let value = value.to_f64()?;
        if !value.is_finite() {
            return None;
        }
        if definitions
            .iter()
            .any(|axis| axis.identifier == id && axis.default == value)
        {
            continue;
        }
        let value = if value == 0.0 { 0.0 } else { value };
        result.push((id, value.to_bits()));
    }
    result.sort_unstable_by_key(|(id, _)| *id);
    if result.windows(2).any(|p| p[0].0 == p[1].0) {
        return None;
    }
    Some(result)
}

/// 明确区分属性缺省与属性存在但转换失败。
fn attribute(font: &CTFont, key: CFStringRef) -> Option<CFType> {
    font.copy_descriptor()
        .attributes()
        .find(key)
        .map(|value| (*value).clone())
}

/// 矩阵使用实际 CTFont 值，不依赖 descriptor 是否显式携带矩阵属性。
fn actual_matrix(font: &CTFont) -> Option<[u64; 6]> {
    let m = font.get_matrix();
    matrix_bits([m.a, m.b, m.c, m.d, m.tx, m.ty])
}

/// 主字号只由 GPUI 的 raster/layout 参数管理；不吞掉 features/cascade 的未知值。
pub(super) fn native_identity(font: &CTFont) -> Option<NativeFontIdentity> {
    let features = unsafe { attribute(font, kCTFontFeatureSettingsAttribute) };
    let cascade = unsafe { attribute(font, kCTFontCascadeListAttribute) };
    Some(NativeFontIdentity {
        postscript_name: font.postscript_name(),
        coordinates: coordinates(font, &axes(font)?)?,
        features: features.as_ref().map(semantic_value).transpose_option()?,
        cascade: cascade.as_ref().map(semantic_value).transpose_option()?,
        matrix: actual_matrix(font)?,
    })
}

/// 将 Option<Option<T>> 的未知值与缺省值分开，不引入外部依赖。
trait TransposeOption<T> {
    fn transpose_option(self) -> Option<Option<T>>;
}
impl<T> TransposeOption<T> for Option<Option<T>> {
    fn transpose_option(self) -> Option<Option<T>> {
        match self {
            None => Some(None),
            Some(value) => value.map(Some),
        }
    }
}

/// 原生属性按完整容器 CFEqual 比较；未知嵌套节点不会被丢弃或替换成通用哨兵。
fn full_value_equal(left: Option<CFType>, right: Option<CFType>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => unsafe {
            CFEqual(left.as_CFTypeRef(), right.as_CFTypeRef()) != 0
        },
        _ => false,
    }
}

/// 未知类型走已有字体的原生值比较；CFEqual 仅发生在栈上，无 CFHash/Send 扩展。
pub(super) fn native_equivalent(left: &CTFont, right: &CTFont) -> bool {
    if left.postscript_name() != right.postscript_name()
        || actual_matrix(left).is_none()
        || actual_matrix(left) != actual_matrix(right)
    {
        return false;
    }
    let left_coordinates = axes(left).and_then(|a| coordinates(left, &a));
    let right_coordinates = axes(right).and_then(|a| coordinates(right, &a));
    if let (Some(a), Some(b)) = (&left_coordinates, &right_coordinates) {
        if a != b {
            return false;
        }
    } else if variation_dictionary(left) != variation_dictionary(right) {
        return false;
    }
    unsafe {
        full_value_equal(
            attribute(left, kCTFontFeatureSettingsAttribute),
            attribute(right, kCTFontFeatureSettingsAttribute),
        ) && full_value_equal(
            attribute(left, kCTFontCascadeListAttribute),
            attribute(right, kCTFontCascadeListAttribute),
        )
    }
}

/// CoreText 省略默认轴时按字体自身默认值解释，绝不以请求值充当 readback。
fn effective_coordinate(font: &CTFont, definitions: &[Axis], axis: Axis) -> Option<f64> {
    Some(
        coordinates(font, definitions)?
            .into_iter()
            .find(|(id, _)| *id == axis.identifier)
            .map(|(_, bits)| f64::from_bits(bits))
            .unwrap_or(axis.default),
    )
}

/// 静态/默认请求沿用候选；只修改有效 wght 轴，保留完整原描述符及矩阵。
pub(super) fn weight_instance(font: &CTFont, requested: f32) -> Result<Option<FontKitFont>> {
    if !requested.is_finite() {
        return Ok(None);
    }
    let Some(definitions) = axes(font) else {
        return Ok(None);
    };
    let Some(axis) = definitions
        .iter()
        .find(|a| a.identifier == WEIGHT_AXIS)
        .copied()
    else {
        return Ok(None);
    };
    let requested = (requested as f64).clamp(axis.minimum, axis.maximum);
    if effective_coordinate(font, &definitions, axis) == Some(requested) {
        return Ok(None);
    }
    let descriptor = font.copy_descriptor();
    let id = CFNumber::from(i64::from(WEIGHT_AXIS));
    let pointer = unsafe {
        CTFontDescriptorCreateCopyWithVariation(
            descriptor.as_concrete_TypeRef(),
            id.as_concrete_TypeRef(),
            requested,
        )
    };
    if pointer.is_null() {
        bail!("CoreText could not create a wght descriptor");
    }
    let varied = unsafe { CTFontDescriptor::wrap_under_create_rule(pointer) };
    let pointer = unsafe {
        CTFontCreateCopyWithAttributes(
            font.as_concrete_TypeRef(),
            0.0,
            std::ptr::null(),
            varied.as_concrete_TypeRef(),
        )
    };
    if pointer.is_null() {
        bail!("CoreText could not create a wght font instance");
    }
    let instance = unsafe { CTFont::wrap_under_create_rule(pointer) };
    if effective_coordinate(&instance, &definitions, axis) != Some(requested) {
        bail!("CoreText did not retain the requested wght coordinate");
    }
    // from_native_font 直接 clone CTFont；CGFont 往返会丢失原 descriptor 上下文。
    Ok(Some(unsafe { FontKitFont::from_native_font(&instance) }))
}

/// 回归读取真实有效 wght；不改变生产行为或另存请求值。
#[cfg(test)]
pub(super) fn effective_weight(font: &CTFont) -> Option<f64> {
    let definitions = axes(font)?;
    let axis = *definitions.iter().find(|a| a.identifier == WEIGHT_AXIS)?;
    effective_coordinate(font, &definitions, axis)
}

#[link(name = "CoreText", kind = "framework")]
unsafe extern "C" {
    /// SDK 原始轴标识键。
    static kCTFontVariationAxisIdentifierKey: CFStringRef;
    /// 原始最小轴坐标键。
    static kCTFontVariationAxisMinimumValueKey: CFStringRef;
    /// 原始最大轴坐标键。
    static kCTFontVariationAxisMaximumValueKey: CFStringRef;
    /// 原始默认轴坐标键。
    static kCTFontVariationAxisDefaultValueKey: CFStringRef;
    /// 拷贝实际轴坐标，由当前调用栈按 Copy Rule 释放。
    fn CTFontCopyVariation(font: CTFontRef) -> CFDictionaryRef;
    /// 修改当前字体的属性，size=0 与 matrix=null 保留原字号及变换。
    pub(super) fn CTFontCreateCopyWithAttributes(
        font: CTFontRef,
        size: CGFloat,
        matrix: *const CGAffineTransform,
        attributes: CTFontDescriptorRef,
    ) -> CTFontRef;
}
