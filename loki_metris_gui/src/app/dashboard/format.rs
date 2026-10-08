//! 看板展示格式与参考产品一致，整数运算避免大 Token 的浮点精度损失。

/// 十进制每三位分组，只影响展示，复制仍使用原始精确整数。
pub(super) fn exact(value: u64) -> String {
    let digits = value.to_string();
    let mut output = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) { output.push(','); }
        output.push(digit);
    }
    output
}

/// K/M/B 固定两位小数；四舍五入跨 1000 时晋级，最大单位仍加千分位。
pub(super) fn compact(value: u64) -> String {
    if value < 1_000 { return exact(value) }
    let units = [(1_000_u128, "K"), (1_000_000, "M"), (1_000_000_000, "B")];
    let mut index = units.iter().rposition(|(threshold, _)| u128::from(value) >= *threshold).unwrap_or(0);
    let mut rounded = (u128::from(value) * 100 + units[index].0 / 2) / units[index].0;
    if rounded >= 100_000 && index < units.len() - 1 {
        index += 1;
        rounded = (u128::from(value) * 100 + units[index].0 / 2) / units[index].0;
    }
    format!("{}.{:02}{}", exact((rounded / 100) as u64), rounded % 100, units[index].1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 半入、跨单位和 u64 上界保持精确且与参考小数规则一致。
    #[test]
    fn token_format_preserves_rounding_and_large_integers() {
        assert_eq!(exact(73_461_934), "73,461,934");
        assert_eq!(compact(999), "999");
        assert_eq!(compact(1_005), "1.01K");
        assert_eq!(compact(999_995), "1.00M");
        assert_eq!(compact(73_461_934), "73.46M");
        assert_eq!(compact(u64::MAX), "18,446,744,073.71B");
    }
}
