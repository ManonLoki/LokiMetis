// 自有补丁：CoreFoundation 值只在当前调用栈存在，转换结果完全由 Rust 拥有。
use super::semantic_identity::{DescriptorValue, SemanticNumber};
use core_foundation::{
    base::{CFType, TCFType},
    data::CFData,
    number::CFNumber,
    string::CFString,
    url::CFURL,
};
use core_foundation_sys::{
    array::*, base::*, characterset::*, dictionary::*, number::*, string::*, url::*,
};
use core_text::font_descriptor::{CTFontDescriptor, CTFontDescriptorGetTypeID};

/// 限制异常递归描述符的转换成本；超界后进入逐字体原生比较，绝不截断成相同键。
struct Budget {
    nodes: usize,
    bytes: usize,
}
impl Budget {
    /// 每个属性值最多处理 8192 个节点及 1 MiB 值数据。
    fn new() -> Self {
        Self {
            nodes: 8192,
            bytes: 1024 * 1024,
        }
    }
    /// 扣减成功才允许复制，深度限制由递归入口负责。
    fn take(&mut self, bytes: usize) -> Option<()> {
        self.nodes = self.nodes.checked_sub(1)?;
        self.bytes = self.bytes.checked_sub(bytes)?;
        Some(())
    }
}

/// 逐 UTF-16 单元复制；不使用有损 UTF-8 转换合并不同的原生字符串。
fn string_value(value: CFStringRef, budget: &mut Budget) -> Option<String> {
    let len = usize::try_from(unsafe { CFStringGetLength(value) }).ok()?;
    budget.take(len.checked_mul(2)?)?;
    let mut units = vec![0; len];
    unsafe { CFStringGetCharacters(value, CFRange::init(0, len as isize), units.as_mut_ptr()) };
    String::from_utf16(&units).ok()
}

/// 字典以规范键排序；数组顺序和重复项由各自的分支完整保留。
fn dictionary(
    value: CFDictionaryRef,
    depth: usize,
    descriptor: bool,
    budget: &mut Budget,
) -> Option<DescriptorValue> {
    let count = usize::try_from(unsafe { CFDictionaryGetCount(value) }).ok()?;
    if count > budget.nodes {
        return None;
    }
    let mut keys = vec![std::ptr::null(); count];
    let mut values = keys.clone();
    unsafe { CFDictionaryGetKeysAndValues(value, keys.as_mut_ptr(), values.as_mut_ptr()) };
    let mut entries = Vec::with_capacity(count);
    for (key, value) in keys.into_iter().zip(values) {
        entries.push((
            convert(key, depth + 1, budget)?,
            convert(value, depth + 1, budget)?,
        ));
    }
    DescriptorValue::dictionary(entries, descriptor)
}

/// 每次调用确认运行时 TypeID；未知类型、失真数字或循环/过深属性一律拒绝键转换。
fn convert(pointer: CFTypeRef, depth: usize, budget: &mut Budget) -> Option<DescriptorValue> {
    if pointer.is_null() || depth > 64 {
        return None;
    }
    budget.take(0)?;
    let type_id = unsafe { CFGetTypeID(pointer) };
    if type_id == unsafe { CFNullGetTypeID() } {
        return Some(DescriptorValue::Null);
    }
    if type_id == unsafe { CFBooleanGetTypeID() } {
        return Some(DescriptorValue::Boolean(unsafe {
            CFBooleanGetValue(pointer as _)
        }));
    }
    if type_id == CFNumber::type_id() {
        let value = unsafe { CFNumber::wrap_under_get_rule(pointer as _) };
        let number = if unsafe { CFNumberIsFloatType(value.as_concrete_TypeRef()) } != 0 {
            SemanticNumber::real(value.to_f64()?)?
        } else {
            SemanticNumber::integer(value.to_i64()?)
        };
        return Some(DescriptorValue::Number(number));
    }
    if type_id == CFString::type_id() {
        return Some(DescriptorValue::Text(string_value(pointer as _, budget)?));
    }
    if type_id == CFData::type_id() {
        let value = unsafe { CFData::wrap_under_get_rule(pointer as _) };
        budget.take(value.bytes().len())?;
        return Some(DescriptorValue::Data(value.bytes().to_vec()));
    }
    if type_id == unsafe { CFArrayGetTypeID() } {
        let count = usize::try_from(unsafe { CFArrayGetCount(pointer as _) }).ok()?;
        if count > budget.nodes {
            return None;
        }
        let mut result = Vec::with_capacity(count);
        for ix in 0..count {
            result.push(convert(
                unsafe { CFArrayGetValueAtIndex(pointer as _, ix as isize) },
                depth + 1,
                budget,
            )?);
        }
        return Some(DescriptorValue::Array(result));
    }
    if type_id == unsafe { CFDictionaryGetTypeID() } {
        return dictionary(pointer as _, depth, false, budget);
    }
    if type_id == CFURL::type_id() {
        let absolute = unsafe { CFURLCopyAbsoluteURL(pointer as _) };
        if absolute.is_null() {
            return None;
        }
        let absolute = unsafe { CFURL::wrap_under_create_rule(absolute) };
        let text = string_value(
            unsafe { CFURLGetString(absolute.as_concrete_TypeRef()) },
            budget,
        )?;
        return Some(DescriptorValue::Url(text));
    }
    if type_id == unsafe { CFCharacterSetGetTypeID() } {
        let bitmap =
            unsafe { CFCharacterSetCreateBitmapRepresentation(kCFAllocatorDefault, pointer as _) };
        if bitmap.is_null() {
            return None;
        }
        let bitmap = unsafe { CFData::wrap_under_create_rule(bitmap) };
        budget.take(bitmap.bytes().len())?;
        return Some(DescriptorValue::CharacterSet(bitmap.bytes().to_vec()));
    }
    if type_id == unsafe { CTFontDescriptorGetTypeID() } {
        let descriptor = unsafe { CTFontDescriptor::wrap_under_get_rule(pointer as _) };
        return dictionary(
            descriptor.attributes().as_concrete_TypeRef(),
            depth,
            true,
            budget,
        );
    }
    None
}

/// None 表示整个已存在属性不能无损表达，不等于“该属性缺省”。
pub(super) fn semantic_value(value: &CFType) -> Option<DescriptorValue> {
    convert(value.as_CFTypeRef(), 0, &mut Budget::new())
}

/// 实际 CoreFoundation 值回归，与纯 Rust schema 测试分别登记。
#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/descriptor_identity_tests.rs"
    ));
}
