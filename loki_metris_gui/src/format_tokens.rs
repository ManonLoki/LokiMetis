//! 看板与桌宠共用整数 Token 格式，不经过浮点运算。

/// 十进制每三位分组，只影响展示，复制仍使用原始精确整数。
pub(crate) fn exact(value: u64) -> String {
    let digits = value.to_string();
    let mut output = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            output.push(',');
        }
        output.push(digit);
    }
    output
}

/// K/M/B 固定两位小数；四舍五入跨 1000 时晋级，最大单位仍加千分位。
pub(crate) fn compact(value: u64) -> String {
    if value < 1_000 {
        return exact(value);
    }
    let units = [(1_000_u128, "K"), (1_000_000, "M"), (1_000_000_000, "B")];
    let mut index = units
        .iter()
        .rposition(|(threshold, _)| u128::from(value) >= *threshold)
        .unwrap_or(0);
    let mut rounded = (u128::from(value) * 100 + units[index].0 / 2) / units[index].0;
    if rounded >= 100_000 && index < units.len() - 1 {
        index += 1;
        rounded = (u128::from(value) * 100 + units[index].0 / 2) / units[index].0;
    }
    format!(
        "{}.{:02}{}",
        exact((rounded / 100) as u64),
        rounded % 100,
        units[index].1
    )
}
